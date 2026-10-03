// SPDX-License-Identifier: MPL-2.0
use crate::{
    Result,
    profiles::Profile,
    results::{Cell, Column, Data},
};
use futures_util::{StreamExt, pin_mut};
use std::{
    io::{Cursor, Read},
    os::unix::fs::OpenOptionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_postgres::{Client, SimpleQueryMessage, config::SslMode};
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_util::sync::CancellationToken;

pub struct Postgres {
    client: Client,
    storage: crate::result_storage::Storage,
    tls: Option<MakeRustlsConnect>,
    task: tokio::task::JoinHandle<()>,
    retired: AtomicBool,
}
impl Drop for Postgres {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn error(e: tokio_postgres::Error) -> String {
    e.code()
        .map(|c| {
            format!(
                "PostgreSQL error {} (check SQL, permissions and transaction state)",
                c.code()
            )
        })
        .unwrap_or_else(|| {
            "PostgreSQL connection failed; an attempted write may have an unknown outcome".into()
        })
}
fn pem_reader(path: &str, role: &str) -> Result<Cursor<Vec<u8>>> {
    const MAX_PEM_BYTES: u64 = 4 * 1024 * 1024;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| format!("Cannot open {role}"))?;
    let metadata = file
        .metadata()
        .map_err(|_| format!("Cannot inspect {role}"))?;
    if !metadata.is_file() {
        return Err(format!("{role} must be a regular file"));
    }
    if metadata.len() > MAX_PEM_BYTES {
        return Err(format!("{role} exceeds the 4 MiB limit"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PEM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("Cannot read {role}"))?;
    if bytes.len() as u64 > MAX_PEM_BYTES {
        return Err(format!("{role} exceeds the 4 MiB limit"));
    }
    Ok(Cursor::new(bytes))
}
fn tls_config(ca: &str, cert: &str, key: &str) -> Result<MakeRustlsConnect> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if !ca.is_empty() {
        for c in rustls_pemfile::certs(&mut pem_reader(ca, "CA certificate")?) {
            roots
                .add(c.map_err(|_| "Invalid CA certificate")?)
                .map_err(|_| "Invalid CA certificate")?;
        }
    }
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| "TLS configuration failed")?
    .with_root_certificates(roots);
    let config = if cert.is_empty() {
        builder.with_no_client_auth()
    } else {
        let certificates = rustls_pemfile::certs(&mut pem_reader(cert, "client certificate")?)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "Invalid client certificate")?;
        let key = rustls_pemfile::private_key(&mut pem_reader(key, "client key")?)
            .map_err(|_| "Invalid client key")?
            .ok_or("Missing client key")?;
        builder
            .with_client_auth_cert(certificates, key)
            .map_err(|_| "Invalid client certificate/key pair")?
    };
    Ok(MakeRustlsConnect::new(config))
}
impl Postgres {
    pub async fn open(
        profile: &Profile,
        password: String,
        storage: crate::result_storage::Storage,
    ) -> Result<Self> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let Profile::Postgres {
            host,
            port,
            database,
            user,
            plaintext,
            ca,
            certificate,
            key,
            ..
        } = profile
        else {
            return Err("Expected PostgreSQL profile".into());
        };
        let tls = if *plaintext || host.starts_with('/') {
            None
        } else {
            let (ca, cert, key) = (ca.clone(), certificate.clone(), key.clone());
            Some(
                tokio::time::timeout_at(
                    deadline,
                    tokio::task::spawn_blocking(move || tls_config(&ca, &cert, &key)),
                )
                .await
                .map_err(|_| "Connection timed out")?
                .map_err(|_| "TLS worker failed")??,
            )
        };
        let mut config = tokio_postgres::Config::new();
        config
            .host(host)
            .port(*port)
            .dbname(database)
            .user(user)
            .password(password)
            .application_name("ru-dbviewer")
            .connect_timeout(Duration::from_secs(10))
            .ssl_mode(if *plaintext || host.starts_with('/') {
                SslMode::Disable
            } else {
                SslMode::Require
            });
        let (client, task) = if let Some(tls) = tls.clone() {
            let (client, connection) = tokio::time::timeout_at(deadline, config.connect(tls))
                .await
                .map_err(|_| "Connection timed out")?
                .map_err(error)?;
            (
                client,
                tokio::spawn(async move {
                    let _ = connection.await;
                }),
            )
        } else {
            let (client, connection) =
                tokio::time::timeout_at(deadline, config.connect(tokio_postgres::NoTls))
                    .await
                    .map_err(|_| "Connection timed out")?
                    .map_err(error)?;
            (
                client,
                tokio::spawn(async move {
                    let _ = connection.await;
                }),
            )
        };
        let db = Self {
            client,
            tls,
            task,
            storage,
            retired: AtomicBool::new(false),
        };
        tokio::time::timeout_at(deadline, db.client.batch_execute("SET standard_conforming_strings=on; SET idle_in_transaction_session_timeout='5min'"))
            .await.map_err(|_| "Connection timed out")?.map_err(error)?;
        Ok(db)
    }
    pub async fn execute(
        &self,
        sql: String,
        write: bool,
        cancel: CancellationToken,
        seconds: u64,
    ) -> Result<Data> {
        self.execute_bound(sql, Vec::new(), write, cancel, seconds)
            .await
    }
    pub async fn execute_bound(
        &self,
        sql: String,
        parameters: Vec<String>,
        write: bool,
        cancel: CancellationToken,
        seconds: u64,
    ) -> Result<Data> {
        if !self.usable() {
            return Err("PostgreSQL connection is retired; reconnect explicitly".into());
        }
        // Drop an interrupted stream before cleanup so its response receiver
        // cannot block the driver's receipt of the rollback acknowledgement.
        let (result, interrupted) = {
            let work = async {
                let mut capture =
                    self.storage
                        .result_with_guard(crate::result_storage::WorkGuard::new(
                            cancel.clone(),
                            std::time::Instant::now() + Duration::from_secs(seconds),
                        ));
                self.client
                    .batch_execute(if write { "BEGIN" } else { "BEGIN READ ONLY" })
                    .await
                    .map_err(error)?;
                self.client
                .batch_execute(&format!(
                    "SET LOCAL statement_timeout='{}ms'; SET LOCAL standard_conforming_strings=on",
                    seconds * 1000
                ))
                .await
                .map_err(error)?;
                // Server preparation independently enforces a single statement before the
                // text-result protocol executes it. This preserves every server type's text.
                let statement = self.client.prepare(&sql).await.map_err(error)?;
                let mut data = Data {
                    columns: statement
                        .columns()
                        .iter()
                        .map(|c| Column {
                            name: c.name().into(),
                            kind: c.type_().name().into(),
                        })
                        .collect(),
                    ..Data::default()
                };
                if !parameters.is_empty() {
                    // Browse projections cast returned columns to text; values remain bound parameters.
                    let params = parameters
                        .iter()
                        .map(|s| s as &(dyn tokio_postgres::types::ToSql + Sync))
                        .collect::<Vec<_>>();
                    let stream = self
                        .client
                        .query_raw(&statement, params)
                        .await
                        .map_err(error)?;
                    pin_mut!(stream);
                    while let Some(row) = stream.next().await {
                        let row = row.map_err(error)?;
                        if data.rows.len() >= crate::results::MAX_ROWS {
                            data.truncated = true;
                            return Ok(data);
                        }
                        let (next, cells, checkpoint) = tokio::task::spawn_blocking(move || {
                            let checkpoint = capture.checkpoint();
                            let cells = (0..row.len())
                                .map(|i| {
                                    capture.check()?;
                                    row.try_get::<_, Option<&str>>(i)
                                        .map(|text| Cell::capture(text, &mut capture))
                                        .map_err(error)
                                })
                                .collect::<Result<Vec<_>>>();
                            (capture, cells, checkpoint)
                        })
                        .await
                        .map_err(|_| "PostgreSQL capture worker stopped")?;
                        capture = next;
                        let cells = cells?;
                        if !data.push(cells) {
                            tokio::task::spawn_blocking(move || capture.discard_since(checkpoint))
                                .await
                                .map_err(|_| "PostgreSQL capture worker stopped")??;
                            return Ok(data);
                        }
                    }
                    capture.check()?;
                    return Ok(data);
                }
                let stream = self.client.simple_query_raw(&sql).await.map_err(error)?;
                pin_mut!(stream);
                while let Some(item) = stream.next().await {
                    match item.map_err(error)? {
                        SimpleQueryMessage::Row(row) => {
                            if data.rows.len() >= crate::results::MAX_ROWS {
                                data.truncated = true;
                                return Ok(data);
                            }
                            let (next, cells, checkpoint) =
                                tokio::task::spawn_blocking(move || {
                                    let checkpoint = capture.checkpoint();
                                    let cells = (0..row.len())
                                        .map(|i| {
                                            capture.check()?;
                                            let cell = Cell::capture(row.get(i), &mut capture);
                                            capture.check()?;
                                            Ok(cell)
                                        })
                                        .collect::<Result<Vec<_>>>();
                                    (capture, cells, checkpoint)
                                })
                                .await
                                .map_err(|_| "PostgreSQL capture worker stopped")?;
                            capture = next;
                            let cells = cells?;
                            if !data.push(cells) {
                                tokio::task::spawn_blocking(move || {
                                    capture.discard_since(checkpoint)
                                })
                                .await
                                .map_err(|_| "PostgreSQL capture worker stopped")??;
                                return Ok(data);
                            }
                        }
                        SimpleQueryMessage::CommandComplete(n) => data.affected = Some(n),
                        _ => {}
                    }
                }
                capture.check()?;
                Ok(data)
            };
            tokio::pin!(work);
            let mut interrupted = false;
            let result = tokio::select! {
             biased;
             _=cancel.cancelled()=>{interrupted=true;Err("Query cancelled".to_string())},
             _=tokio::time::sleep(Duration::from_secs(seconds))=>{interrupted=true;Err("Query timed out".to_string())},
             result=&mut work=>result,
            };
            (result, interrupted)
        };
        let capped = result.as_ref().is_ok_and(|d| d.truncated);
        if interrupted || capped {
            self.retired.store(true, Ordering::Release);
            let _ = tokio::time::timeout(Duration::from_millis(400), async {
                let token = self.client.cancel_token();
                if let Some(tls) = self.tls.clone() {
                    token.cancel_query(tls).await
                } else {
                    token.cancel_query(tokio_postgres::NoTls).await
                }
            })
            .await;
        }
        if interrupted || capped {
            // A cancel packet can arrive after its query finishes. Retire this
            // connection so it can never cancel a later user operation.
            let settled = self.settle(false).await;
            self.task.abort();
            if settled.is_err() && write {
                return Err("Transaction outcome unknown after cancellation".into());
            }
        } else if result.is_err() || !write {
            self.settle(false).await?;
        }
        if capped && write {
            return Err("Result limit reached; writable transaction rolled back".into());
        }
        result
    }
    pub fn usable(&self) -> bool {
        !self.retired.load(Ordering::Acquire)
            && !self.task.is_finished()
            && !self.client.is_closed()
    }
    pub async fn settle(&self, commit: bool) -> Result<()> {
        tokio::time::timeout(
            Duration::from_secs(1),
            self.client
                .batch_execute(if commit { "COMMIT" } else { "ROLLBACK" }),
        )
        .await
        .map_err(|_| "Transaction outcome unknown: connection did not settle")?
        .map_err(error)
    }
}
