// SPDX-License-Identifier: MPL-2.0
use crate::{
    Result,
    profiles::Profile,
    results::{Cell, Column, Data},
};
use futures_util::{StreamExt, pin_mut};
use std::{io::BufReader, sync::Arc, time::Duration};
use tokio_postgres::{Client, SimpleQueryMessage, config::SslMode};
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_util::sync::CancellationToken;

pub struct Postgres {
    client: Client,
    tls: Option<MakeRustlsConnect>,
    task: tokio::task::JoinHandle<()>,
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
fn tls_config(ca: &str, cert: &str, key: &str) -> Result<MakeRustlsConnect> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if !ca.is_empty() {
        let f = std::fs::File::open(ca).map_err(|_| "Cannot open CA certificate".to_string())?;
        for c in rustls_pemfile::certs(&mut BufReader::new(f)) {
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
        let f = std::fs::File::open(cert).map_err(|_| "Cannot open client certificate")?;
        let certificates = rustls_pemfile::certs(&mut BufReader::new(f))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "Invalid client certificate")?;
        let f = std::fs::File::open(key).map_err(|_| "Cannot open client key")?;
        let key = rustls_pemfile::private_key(&mut BufReader::new(f))
            .map_err(|_| "Invalid client key")?
            .ok_or("Missing client key")?;
        builder
            .with_client_auth_cert(certificates, key)
            .map_err(|_| "Invalid client certificate/key pair")?
    };
    Ok(MakeRustlsConnect::new(config))
}
impl Postgres {
    pub async fn open(profile: &Profile, password: String) -> Result<Self> {
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
                tokio::task::spawn_blocking(move || tls_config(&ca, &cert, &key))
                    .await
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
            let (client, connection) =
                tokio::time::timeout(Duration::from_secs(10), config.connect(tls))
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
            let (client, connection) = tokio::time::timeout(
                Duration::from_secs(10),
                config.connect(tokio_postgres::NoTls),
            )
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
        client.batch_execute("SET standard_conforming_strings=on; SET idle_in_transaction_session_timeout='5min'").await.map_err(error)?;
        Ok(Self { client, tls, task })
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
        let work = async {
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
                    let cells = (0..row.len())
                        .map(|i| {
                            row.try_get::<_, Option<String>>(i)
                                .map(Cell::new)
                                .map_err(error)
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if !data.push(cells) {
                        return Ok(data);
                    }
                }
                return Ok(data);
            }
            let stream = self.client.simple_query_raw(&sql).await.map_err(error)?;
            pin_mut!(stream);
            while let Some(item) = stream.next().await {
                match item.map_err(error)? {
                    SimpleQueryMessage::Row(row) => {
                        let cells = (0..row.len())
                            .map(|i| Cell::new(row.get(i).map(str::to_owned)))
                            .collect();
                        if !data.push(cells) {
                            return Ok(data);
                        }
                    }
                    SimpleQueryMessage::CommandComplete(n) => data.affected = Some(n),
                    _ => {}
                }
            }
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
        let capped = result.as_ref().is_ok_and(|d| d.truncated);
        if interrupted || capped {
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
        !self.task.is_finished() && !self.client.is_closed()
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
