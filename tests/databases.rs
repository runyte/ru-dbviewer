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
        &make("localhost", ca.clone()),
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
    browse.filters.push(Filter {
        column: 2,
        op: Operator::Greater,
        value: "1.5".into(),
        enabled: true,
    });
    let filtered = db.browse_with(&table, 0, &browse, token()).await.unwrap();
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
