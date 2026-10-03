// SPDX-License-Identifier: MPL-2.0
use ru_dbviewer::{db::Database, profiles::Profile};
use tokio_util::sync::CancellationToken;
fn token() -> CancellationToken {
    CancellationToken::new()
}
fn fixture() -> (tempfile::TempDir, Profile) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.sqlite");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT, amount INTEGER); INSERT INTO items VALUES(1,'first',9223372036854775807),(2,'second',NULL); CREATE VIEW names AS SELECT name FROM items;").unwrap();
    (
        dir,
        Profile::Sqlite {
            name: "fixture".into(),
            path: path.to_str().unwrap().into(),
        },
    )
}
fn postgres_profile(name: &str) -> Profile {
    Profile::Postgres {
        name: name.into(),
        host: "127.0.0.1".into(),
        port: std::env::var("DBVIEWER_TEST_PG_PORT")
            .expect("set isolated PostgreSQL port")
            .parse()
            .unwrap(),
        database: "dbviewer".into(),
        user: "dbviewer".into(),
        plaintext: true,
        password_env: String::new(),
        ca: String::new(),
        certificate: String::new(),
        key: String::new(),
    }
}

async fn postgres_fixture_sql(sql: &str) {
    let (client, connection) = tokio_postgres::Config::new()
        .host("127.0.0.1")
        .port(
            std::env::var("DBVIEWER_TEST_PG_PORT")
                .unwrap()
                .parse()
                .unwrap(),
        )
        .dbname("dbviewer")
        .user("dbviewer")
        .password("dbviewer-test-only")
        .connect_timeout(std::time::Duration::from_secs(5))
        .options("-c statement_timeout=5000")
        .connect(tokio_postgres::NoTls)
        .await
        .unwrap();
    let task = tokio::spawn(connection);
    client.batch_execute(sql).await.unwrap();
    drop(client);
    task.await.unwrap().unwrap();
}

async fn cte_affected_counts_contract(db: &Database) {
    let ddl = db
        .execute(
            "CREATE TABLE affected_contract(value INTEGER)".into(),
            true,
            token(),
            5,
        )
        .await
        .unwrap();
    if matches!(db, Database::Sqlite(_)) {
        assert_eq!(ddl.affected, None);
    }
    db.settle(true).await.unwrap();
    for (sql, expected) in [
        (
            "WITH values_to_add(v) AS (SELECT 1 UNION ALL SELECT 2) INSERT INTO affected_contract SELECT v FROM values_to_add",
            2,
        ),
        (
            "WITH chosen(v) AS (SELECT 1) UPDATE affected_contract SET value=value+10 WHERE value IN (SELECT v FROM chosen)",
            1,
        ),
        (
            "WITH chosen(v) AS (SELECT 999) UPDATE affected_contract SET value=0 WHERE value IN (SELECT v FROM chosen)",
            0,
        ),
        (
            "WITH chosen(v) AS (SELECT 2) DELETE FROM affected_contract WHERE value IN (SELECT v FROM chosen)",
            1,
        ),
    ] {
        let data = db.execute(sql.into(), true, token(), 5).await.unwrap();
        assert_eq!(data.affected, Some(expected), "{sql}");
        db.settle(true).await.unwrap();
    }
    let data = db
        .execute(
            "SELECT value FROM affected_contract".into(),
            false,
            token(),
            5,
        )
        .await
        .unwrap();
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0][0].text.as_deref(), Some("11"));
    if matches!(db, Database::Sqlite(_)) {
        assert_eq!(data.affected, None);
    }
    let ddl = db
        .execute("DROP TABLE affected_contract".into(), true, token(), 5)
        .await
        .unwrap();
    if matches!(db, Database::Sqlite(_)) {
        assert_eq!(ddl.affected, None);
    }
    db.settle(true).await.unwrap();
}

