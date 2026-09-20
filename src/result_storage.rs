// SPDX-License-Identifier: MPL-2.0
//! Captured SQL values. Anonymous files disappear even after an unclean exit.
use crate::Result;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

pub const MAX_FULL_VALUE: usize = 8 * 1024 * 1024;
pub const MAX_RESULT_CAPTURE: usize = 32 * 1024 * 1024;
pub const MAX_OWNER_CAPTURE: usize = 64 * 1024 * 1024;
pub const MAX_SPOOLS: usize = 32;

#[derive(Clone, Debug)]
pub struct WorkGuard {
    cancel: tokio_util::sync::CancellationToken,
    deadline: Instant,
}
impl WorkGuard {
    pub fn new(cancel: tokio_util::sync::CancellationToken, deadline: Instant) -> Self {
        Self { cancel, deadline }
    }
    pub fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err("Operation cancelled".into());
        }
        if Instant::now() >= self.deadline {
            return Err("Operation timed out".into());
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Usage {
    bytes: usize,
    spools: usize,
}
#[derive(Debug)]
struct Owner {
    root: PathBuf,
    usage: Mutex<Usage>,
}
#[derive(Clone, Debug)]
pub struct Storage(Arc<Owner>);
impl Storage {
    /// Each plugin process shares one quota owner. Tests may inject their own root.
    pub fn global() -> Self {
        static STORAGE: OnceLock<Storage> = OnceLock::new();
        STORAGE
            .get_or_init(|| Self::new(std::env::temp_dir()))
            .clone()
    }
    pub fn new(root: PathBuf) -> Self {
        Self(Arc::new(Owner {
            root,
            usage: Mutex::new(Usage::default()),
        }))
    }
    pub fn result(&self) -> Capture {
        Capture {
            storage: self.clone(),
            spool: None,
            guard: None,
        }
    }
    pub fn result_with_guard(&self, guard: WorkGuard) -> Capture {
        Capture {
            guard: Some(guard),
            ..self.result()
        }
    }
    pub fn usage(&self) -> (usize, usize) {
        let usage = self.0.usage.lock().unwrap_or_else(|e| e.into_inner());
        (usage.bytes, usage.spools)
    }
}
#[derive(Debug)]
struct FileState {
    file: Option<File>,
    bytes: usize,
    failed: bool,
}
#[derive(Debug)]
struct Spool {
    owner: Storage,
    state: Mutex<FileState>,
}
impl Drop for Spool {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(|e| e.into_inner());
        // Close the descriptor before admitting another reservation on any thread.
        drop(state.file.take());
        let mut usage = self.owner.0.usage.lock().unwrap_or_else(|e| e.into_inner());
        usage.bytes -= state.bytes;
        usage.spools -= 1;
    }
}
#[derive(Debug)]
pub struct Capture {
    storage: Storage,
    spool: Option<Arc<Spool>>,
    guard: Option<WorkGuard>,
}
impl Capture {
    pub fn check(&self) -> Result<()> {
        self.guard.as_ref().map_or(Ok(()), WorkGuard::check)
    }
    /// Call only on a blocking worker. A failed capture never substitutes a prefix.
    pub fn store(&mut self, text: &str) -> Result<CapturedValue> {
        self.check()?;
        if text.len() > MAX_FULL_VALUE {
            return Err("Full value exceeds the 8 MiB limit".into());
        }
        if self.spool.is_none() {
            let mut usage = self
                .storage
                .0
                .usage
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if usage.spools >= MAX_SPOOLS {
                return Err("Full-value storage has reached its file limit".into());
            }
            if usage.bytes + text.len() > MAX_OWNER_CAPTURE {
                return Err("Full-value storage has reached its 64 MiB limit".into());
            }
            usage.spools += 1;
            drop(usage);
            let file = match tempfile::tempfile_in(&self.storage.0.root) {
                Ok(file) => file,
                Err(_) => {
                    self.storage
                        .0
                        .usage
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .spools -= 1;
                    return Err("Full-value temporary storage is unavailable".into());
                }
            };
            self.spool = Some(Arc::new(Spool {
                owner: self.storage.clone(),
                state: Mutex::new(FileState {
                    file: Some(file),
                    bytes: 0,
                    failed: false,
                }),
            }));
        }
        let spool = self.spool.as_ref().unwrap();
        let mut state = spool.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.failed {
            return Err("Full-value temporary storage failed".into());
        }
        if state.bytes + text.len() > MAX_RESULT_CAPTURE {
            return Err("Result full-value storage has reached its 32 MiB limit".into());
        }
        let mut usage = self
            .storage
            .0
            .usage
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if usage.bytes + text.len() > MAX_OWNER_CAPTURE {
            return Err("Full-value storage has reached its 64 MiB limit".into());
        }
        usage.bytes += text.len();
        drop(usage);
        let offset = state.bytes;
        // Retain the reservation even after a partial failed write until this file closes.
        state.bytes += text.len();
        let write = (|| {
            self.check()?;
            state
                .file
                .as_mut()
                .expect("live spool file")
                .seek(SeekFrom::Start(offset as u64))
                .map_err(|_| "Full-value temporary storage failed")?;
            for chunk in text.as_bytes().chunks(64 * 1024) {
                self.check()?;
                state
                    .file
                    .as_mut()
                    .expect("live spool file")
                    .write_all(chunk)
                    .map_err(|_| "Full-value temporary storage failed")?;
            }
            self.check()
        })();
        if let Err(reason) = write {
            state.failed = true;
            return Err(reason);
        }
        Ok(CapturedValue {
            spool: spool.clone(),
            offset,
            len: text.len(),
        })
    }
}
#[derive(Clone, Debug)]
pub struct CapturedValue {
    spool: Arc<Spool>,
    offset: usize,
    len: usize,
}
impl CapturedValue {
    /// Immutable ranges remain valid even while a later field is appended.
    pub fn read(&self) -> Result<String> {
        self.read_with_guard(None)
    }
    pub fn read_checked(&self, guard: &WorkGuard) -> Result<String> {
        self.read_with_guard(Some(guard))
    }
    fn read_with_guard(&self, guard: Option<&WorkGuard>) -> Result<String> {
        let check = || guard.map_or(Ok(()), WorkGuard::check);
        check()?;
        let mut state = self.spool.state.lock().unwrap_or_else(|e| e.into_inner());
        check()?;
        let mut bytes = vec![0; self.len];
        state
            .file
            .as_mut()
            .expect("live spool file")
            .seek(SeekFrom::Start(self.offset as u64))
            .map_err(|_| "Full-value temporary storage could not be read")?;
        for chunk in bytes.chunks_mut(64 * 1024) {
            check()?;
            state
                .file
                .as_mut()
                .expect("live spool file")
                .read_exact(chunk)
                .map_err(|_| "Full-value temporary storage could not be read")?;
        }
        check()?;
        String::from_utf8(bytes).map_err(|_| "Full-value temporary storage is invalid".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anonymous_ranges_share_one_file_and_release_on_last_reference() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into());
        let mut capture = storage.result();
        let first = capture.store("first é").unwrap();
        let next = capture.store("second 🦀").unwrap();
        assert_eq!(first.read().unwrap(), "first é");
        assert_eq!(next.read().unwrap(), "second 🦀");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert_eq!(storage.usage(), ("first ésecond 🦀".len(), 1));
        let descendant = first.clone();
        drop(capture);
        drop(first);
        drop(next);
        assert_eq!(descendant.read().unwrap(), "first é");
        assert_eq!(storage.usage().1, 1);
        drop(descendant);
        assert_eq!(storage.usage(), (0, 0));
    }
    #[test]
    fn quotas_reserve_before_write_and_release_without_cross_owner_interference() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into());
        let value = "x".repeat(MAX_FULL_VALUE);
        let mut first = storage.result();
        assert!(first.store(&(value.clone() + "x")).is_err());
        assert_eq!(storage.usage(), (0, 0));
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(first.store(&value).unwrap());
        }
        assert!(first.store("x").unwrap_err().contains("32 MiB"));
        let mut second = storage.result();
        for _ in 0..4 {
            held.push(second.store(&value).unwrap());
        }
        let mut third = storage.result();
        assert!(third.store("x").unwrap_err().contains("64 MiB"));
        assert_eq!(storage.usage(), (MAX_OWNER_CAPTURE, 2));
        assert_eq!(Storage::new(dir.path().into()).usage(), (0, 0));
        drop(first);
        drop(second);
        drop(held);
        assert_eq!(storage.usage(), (0, 0));
        assert_eq!(third.store("retry").unwrap().read().unwrap(), "retry");
    }
    #[test]
    fn descriptor_limit_and_file_creation_failure_release_reservations() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into());
        let held = (0..MAX_SPOOLS)
            .map(|_| storage.result().store("x").unwrap())
            .collect::<Vec<_>>();
        assert!(
            storage
                .result()
                .store("x")
                .unwrap_err()
                .contains("file limit")
        );
        assert_eq!(storage.usage(), (MAX_SPOOLS, MAX_SPOOLS));
        drop(held);
        assert_eq!(storage.usage(), (0, 0));
        let unavailable = Storage::new(dir.path().join("absent"));
        assert!(unavailable.result().store("x").is_err());
        assert_eq!(unavailable.usage(), (0, 0));
    }
    #[test]
    fn cancellation_and_deadline_refuse_capture_and_later_load_without_leaks() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into());
        let token = tokio_util::sync::CancellationToken::new();
        let guard = WorkGuard::new(
            token.clone(),
            Instant::now() + std::time::Duration::from_secs(10),
        );
        let mut capture = storage.result_with_guard(guard.clone());
        let value = capture.store("complete").unwrap();
        token.cancel();
        assert!(capture.store("later").unwrap_err().contains("cancelled"));
        assert!(
            value
                .read_checked(&guard)
                .unwrap_err()
                .contains("cancelled")
        );
        assert_eq!(value.read().unwrap(), "complete");
        let expired = WorkGuard::new(tokio_util::sync::CancellationToken::new(), Instant::now());
        assert!(
            storage
                .result_with_guard(expired.clone())
                .store("x")
                .unwrap_err()
                .contains("timed out")
        );
        assert!(
            value
                .read_checked(&expired)
                .unwrap_err()
                .contains("timed out")
        );
        drop(capture);
        drop(value);
        assert_eq!(storage.usage(), (0, 0));
    }
    #[test]
    fn failed_write_does_not_publish_partial_ranges_or_leak_reservations() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into());
        let mut capture = storage.result();
        let first = capture.store("stable").unwrap();
        // A read-only descriptor gives a deterministic write error without disk exhaustion.
        let readonly_path = dir.path().join("readonly");
        std::fs::write(&readonly_path, b"stable").unwrap();
        capture.spool.as_ref().unwrap().state.lock().unwrap().file =
            Some(File::open(readonly_path).unwrap());
        assert!(capture.store("new").is_err());
        assert!(capture.store("retry").is_err());
        assert_eq!(first.read().unwrap(), "stable");
        assert_eq!(storage.usage(), (9, 1));
        drop(capture);
        drop(first);
        assert_eq!(storage.usage(), (0, 0));
    }
}
