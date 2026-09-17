// SPDX-License-Identifier: MPL-2.0
//! Bounded Unix pipe transport. Reader/control traffic never waits for a query.
use crate::Result;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc as async_mpsc, oneshot};
use tokio_util::sync::CancellationToken;
pub const FRAME: usize = 1024 * 1024;
type Reply = oneshot::Sender<Result<Value>>;
struct Inner {
    output: mpsc::SyncSender<Vec<u8>>,
    wake: UnixStream,
    pending: Mutex<HashMap<String, Reply>>,
    pub cancelled: Mutex<HashMap<String, CancellationToken>>,
    early: Mutex<Vec<String>>,
    serial: AtomicU64,
    stopped: AtomicBool,
    slots: Semaphore,
}
#[derive(Clone)]
pub struct Rpc(Arc<Inner>);
impl Rpc {
    pub fn start() -> Result<(Self, async_mpsc::Receiver<Value>)> {
        let (wake, read_wake) =
            UnixStream::pair().map_err(|_| "Cannot create transport wake channel")?;
        wake.set_nonblocking(true)
            .map_err(|_| "Cannot configure wake channel")?;
        let (out, rx) = mpsc::sync_channel::<Vec<u8>>(4);
        // Host requests (16) plus closed views (12), two activity leases, and
        // lifecycle headroom. Events do not consume the host request window.
        // FRAME bounds encoded queued input to 40 MiB. Responses and immediate
        // cancellation signalling stay on the reader, independent of this queue.
        let (incoming, events) = async_mpsc::channel(16 + 12 + 2 + 10);
        let rpc = Self(Arc::new(Inner {
            output: out,
            wake,
            pending: Mutex::new(HashMap::new()),
            cancelled: Mutex::new(HashMap::new()),
            early: Mutex::new(Vec::new()),
            serial: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            slots: Semaphore::new(12),
        }));
        let reader = rpc.clone();
        std::thread::spawn(move || {
            let _ = read_loop(&reader, incoming, read_wake);
            reader.stop();
        });
        let writer = rpc.clone();
        std::thread::spawn(move || {
            let _ = nonblocking(1);
            while !writer.closed() {
                let bytes = match rx.recv() {
                    Ok(bytes) => bytes,
                    Err(_) => break,
                };
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut at = 0;
                while at < bytes.len() {
                    if writer.closed() || wait(1, libc::POLLOUT, deadline).is_err() {
                        writer.stop();
                        return;
                    }
                    match std::io::stdout().write(&bytes[at..]) {
                        Ok(0) => {
                            writer.stop();
                            return;
                        }
                        Ok(n) => at += n,
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) => {}
                        Err(_) => {
                            writer.stop();
                            return;
                        }
                    }
                }
            }
        });
        Ok((rpc, events))
    }
    pub fn closed(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    pub fn stop(&self) {
        if self.0.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut wake = &self.0.wake;
        let _ = wake.write(&[1]);
        let _ = self.0.output.try_send(Vec::new());
        for (_, token) in self.0.cancelled.lock().unwrap().iter() {
            token.cancel();
        }
        self.0.pending.lock().unwrap().clear();
    }
    pub fn send(&self, message: Value) -> Result<()> {
        if self.closed() {
            return Err("Host disconnected".into());
        }
        let mut bytes = serde_json::to_vec(&message).map_err(|_| "Invalid message")?;
        bytes.push(b'\n');
        if bytes.len() > FRAME {
            return Err("Protocol frame limit exceeded".into());
        }
        self.0.output.try_send(bytes).map_err(|_| {
            self.stop();
            "Host output stalled".to_string()
        })
    }
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let _permit = self
            .0
            .slots
            .try_acquire()
            .map_err(|_| "Too many host requests")?;
        let id = format!("p:{}", self.0.serial.fetch_add(1, Ordering::Relaxed) + 1);
        let (tx, rx) = oneshot::channel();
        self.0.pending.lock().unwrap().insert(id.clone(), tx);
        if let Err(e) = self.send(json!({"type":"request","id":id,"method":method,"params":params}))
        {
            self.0.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(Duration::from_secs(8), rx).await {
            Ok(Ok(result)) => result,
            _ => {
                self.stop();
                Err(
                    "Host request failed; restart the plugin without replaying the operation"
                        .into(),
                )
            }
        }
    }
    pub fn reply(&self, id: &str, result: Result<Option<String>>) -> Result<()> {
        self.send(match result{Ok(job)=>json!({"type":"response","id":id,"result":{"job":job}}),Err(message)=>json!({"type":"response","id":id,"error":{"code":"invalid_argument","message":message}})})
    }
    pub fn track(&self, id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let mut map = self.0.cancelled.lock().unwrap();
        let mut early = self.0.early.lock().unwrap();
        if let Some(i) = early.iter().position(|x| x == id) {
            early.remove(i);
            token.cancel();
        }
        if self.closed() {
            token.cancel();
        }
        map.insert(id.to_owned(), token.clone());
        token
    }
    pub fn untrack(&self, id: &str) {
        self.0.cancelled.lock().unwrap().remove(id);
    }
}
fn nonblocking(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: fcntl only examines and changes flags on the supplied standard fd.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
fn wait(fd: RawFd, event: i16, deadline: Instant) -> Result<()> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Pipe deadline exceeded".into());
        }
        let mut p = libc::pollfd {
            fd,
            events: event,
            revents: 0,
        };
        // SAFETY: poll receives one initialized descriptor and its exact count.
        let n = unsafe { libc::poll(&mut p, 1, remaining.as_millis().min(100) as i32) };
        if n > 0 {
            return Ok(());
        }
        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err("Pipe poll failed".into());
        }
    }
}
fn read_loop(rpc: &Rpc, events: async_mpsc::Sender<Value>, wake: UnixStream) -> Result<()> {
    nonblocking(0).map_err(|_| "Cannot configure stdin")?;
    let mut frame = Vec::new();
    let mut block = [0; 8192];
    while !rpc.closed() {
        let mut fds = [
            libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: both entries refer to live descriptors for the duration of poll.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err("Input poll failed".into());
        }
        if fds[1].revents != 0 {
            return Ok(());
        }
        let n = match std::io::stdin().read(&mut block) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => return Err("Host input failed".into()),
        };
        for &b in &block[..n] {
            if frame.len() >= FRAME {
                return Err("Host frame too large".into());
            }
            frame.push(b);
            if b != b'\n' {
                continue;
            }
            let message: Value = serde_json::from_slice(&frame).map_err(|_| "Invalid host JSON")?;
            frame.clear();
            if message["type"] == "response" {
                if let Some(tx) = rpc
                    .0
                    .pending
                    .lock()
                    .unwrap()
                    .remove(message["id"].as_str().unwrap_or(""))
                {
                    let result = if message.get("error").is_some() {
                        Err(format!(
                            "Host refused operation: {}",
                            message["error"]["code"].as_str().unwrap_or("error")
                        ))
                    } else {
                        Ok(message["result"].clone())
                    };
                    let _ = tx.send(result);
                }
            } else if message["event"] == "job.cancel_requested"
                || message["event"] == "activity.cancel_requested"
            {
                let data = &message["data"];
                let id = data["job"]
                    .as_str()
                    .or(data["lease"].as_str())
                    .or(data["id"].as_str())
                    .unwrap_or("")
                    .to_owned();
                let tokens = rpc.0.cancelled.lock().unwrap();
                if let Some(t) = tokens.get(&id) {
                    t.cancel();
                } else {
                    let mut early = rpc.0.early.lock().unwrap();
                    if early.len() == 16 {
                        early.remove(0);
                    }
                    early.push(id);
                }
                drop(tokens);
                if message["event"] == "activity.cancel_requested" {
                    events.try_send(message).map_err(|_| "Control queue full")?;
                }
            } else if message["type"] != "event" || message["event"] == "view.closed" {
                events
                    .try_send(message)
                    .map_err(|_| "Host callback queue full")?;
            }
        }
    }
    Ok(())
}