#[tokio::test]
async fn sqlite_cte_writes_report_affected_counts() {
    let (_dir, profile) = fixture();
    let db = Database::open(&profile, true, String::new()).await.unwrap();
    cte_affected_counts_contract(&db).await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_cte_writes_report_affected_counts() {
    let db = Database::open(
        &postgres_profile("cte-counts"),
        true,
        "dbviewer-test-only".into(),
    )
    .await
    .unwrap();
    cte_affected_counts_contract(&db).await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_primary_key_include_is_not_an_order_key() {
    let db = Database::open(
        &postgres_profile("include-key"),
        true,
        "dbviewer-test-only".into(),
    )
    .await
    .unwrap();
    postgres_fixture_sql(
        "CREATE TABLE browse_include(id INTEGER, payload JSON, PRIMARY KEY(id) INCLUDE(payload))",
    )
    .await;
    db.execute(
        "INSERT INTO browse_include VALUES(2,'{}'),(1,'[]')".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    let table = ru_dbviewer::db::Table {
        schema: "public".into(),
        name: "browse_include".into(),
        kind: "table".into(),
    };
    assert_eq!(db.browse_keys(&table, token()).await.unwrap(), ["id"]);
    let data = db.browse(&table, 0, token()).await.unwrap();
    assert_eq!(
        data.rows
            .iter()
            .map(|row| row[0].text.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
    db.execute("DROP TABLE browse_include".into(), true, token(), 5)
        .await
        .unwrap();
    db.settle(true).await.unwrap();
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_nonscalar_types_use_text_filters() {
    use ru_dbviewer::browse::{Browse, Filter, Operator, operators};
    postgres_fixture_sql("CREATE TABLE browse_textual_types(id INTEGER PRIMARY KEY, position POINT, duration INTERVAL, sequence INTEGER[], span INT4RANGE); INSERT INTO browse_textual_types VALUES(1,'(1,2)','2 days','{1,2}','[1,3)'),(2,'(3,4)','4 days','{3,4}','[3,5)')").await;
    let db = Database::open(
        &postgres_profile("text-types"),
        false,
        "dbviewer-test-only".into(),
    )
    .await
    .unwrap();
    let table = ru_dbviewer::db::Table {
        schema: "public".into(),
        name: "browse_textual_types".into(),
        kind: "table".into(),
    };
    let original = db.browse(&table, 0, token()).await.unwrap();
    let mut browse = Browse {
        columns: original.columns,
        ..Browse::default()
    };
    for column in 1..browse.columns.len() {
        assert!(
            operators(&browse.columns[column].kind)
                .contains(&Operator::Contains.name().to_string())
        );
        for op in [Operator::Equal, Operator::Contains] {
            browse.filters = vec![Filter {
                column,
                op,
                value: original.rows[0][column].text.clone().unwrap(),
                enabled: true,
            }];
            let filtered = db.browse_with(&table, 0, &browse, token()).await.unwrap();
            assert_eq!(filtered.rows.len(), 1);
            assert_eq!(filtered.rows[0][0].text.as_deref(), Some("1"));
        }
    }
    postgres_fixture_sql("DROP TABLE browse_textual_types").await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_retirement_is_immediate_after_interruption_or_cap() {
    for reason in ["read cap", "write cap", "cancel", "timeout"] {
        let db = Database::open(
            &postgres_profile("retirement"),
            true,
            "dbviewer-test-only".into(),
        )
        .await
        .unwrap();
        let cancel = token();
        if reason == "cancel" {
            cancel.cancel();
        }
        let seconds = if reason == "timeout" { 1 } else { 5 };
        let sql = if reason == "timeout" {
            "SELECT pg_sleep(5)"
        } else {
            "SELECT generate_series(1,1001)"
        };
        let result = db
            .execute(sql.into(), reason == "write cap", cancel, seconds)
            .await;
        if reason == "read cap" {
            assert!(result.unwrap().truncated);
        } else {
            assert!(result.is_err());
        }
        assert!(
            !db.usable(),
            "{reason} must retire before returning to the caller"
        );
        assert!(
            db.execute("SELECT 42".into(), false, token(), 5)
                .await
                .is_err()
        );
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_open_deadline_includes_session_setup() {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let server_port = std::env::var("DBVIEWER_TEST_PG_PORT")
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let saw_setup = Arc::new(AtomicBool::new(false));
    let observed = saw_setup.clone();
    let release = token();
    let released = release.clone();
    let mut proxy = tokio::spawn(async move {
        let (front, _) = listener.accept().await.unwrap();
        let back = TcpStream::connect(("127.0.0.1", server_port))
            .await
            .unwrap();
        let (mut front_read, mut front_write) = front.into_split();
        let (mut back_read, mut back_write) = back.into_split();
        let upstream = tokio::io::copy(&mut front_read, &mut back_write);
        let downstream = async {
            loop {
                let mut header = [0; 5];
                if back_read.read_exact(&mut header).await.is_err() {
                    break;
                }
                let length = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                assert!((4..=1024 * 1024).contains(&length));
                let mut body = vec![0; length - 4];
                if back_read.read_exact(&mut body).await.is_err() {
                    break;
                }
                if header[0] == b'C' && body == b"SET\0" {
                    observed.store(true, Ordering::Release);
                    released.cancelled().await;
                    break;
                }
                if front_write.write_all(&header).await.is_err()
                    || front_write.write_all(&body).await.is_err()
                {
                    break;
                }
            }
        };
        tokio::select! { _ = upstream => {}, _ = downstream => {} }
    });
    let mut profile = postgres_profile("setup-timeout");
    let Profile::Postgres { port, .. } = &mut profile else {
        unreachable!()
    };
    *port = proxy_port;
    let result = tokio::time::timeout(
        Duration::from_secs(12),
        Database::open(&profile, false, "dbviewer-test-only".into()),
    )
    .await;
    let closed = tokio::time::timeout(Duration::from_secs(1), &mut proxy)
        .await
        .is_ok();
    release.cancel();
    if !closed {
        tokio::time::timeout(Duration::from_secs(1), proxy)
            .await
            .unwrap()
            .unwrap();
    }
    assert!(
        saw_setup.load(Ordering::Acquire),
        "fixture must reach session initialization"
    );
    let error = result
        .expect("connection's own deadline must cover session initialization")
        .err()
        .expect("stalled setup cannot connect successfully");
    assert!(error.contains("timed out"), "{error}");
    assert!(closed, "failed opening must close its driver connection");
}
#[tokio::test]
#[ignore = "requires local TCP fixtures; run with the PostgreSQL fixture suite"]
async fn postgres_custom_tls_files_reject_fifo_and_oversized_inputs() {
    use std::{
        ffi::CString,
        fs::OpenOptions,
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        time::Duration,
    };
    let directory = tempfile::tempdir().unwrap();
    let fifo = directory.path().join("ca.fifo");
    let path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // The path is a valid, NUL-terminated name in this fixture's private directory.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let oversized = directory.path().join("oversized.pem");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(4 * 1024 * 1024 + 1)
        .unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    for (path, expected) in [(&fifo, "regular file"), (&oversized, "4 MiB")] {
        let (release, released) = std::sync::mpsc::channel();
        let fifo = fifo.clone();
        let cleanup = std::thread::spawn(move || {
            released.recv_timeout(Duration::from_secs(5)).unwrap();
            // Release the original blocking-open implementation after its timeout.
            // RDWR also opens immediately when the fixed implementation has no reader.
            let _writer = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(fifo)
                .unwrap();
        });
        let profile = Profile::Postgres {
            name: "tls-files".into(),
            host: "127.0.0.1".into(),
            port,
            database: "fixture".into(),
            user: "fixture".into(),
            plaintext: false,
            password_env: String::new(),
            ca: path.to_str().unwrap().into(),
            certificate: String::new(),
            key: String::new(),
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            Database::open(&profile, false, String::new()),
        )
        .await;
        release.send(()).unwrap();
        cleanup.join().unwrap();
        let error = result
            .expect("invalid TLS files must be rejected without blocking")
            .err()
            .expect("invalid TLS file must fail");
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains(path.to_str().unwrap()));
    }
}
async fn rejected_row_capture_contract(
    db: &Database,
    storage: &ru_dbviewer::result_storage::Storage,
    pg: bool,
) {
    let sql = if pg {
        "SELECT CASE WHEN n=1 THEN repeat('x',70000) WHEN n=1001 THEN repeat('y',1048576) ELSE 'small' END FROM generate_series(1,1001) AS series(n)"
    } else {
        "WITH RECURSIVE series(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM series WHERE n<1001) SELECT CASE WHEN n=1 THEN printf('%.*c',70000,'x') WHEN n=1001 THEN printf('%.*c',1048576,'y') ELSE 'small' END FROM series"
    };
    let data = db.execute(sql.into(), false, token(), 5).await.unwrap();
    assert!(data.truncated);
    assert_eq!(data.rows.len(), 1000);
    assert_eq!(
        storage.usage(),
        (70000, 1),
        "discarded row must not consume retained spool quota"
    );
    assert_eq!(
        data.rows[0][0].load_full().await.unwrap().unwrap(),
        "x".repeat(70000)
    );
    drop(data);
    assert_eq!(storage.usage(), (0, 0));
}
#[tokio::test]
async fn sqlite_row_limit_does_not_capture_discarded_values() {
    let (directory, profile) = fixture();
    let storage = ru_dbviewer::result_storage::Storage::new(directory.path().into());
    let db = Database::open_with_storage(&profile, false, String::new(), storage.clone())
        .await
        .unwrap();
    rejected_row_capture_contract(&db, &storage, false).await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_row_limit_does_not_capture_discarded_values() {
    let directory = tempfile::tempdir().unwrap();
    let storage = ru_dbviewer::result_storage::Storage::new(directory.path().into());
    let db = Database::open_with_storage(
        &postgres_profile("row-cap"),
        false,
        "dbviewer-test-only".into(),
        storage.clone(),
    )
    .await
    .unwrap();
    rejected_row_capture_contract(&db, &storage, true).await;
}
#[tokio::test]
async fn sqlite_declared_numeric_types_keep_numeric_filtering() {
    use ru_dbviewer::browse::{Browse, Filter, Operator};
    let (_dir, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    setup.execute_batch("CREATE TABLE numeric_types(a INTEGER,b DOUBLE PRECISION,c DECIMAL(12,3)); INSERT INTO numeric_types VALUES(2,2,2),(10,10,10)").unwrap();
    let db = Database::open(&profile, false, String::new())
        .await
        .unwrap();
    let table = ru_dbviewer::db::Table {
        schema: "main".into(),
        name: "numeric_types".into(),
        kind: "table".into(),
    };
    let original = db.browse(&table, 0, token()).await.unwrap();
    let mut browse = Browse {
        columns: original.columns,
        ..Browse::default()
    };
    for column in 0..browse.columns.len() {
        browse.filters = vec![Filter {
            column,
            op: Operator::Greater,
            value: "3".into(),
            enabled: true,
        }];
        let filtered = db.browse_with(&table, 0, &browse, token()).await.unwrap();
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0][0].text.as_deref(), Some("10"));
    }
}
#[tokio::test]
async fn sqlite_catalog_browse_schema_and_types() {
    let (_dir, p) = fixture();
    let db = Database::open(&p, false, String::new()).await.unwrap();
    let tables = db.catalog(false, token()).await.unwrap();
    assert_eq!(tables.len(), 2);
    let data = db.browse(&tables[0], 0, token()).await.unwrap();
    assert_eq!(data.rows[0][2].text.as_deref(), Some("9223372036854775807"));
    assert!(data.rows[1][2].text.is_none());
    let schema = db.schema(&tables[0], token()).await.unwrap();
    assert!(schema.rows.len() >= 3);
}
#[tokio::test]
async fn sqlite_schema_includes_generated_columns() {
    let (_dir, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    setup.execute_batch("CREATE TABLE generated_columns(base INTEGER, virtual_value INTEGER GENERATED ALWAYS AS (base+1) VIRTUAL, stored_value TEXT GENERATED ALWAYS AS ('value:'||base) STORED); INSERT INTO generated_columns(base) VALUES(7)").unwrap();
    drop(setup);
    let db = Database::open(&profile, false, String::new())
        .await
        .unwrap();
    let table = ru_dbviewer::db::Table {
        schema: "main".into(),
        name: "generated_columns".into(),
        kind: "table".into(),
    };
    let schema = db.schema(&table, token()).await.unwrap();
    let columns = schema
        .rows
        .iter()
        .filter(|row| row[0].text.as_deref() == Some("column"))
        .map(|row| {
            (
                row[1].text.as_deref().unwrap(),
                row[2].text.as_deref().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        columns,
        [
            ("base", "INTEGER"),
            ("virtual_value", "INTEGER"),
            ("stored_value", "TEXT")
        ]
    );
    let data = db.browse(&table, 0, token()).await.unwrap();
    assert_eq!(
        data.rows[0]
            .iter()
            .map(|cell| cell.text.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["7", "8", "value:7"]
    );
}
#[tokio::test]
async fn sqlite_schema_preserves_implicit_foreign_key_targets() {
    let (_directory, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    setup.execute_batch("CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(implicit_target INTEGER REFERENCES parent, explicit_target INTEGER REFERENCES parent(id))").unwrap();
    let db = Database::open(&profile, false, String::new())
        .await
        .unwrap();
    let table = ru_dbviewer::db::Table {
        schema: "main".into(),
        name: "child".into(),
        kind: "table".into(),
    };
    let schema = db.schema(&table, token()).await.unwrap();
    for (column, expected) in [
        ("implicit_target", "parent"),
        ("explicit_target", "parent(id)"),
    ] {
        let row = schema
            .rows
            .iter()
            .find(|row| {
                row[0].text.as_deref() == Some("foreign key")
                    && row[1].text.as_deref() == Some(column)
            })
            .unwrap();
        assert_eq!(row[2].text.as_deref(), Some(expected));
    }
}
#[tokio::test]
async fn sqlite_metadata_uses_the_captured_schema_despite_temp_shadowing() {
    let (_directory, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    setup
        .execute_batch("CREATE INDEX main_items_name ON items(name)")
        .unwrap();
    drop(setup);
    let db = Database::open(&profile, true, String::new()).await.unwrap();
    for sql in [
        "CREATE TEMP TABLE items(temp_id INTEGER PRIMARY KEY,temp_value TEXT REFERENCES temp_parent(id))",
        "CREATE INDEX temp.temp_items_value ON items(temp_value)",
    ] {
        db.execute(sql.into(), true, token(), 5).await.unwrap();
        db.settle(true).await.unwrap();
    }
    let main = db
        .catalog(false, token())
        .await
        .unwrap()
        .into_iter()
        .find(|table| table.name == "items")
        .unwrap();
    assert_eq!(main.schema, "main");
    let temporary = ru_dbviewer::db::Table {
        schema: "temp".into(),
        ..main.clone()
    };
    for (table, key, expected_columns, expected_index) in [
        (&main, "id", vec!["id", "name", "amount"], "main_items_name"),
        (
            &temporary,
            "temp_id",
            vec!["temp_id", "temp_value"],
            "temp_items_value",
        ),
    ] {
        assert_eq!(db.browse_keys(table, token()).await.unwrap(), [key]);
        let schema = db.schema(table, token()).await.unwrap();
        assert_eq!(
            schema
                .rows
                .iter()
                .filter(|row| row[0].text.as_deref() == Some("column"))
                .map(|row| row[1].text.as_deref().unwrap())
                .collect::<Vec<_>>(),
            expected_columns
        );
        let indexes = schema
            .rows
            .iter()
            .filter(|row| row[0].text.as_deref() == Some("index"))
            .map(|row| row[1].text.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(indexes, [expected_index]);
        let foreign_keys = schema
            .rows
            .iter()
            .filter(|row| row[0].text.as_deref() == Some("foreign key"))
            .collect::<Vec<_>>();
        if table.schema == "main" {
            assert!(foreign_keys.is_empty());
        } else {
            assert_eq!(foreign_keys[0][2].text.as_deref(), Some("temp_parent(id)"));
        }
    }
    assert_eq!(db.browse(&main, 0, token()).await.unwrap().rows.len(), 2);
    assert!(
        db.browse(&temporary, 0, token())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
}
#[tokio::test]
async fn sqlite_catalog_preserves_user_names_resembling_system_prefix() {
    let (_dir, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    setup
        .execute_batch(
            "CREATE TABLE sqliteXaudit(id INTEGER); CREATE TABLE sequence_owner(id INTEGER PRIMARY KEY AUTOINCREMENT)",
        )
        .unwrap();
    drop(setup);
    let db = Database::open(&profile, false, String::new())
        .await
        .unwrap();
    let tables = db.catalog(false, token()).await.unwrap();
    assert!(tables.iter().any(|t| t.name == "sqliteXaudit"));
    assert!(!tables.iter().any(|t| t.name == "sqlite_sequence"));
    let tables = db.catalog(true, token()).await.unwrap();
    assert!(tables.iter().any(|t| t.name == "sqliteXaudit"));
    assert!(tables.iter().any(|t| t.name == "sqlite_sequence"));
}
#[tokio::test]
async fn sqlite_clipped_metadata_is_not_reused_as_an_identifier() {
    use ru_dbviewer::{db::Table, query::ident, results::MAX_VALUE};
    let (_dir, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    let long_name = "x".repeat(MAX_VALUE + 1);
    setup
        .execute_batch(&format!("CREATE TABLE {}(id INTEGER)", ident(&long_name)))
        .unwrap();
    let db = Database::open(&profile, false, String::new())
        .await
        .unwrap();
    assert!(db.catalog(false, token()).await.is_err());
    setup
        .execute_batch(&format!(
            "DROP TABLE {}; CREATE TABLE long_key({} INTEGER PRIMARY KEY)",
            ident(&long_name),
            ident(&long_name)
        ))
        .unwrap();
    let table = Table {
        schema: "main".into(),
        name: "long_key".into(),
        kind: "table".into(),
    };
    assert!(db.browse_keys(&table, token()).await.is_err());
    assert!(db.browse(&table, 0, token()).await.is_err());
    let keys = (0..1001).map(|i| format!("k{i}")).collect::<Vec<_>>();
    let columns = keys
        .iter()
        .map(|key| format!("{key} INTEGER"))
        .collect::<Vec<_>>()
        .join(",");
    setup
        .execute_batch(&format!(
            "CREATE TABLE wide_key({columns}, PRIMARY KEY({}))",
            keys.join(",")
        ))
        .unwrap();
    let table = Table {
        name: "wide_key".into(),
        ..table
    };
    assert!(db.browse_keys(&table, token()).await.is_err());
}
#[tokio::test]
async fn sqlite_stale_browse_columns_fail_instead_of_becoming_literals() {
    use ru_dbviewer::browse::{Browse, Filter, Operator};
    let (_dir, profile) = fixture();
    let Profile::Sqlite { path, .. } = &profile else {
        unreachable!()
    };
    let setup = rusqlite::Connection::open(path).unwrap();
    setup.execute_batch("CREATE TABLE stale_columns(id INTEGER PRIMARY KEY, \"odd\"\"field\" TEXT); INSERT INTO stale_columns VALUES(1,'original')").unwrap();
    let db = Database::open(&profile, false, String::new())
        .await
        .unwrap();
    let table = ru_dbviewer::db::Table {
        schema: "main".into(),
        name: "stale_columns".into(),
        kind: "table".into(),
    };
    let original = db.browse(&table, 0, token()).await.unwrap();
    let mut browse = Browse {
        columns: original.columns,
        ..Browse::default()
    };
    assert_eq!(
        db.browse_with(&table, 0, &browse, token())
            .await
            .unwrap()
            .rows[0][1]
            .text
            .as_deref(),
        Some("original")
    );
    setup
        .execute_batch("ALTER TABLE stale_columns DROP COLUMN \"odd\"\"field\"")
        .unwrap();
    assert!(db.browse_with(&table, 0, &browse, token()).await.is_err());
    browse.selected = vec![0];
    browse.filters.push(Filter {
        column: 1,
        op: Operator::Equal,
        value: "odd\"field".into(),
        enabled: true,
    });
    for literals in [false, true] {
        let (sql, parameters) = browse
            .compile(&table, 0, &["id".into()], false, literals)
            .unwrap();
        let prepared = setup.prepare(&sql);
        assert!(
            prepared.is_err(),
            "A stale filter must fail even when its field is not projected: {parameters:?}"
        );
        if literals {
            assert!(db.execute(sql, false, token(), 5).await.is_err());
        }
    }
}
#[tokio::test]
async fn sqlite_read_only_rejects_writes_and_escape() {
    let (_d, p) = fixture();
    let db = Database::open(&p, false, String::new()).await.unwrap();
    for sql in [
        "DELETE FROM items",
        "PRAGMA query_only=OFF",
        "ATTACH DATABASE ':memory:' AS other",
        "SELECT 1; DELETE FROM items",
        "COMMIT",
    ] {
        assert!(
            db.execute(sql.into(), false, token(), 1).await.is_err(),
            "{sql}"
        );
    }
    let data = db
        .execute("SELECT COUNT(*) FROM items".into(), false, token(), 1)
        .await
        .unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("2"));
}
#[tokio::test]
async fn sqlite_commit_rollback_and_cancel() {
    let (_d, p) = fixture();
    let db = Database::open(&p, true, String::new()).await.unwrap();
    db.execute(
        "INSERT INTO items VALUES(3,'third',1)".into(),
        true,
        token(),
        1,
    )
    .await
    .unwrap();
    db.settle(false).await.unwrap();
    db.execute(
        "INSERT INTO items VALUES(4,'fourth',1)".into(),
        true,
        token(),
        1,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    let cancel = token();
    let signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        signal.cancel();
    });
    assert!(
        db.execute(
            "WITH RECURSIVE x(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM x) SELECT sum(n) FROM x"
                .into(),
            false,
            cancel,
            10
        )
        .await
        .is_err()
    );
    let data = db
        .execute("SELECT COUNT(*) FROM items".into(), false, token(), 1)
        .await
        .unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("3"));
}
#[tokio::test]
async fn sqlite_missing_file_is_not_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing");
    let p = Profile::Sqlite {
        name: "missing".into(),
        path: path.to_str().unwrap().into(),
    };
    assert!(Database::open(&p, false, String::new()).await.is_err());
    assert!(!path.exists());
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_types_transactions_and_cancellation() {
    let port = std::env::var("DBVIEWER_TEST_PG_PORT")
        .expect("set isolated PostgreSQL port")
        .parse()
        .unwrap();
    let p = Profile::Postgres {
        name: "integration".into(),
        host: "127.0.0.1".into(),
        port,
        database: "dbviewer".into(),
        user: "dbviewer".into(),
        plaintext: true,
        password_env: String::new(),
        ca: String::new(),
        certificate: String::new(),
        key: String::new(),
    };
    let capture_dir = tempfile::tempdir().unwrap();
    let db = Database::open_with_storage(
        &p,
        true,
        "dbviewer-test-only".into(),
        ru_dbviewer::result_storage::Storage::new(capture_dir.path().into()),
    )
    .await
    .unwrap();
    db.execute(
        "CREATE TABLE IF NOT EXISTS dbviewer_test(id bigint primary key, name text)".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    db.execute("DELETE FROM dbviewer_test".into(), true, token(), 5)
        .await
        .unwrap();
    db.settle(true).await.unwrap();
    let data=db.execute("SELECT 9223372036854775807::bigint, 12345678901234567890.123456789::numeric, ARRAY[1,2], '{\"a\":1}'::jsonb, NULL::text, '2000-01-01'::date, '00000000-0000-0000-0000-000000000000'::uuid".into(),false,token(),5).await.unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("9223372036854775807"));
    assert_eq!(
        data.rows[0][1].text.as_deref(),
        Some("12345678901234567890.123456789")
    );
    assert_eq!(data.rows[0][2].text.as_deref(), Some("{1,2}"));
    assert!(data.rows[0][4].text.is_none());
    db.execute(
        "INSERT INTO dbviewer_test VALUES(1,'rolled back')".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(false).await.unwrap();
    db.execute(
        "INSERT INTO dbviewer_test VALUES(2,'committed')".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    assert!(
        db.execute("DELETE FROM dbviewer_test".into(), false, token(), 5)
            .await
            .is_err()
    );
    for writable in [false, true] {
        let data = db
            .execute("SELECT repeat('x', 70000)".into(), writable, token(), 5)
            .await
            .unwrap();
        assert!(data.cells_truncated);
        assert_eq!(
            data.rows[0][0].load_full().await.unwrap().unwrap(),
            "x".repeat(70000)
        );
        assert!(!data.truncated);
        assert!(db.usable());
        if writable {
            db.settle(true).await.unwrap();
        }
    }
    let captured = db.execute("UPDATE dbviewer_test SET name=repeat('é', 40000) || 'sentinel' WHERE id=2 RETURNING name".into(), true, token(), 5).await.unwrap();
    db.settle(false).await.unwrap();
    assert_eq!(
        captured.rows[0][0].load_full().await.unwrap().unwrap(),
        "é".repeat(40000) + "sentinel"
    );
    assert_eq!(
        captured.rows[0][0].load_full().await.unwrap().unwrap(),
        "é".repeat(40000) + "sentinel"
    );
    let unchanged = db
        .execute(
            "SELECT name FROM dbviewer_test WHERE id=2".into(),
            false,
            token(),
            5,
        )
        .await
        .unwrap();
    assert_eq!(unchanged.rows[0][0].text.as_deref(), Some("committed"));
    let tables = db.catalog(false, token()).await.unwrap();

    let unusual = ru_dbviewer::db::Table {
        schema: "public".into(),
        name: "q'\\\"name".into(),
        kind: "table".into(),
    };
    let quoted = ru_dbviewer::query::ident(&unusual.name);
    db.execute(
        format!("CREATE TABLE {quoted}(id integer PRIMARY KEY)"),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    db.execute(
        "SELECT set_config('standard_conforming_strings','off',false)".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    assert!(
        db.browse(&unusual, 0, token())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(!db.schema(&unusual, token()).await.unwrap().rows.is_empty());
    db.execute(format!("DROP TABLE {quoted}"), true, token(), 5)
        .await
        .unwrap();
    db.settle(true).await.unwrap();
    let table = tables.iter().find(|t| t.name == "dbviewer_test").unwrap();
    assert_eq!(db.browse(table, 0, token()).await.unwrap().rows.len(), 1);
    assert!(!db.schema(table, token()).await.unwrap().rows.is_empty());
    let cancel = token();
    let signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        signal.cancel();
    });
    assert!(
        db.execute("SELECT pg_sleep(10)".into(), false, cancel, 15)
            .await
            .is_err()
    );
    let db = Database::open(&p, true, "dbviewer-test-only".into())
        .await
        .unwrap();
    assert!(
        db.execute("SELECT 42".into(), false, token(), 5)
            .await
            .is_ok()
    );
    db.execute("DROP TABLE dbviewer_test".into(), true, token(), 5)
        .await
        .unwrap();
    db.settle(true).await.unwrap();
}

#[tokio::test]
#[ignore = "requires isolated TLS PostgreSQL: DBVIEWER_TEST_PG_PORT and DBVIEWER_TEST_CA"]
async fn postgres_tls_verifies_ca_and_hostname() {
    let port = std::env::var("DBVIEWER_TEST_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let ca = std::env::var("DBVIEWER_TEST_CA").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let ca_link = directory.path().join("ca-link.pem");
    std::os::unix::fs::symlink(&ca, &ca_link).unwrap();
    let make = |host: &str, ca: String| Profile::Postgres {
        name: "tls".into(),
        host: host.into(),
        port,
        database: "dbviewer".into(),
        user: "dbviewer".into(),
        plaintext: false,
        password_env: String::new(),
        ca,
        certificate: String::new(),
        key: String::new(),
    };
    let db = Database::open(
        &make("localhost", ca_link.to_str().unwrap().into()),
        false,
        "dbviewer-test-only".into(),
    )
    .await
    .unwrap();
    assert!(
        db.execute("SELECT 1".into(), false, token(), 5)
            .await
            .is_ok()
    );
    assert!(
        Database::open(&make("127.0.0.1", ca), false, "dbviewer-test-only".into())
            .await
            .is_err()
    );
    assert!(
        Database::open(
            &make("localhost", String::new()),
            false,
            "dbviewer-test-only".into()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
#[ignore = "requires scripts/postgres_tests.py temporary cluster"]
async fn postgres_client_certificate_and_unix_socket() {
    let port = std::env::var("DBVIEWER_TEST_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let certificate = std::env::var("DBVIEWER_TEST_CLIENT_CERT").unwrap();
    let key = std::env::var("DBVIEWER_TEST_CLIENT_KEY").unwrap();
    let p = Profile::Postgres {
        name: "client-auth".into(),
        host: "localhost".into(),
        port,
        database: "dbviewer".into(),
        user: "dbviewer_cert".into(),
        plaintext: false,
        password_env: String::new(),
        ca: std::env::var("DBVIEWER_TEST_CA").unwrap(),
        certificate,
        key,
    };
    let db = Database::open(&p, false, String::new()).await.unwrap();
    let data = db
        .execute("SELECT current_user".into(), false, token(), 5)
        .await
        .unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("dbviewer_cert"));
    let p = Profile::Postgres {
        name: "socket".into(),
        host: std::env::var("DBVIEWER_TEST_SOCKET").unwrap(),
        port,
        database: "dbviewer".into(),
        user: "dbviewer".into(),
        plaintext: false,
        password_env: String::new(),
        ca: String::new(),
        certificate: String::new(),
        key: String::new(),
    };
    let db = Database::open(&p, false, String::new()).await.unwrap();
    assert!(
        db.execute("SELECT 1".into(), false, token(), 5)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn sqlite_foreign_keys_large_values_and_duplicate_labels() {
    let (dir, p) = fixture();
    let db = Database::open_with_storage(
        &p,
        true,
        String::new(),
        ru_dbviewer::result_storage::Storage::new(dir.path().into()),
    )
    .await
    .unwrap();
    db.execute(
        "CREATE TABLE children(id INTEGER REFERENCES items(id))".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    assert!(
        db.execute("INSERT INTO children VALUES(999)".into(), true, token(), 5)
            .await
            .is_err()
    );
    let data=db.execute("SELECT 1 AS x, NULL AS x, CAST(x'80' AS TEXT), zeroblob(40000), printf('%.*c', 70000, 'x')".into(),false,token(),5).await.unwrap();
    assert_eq!(data.columns[0].name, data.columns[1].name);
    assert!(data.rows[0][1].text.is_none());
    assert!(data.rows[0][2].display().contains("invalid UTF-8"));
    assert!(data.rows[0][4].truncated);
    assert!(data.cells_truncated);
    assert!(!data.truncated);
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_lost_commit_reply_is_unknown_without_replay() {
    use std::{
        io::{Read, Write},
        net::{Shutdown, TcpListener, TcpStream},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    let port: u16 = std::env::var("DBVIEWER_TEST_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let profile = |port| Profile::Postgres {
        name: "commit-fault".into(),
        host: "127.0.0.1".into(),
        port,
        database: "dbviewer".into(),
        user: "dbviewer".into(),
        plaintext: true,
        password_env: String::new(),
        ca: String::new(),
        certificate: String::new(),
        key: String::new(),
    };
    let direct = Database::open(&profile(port), true, "dbviewer-test-only".into())
        .await
        .unwrap();
    direct
        .execute(
            "CREATE TABLE IF NOT EXISTS dbviewer_commit_fault(value integer)".into(),
            true,
            token(),
            5,
        )
        .await
        .unwrap();
    direct.settle(true).await.unwrap();
    direct
        .execute("DELETE FROM dbviewer_commit_fault".into(), true, token(), 5)
        .await
        .unwrap();
    direct.settle(true).await.unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let cut = Arc::new(AtomicBool::new(false));
    let did_cut = cut.clone();
    let proxy = std::thread::spawn(move || {
        let (mut front, _) = listener.accept().unwrap();
        let mut back = TcpStream::connect(("127.0.0.1", port)).unwrap();
        front
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        back.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut front_write = front.try_clone().unwrap();
        let mut back_read = back.try_clone().unwrap();
        let upstream = std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(n) = front.read(&mut bytes) {
                if n == 0 {
                    break;
                }
                if back.write_all(&bytes[..n]).is_err() {
                    break;
                }
            }
            let _ = back.shutdown(Shutdown::Both);
        });
        loop {
            let mut header = [0; 5];
            if back_read.read_exact(&mut header).is_err() {
                break;
            }
            let length = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            if !(4..=1024 * 1024).contains(&length) {
                break;
            }
            let mut body = vec![0; length - 4];
            if back_read.read_exact(&mut body).is_err() {
                break;
            }
            if header[0] == b'C' && body == b"COMMIT\0" {
                did_cut.store(true, Ordering::Release);
                let _ = front_write.shutdown(Shutdown::Both);
                break;
            }
            if front_write
                .write_all(&header)
                .and_then(|_| front_write.write_all(&body))
                .is_err()
            {
                break;
            }
        }
        let _ = front_write.shutdown(Shutdown::Both);
        let _ = back_read.shutdown(Shutdown::Both);
        upstream.join().unwrap();
    });
    let through = Database::open(&profile(proxy_port), true, "dbviewer-test-only".into())
        .await
        .unwrap();
    through
        .execute(
            "INSERT INTO dbviewer_commit_fault VALUES(1)".into(),
            true,
            token(),
            5,
        )
        .await
        .unwrap();
    let error = through.settle(true).await.unwrap_err();
    assert!(
        error.contains("unknown outcome") || error.contains("outcome unknown"),
        "{error}"
    );
    drop(through);
    proxy.join().unwrap();
    assert!(cut.load(Ordering::Acquire));
    let data = direct
        .execute(
            "SELECT COUNT(*) FROM dbviewer_commit_fault".into(),
            false,
            token(),
            5,
        )
        .await
        .unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("1"));
    direct
        .execute("DROP TABLE dbviewer_commit_fault".into(), true, token(), 5)
        .await
        .unwrap();
    direct.settle(true).await.unwrap();
}

#[tokio::test]
async fn sqlite_clipped_cells_preserve_writable_transaction() {
    let (dir, p) = fixture();
    let db = Database::open_with_storage(
        &p,
        true,
        String::new(),
        ru_dbviewer::result_storage::Storage::new(dir.path().into()),
    )
    .await
    .unwrap();
    for sql in [
        "SELECT zeroblob(32768)",
        "INSERT INTO items VALUES(3, 'third', 1) RETURNING zeroblob(40000)",
    ] {
        let data = db.execute(sql.into(), true, token(), 5).await.unwrap();
        assert!(data.cells_truncated);
        assert!(!data.truncated);
        db.settle(true).await.unwrap();
    }
    let data = db
        .execute("SELECT count(*) FROM items".into(), false, token(), 5)
        .await
        .unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("3"));
}

#[tokio::test]
async fn sqlite_recursive_result_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recursive.db");
    rusqlite::Connection::open(&path).unwrap();
    let db = Database::open(
        &Profile::Sqlite {
            name: "recursive".into(),
            path: path.to_str().unwrap().into(),
        },
        false,
        String::new(),
    )
    .await
    .unwrap();
    let data=db.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1100) SELECT x FROM n".into(),false,token(),1).await.unwrap();
    assert_eq!(data.rows.len(), 1000);
    assert!(data.truncated);
}

async fn browse_filter_contract(db: &Database, schema: &str) {
    use ru_dbviewer::browse::{Browse, Filter, Operator};
    db.execute(
        "CREATE TABLE browse_contract(id INTEGER PRIMARY KEY, \"odd name\" TEXT, amount NUMERIC)"
            .into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    db.execute("INSERT INTO browse_contract VALUES(1,'literal%\\value',1.25),(2,'plain',2),(10,'',10),(20,NULL,NULL)".into(),true,token(),5).await.unwrap();
    db.settle(true).await.unwrap();
    let table = ru_dbviewer::db::Table {
        schema: schema.into(),
        name: "browse_contract".into(),
        kind: "table".into(),
    };
    let original = db.browse(&table, 0, token()).await.unwrap();
    let mut browse = Browse {
        columns: original.columns,
        sort: Some((0, false)),
        ..Browse::default()
    };
    let expected_types = browse
        .columns
        .iter()
        .map(|column| column.kind.as_str())
        .collect::<Vec<_>>();
    let refreshed = db.browse_with(&table, 0, &browse, token()).await.unwrap();
    assert_eq!(
        refreshed
            .columns
            .iter()
            .map(|column| column.kind.as_str())
            .collect::<Vec<_>>(),
        expected_types
    );
    browse.filters.push(Filter {
        column: 2,
        op: Operator::Greater,
        value: "1.5".into(),
        enabled: true,
    });
    let filtered = db.browse_with(&table, 0, &browse, token()).await.unwrap();
    assert_eq!(
        filtered
            .columns
            .iter()
            .map(|column| column.kind.as_str())
            .collect::<Vec<_>>(),
        expected_types
    );
    assert_eq!(
        filtered
            .rows
            .iter()
            .map(|r| r[0].text.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["2", "10"]
    );
    let keys = db.browse_keys(&table, token()).await.unwrap();
    let (sql, _) = browse
        .compile(&table, 0, &keys, schema == "public", true)
        .unwrap();
    let generated = db.execute(sql, false, token(), 5).await.unwrap();
    assert_eq!(generated.rows.len(), filtered.rows.len());
    for (a, b) in generated.rows.iter().zip(filtered.rows.iter()) {
        assert_eq!(a[0].text, b[0].text);
    }
    browse.filters[0] = Filter {
        column: 1,
        op: Operator::Contains,
        value: "%\\".into(),
        enabled: true,
    };
    assert_eq!(
        db.browse_with(&table, 0, &browse, token())
            .await
            .unwrap()
            .rows
            .len(),
        1
    );
    browse.filters[0] = Filter {
        column: 1,
        op: Operator::Null,
        value: String::new(),
        enabled: true,
    };
    assert_eq!(
        db.browse_with(&table, 0, &browse, token())
            .await
            .unwrap()
            .rows[0][0]
            .text
            .as_deref(),
        Some("20")
    );
    browse.filters[0].enabled = false;
    browse.page_size = 2;
    assert_eq!(
        db.browse_with(&table, 1, &browse, token())
            .await
            .unwrap()
            .rows[0][0]
            .text
            .as_deref(),
        Some("10")
    );
    db.execute("WITH RECURSIVE n(i) AS (SELECT 21 UNION ALL SELECT i+1 FROM n WHERE i<1021) INSERT INTO browse_contract SELECT i, 'page fixture', i FROM n".into(), true, token(), 5).await.unwrap();
    db.settle(true).await.unwrap();
    browse.filters.clear();
    browse.sort = None;
    browse.page_size = 1000;
    let first = db.browse_with(&table, 0, &browse, token()).await.unwrap();
    assert_eq!(first.rows.len(), 1000);
    assert!(!first.truncated);
    assert_eq!(first.rows[999][0].text.as_deref(), Some("1016"));
    let last = db.browse_with(&table, 1, &browse, token()).await.unwrap();
    assert_eq!(last.rows.len(), 5);
    assert_eq!(last.rows[0][0].text.as_deref(), Some("1017"));
    assert_eq!(last.rows[4][0].text.as_deref(), Some("1021"));
    assert!(
        db.browse_with(&table, 2, &browse, token())
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    db.execute("DROP TABLE browse_contract".into(), true, token(), 5)
        .await
        .unwrap();
    db.settle(true).await.unwrap();
}
#[tokio::test]
async fn sqlite_interactive_browse_parameters_and_generated_sql() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("browse.db");
    rusqlite::Connection::open(&path).unwrap();
    let db = Database::open(
        &Profile::Sqlite {
            name: "browse".into(),
            path: path.to_str().unwrap().into(),
        },
        true,
        String::new(),
    )
    .await
    .unwrap();
    browse_filter_contract(&db, "main").await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL: DBVIEWER_TEST_PG_PORT"]
async fn postgres_interactive_browse_parameters_and_generated_sql() {
    let port = std::env::var("DBVIEWER_TEST_PG_PORT")
        .expect("isolated PostgreSQL port")
        .parse()
        .unwrap();
    let p = Profile::Postgres {
        name: "browse".into(),
        host: "127.0.0.1".into(),
        port,
        database: "dbviewer".into(),
        user: "dbviewer".into(),
        plaintext: true,
        password_env: String::new(),
        ca: String::new(),
        certificate: String::new(),
        key: String::new(),
    };
    let db = Database::open(&p, true, "dbviewer-test-only".into())
        .await
        .unwrap();
    browse_filter_contract(&db, "public").await;
}

#[tokio::test]
async fn sqlite_full_capture_preserves_original_results_without_replay() {
    use ru_dbviewer::{result_storage::Storage, results::Representation};
    let (dir, profile) = fixture();
    let storage = Storage::new(dir.path().into());
    let db = Database::open_with_storage(&profile, true, String::new(), storage.clone())
        .await
        .unwrap();
    let data = db.execute("SELECT printf('%.*c', 70000, 'x') || 'sentinel', zeroblob(40000), CAST(zeroblob(40000) || x'80' AS TEXT)".into(), false, token(), 5).await.unwrap();
    assert!(
        data.rows[0]
            .iter()
            .all(|c| c.truncated && c.full_available())
    );
    assert_eq!(storage.usage().1, 1);
    assert!(
        data.rows[0][0]
            .load_full()
            .await
            .unwrap()
            .unwrap()
            .ends_with("sentinel")
    );
    assert_eq!(data.rows[0][1].representation, Representation::Binary);
    assert_eq!(
        data.rows[0][1].load_full().await.unwrap().unwrap().len(),
        80003
    );
    assert_eq!(data.rows[0][2].representation, Representation::InvalidUtf8);
    assert!(
        data.rows[0][2]
            .load_full()
            .await
            .unwrap()
            .unwrap()
            .ends_with("80\"")
    );
    // Writable RETURNING is captured once; loading survives subsequent database changes.
    let written = db
        .execute(
            "INSERT INTO items(name) VALUES(printf('%.*c', 70000, 'z')) RETURNING name".into(),
            true,
            token(),
            5,
        )
        .await
        .unwrap();
    let retained = written.rows[0][0].clone();
    drop(written);
    db.settle(true).await.unwrap();
    db.execute(
        "DELETE FROM items WHERE name LIKE 'z%'".into(),
        true,
        token(),
        5,
    )
    .await
    .unwrap();
    db.settle(true).await.unwrap();
    assert_eq!(
        retained.load_full().await.unwrap().unwrap(),
        "z".repeat(70000)
    );
    assert_eq!(
        retained.load_full().await.unwrap().unwrap(),
        "z".repeat(70000)
    );
    drop(retained);
    drop(data);
    drop(db);
    assert_eq!(storage.usage(), (0, 0));
}

#[tokio::test]
async fn sqlite_unavailable_capture_keeps_preview_and_pending_transaction() {
    use ru_dbviewer::result_storage::{MAX_FULL_VALUE, Storage};
    let (dir, profile) = fixture();
    let missing = Storage::new(dir.path().join("missing"));
    let db = Database::open_with_storage(&profile, true, String::new(), missing.clone())
        .await
        .unwrap();
    let data = db
        .execute(
            "INSERT INTO items(name) VALUES(printf('%.*c', 70000, 'q')) RETURNING name".into(),
            true,
            token(),
            5,
        )
        .await
        .unwrap();
    assert!(!data.truncated);
    assert!(data.rows[0][0].truncated);
    assert!(!data.rows[0][0].full_available());
    assert!(
        data.rows[0][0]
            .unavailable_reason()
            .unwrap()
            .contains("unavailable")
    );
    db.settle(true).await.unwrap();
    assert_eq!(missing.usage(), (0, 0));
    let oversized = db
        .execute(
            format!("SELECT printf('%.*c', {}, 'x')", MAX_FULL_VALUE + 1),
            false,
            token(),
            5,
        )
        .await
        .unwrap();
    assert!(
        oversized.rows[0][0]
            .unavailable_reason()
            .unwrap()
            .contains("8 MiB")
    );
}

#[tokio::test]
async fn sqlite_capture_deadline_rolls_back_short_statements_before_vm_progress() {
    let (dir, profile) = fixture();
    let storage = ru_dbviewer::result_storage::Storage::new(dir.path().into());
    let db = Database::open_with_storage(&profile, true, String::new(), storage.clone())
        .await
        .unwrap();
    assert!(
        db.execute(
            "INSERT INTO items(name) VALUES('deadline') RETURNING name".into(),
            true,
            token(),
            0
        )
        .await
        .is_err()
    );
    let data = db
        .execute("SELECT COUNT(*) FROM items".into(), false, token(), 5)
        .await
        .unwrap();
    assert_eq!(data.rows[0][0].text.as_deref(), Some("2"));
    assert_eq!(storage.usage(), (0, 0));
}
