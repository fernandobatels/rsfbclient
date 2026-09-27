//! Opt-in native acceptance tests. Creates/drops uniquely named databases only.
//! Set RSFB_TEST_DIR (existing server-side directory), RSFB_TEST_CLIENT (library),
//! RSFB_TEST_HOST, RSFB_TEST_PORT, ISC_USER and ISC_PASSWORD. Run serially with
//! cargo test -p rsfbclient-native --features dynamic_loading --test managed -- --ignored --test-threads=1
#![cfg(feature = "dynamic_loading")]

use rsfbclient_core::*;
use rsfbclient_native::managed::*;
use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Tx = NativeTransaction<DynLoad>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    loading: DynLoad,
    configs: Vec<NativeFbAttachmentConfig>,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = env::var("RSFB_TEST_DIR")?;
        let loading = DynLoad {
            lib_path: env::var("RSFB_TEST_CLIENT")?,
            charset: "WIN1254".parse()?,
        };
        let mut fixture = Self {
            loading,
            configs: Vec::new(),
        };
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        for index in 0..2 {
            // Non-ASCII filename verifies that the connection charset does not
            // change interpretation of Rust's UTF-8 database path.
            let config = NativeFbAttachmentConfig {
                db_name: format!("{directory}/rsfb-işlem-{unique}-{index}.fdb"),
                user: env::var("ISC_USER")?,
                role_name: None,
                remote: Some(RemoteConfig {
                    host: env::var("RSFB_TEST_HOST")?,
                    port: env::var("RSFB_TEST_PORT")?.parse()?,
                    pass: env::var("ISC_PASSWORD")?,
                }),
            };
            let mut client = fixture.loading.try_to_client()?;
            let mut handle = client.create_database(&config, None, Dialect::D3)?;
            fixture.configs.push(config);
            client.detach_database(&mut handle)?;
        }
        Ok(fixture)
    }
    fn start(&self, both: bool, write: bool) -> Result<Tx, FbError> {
        NativeTransaction::start(
            self.loading.try_to_client()?,
            &self.configs[..if both { 2 } else { 1 }],
            Dialect::D3,
            TransactionConfiguration {
                isolation: TrIsolationLevel::Concurrency,
                lock_resolution: TrLockResolution::NoWait,
                data_access: if write {
                    TrDataAccessMode::ReadWrite
                } else {
                    TrDataAccessMode::ReadOnly
                },
            },
            RowConversion::TextAndBinary,
        )
    }
    fn scalar(&self, sql: &str) -> Result<String, FbError> {
        scalar(&mut self.start(false, false)?, 0, sql)
    }
    fn table(&self) -> Result<(), FbError> {
        let mut tx = self.start(true, true)?;
        for index in 0..2 {
            tx.query(index, "create table rsfb_managed (id integer not null primary key, amount numeric(18,4), txt varchar(80))", vec![])?;
        }
        tx.commit()
    }
    fn subscribe(&self) -> Result<NativeEvents<DynLoad>, FbError> {
        NativeEvents::subscribe(
            self.loading.try_to_client()?,
            &self.configs[0],
            Dialect::D3,
            "RSFB_MANAGED_TEST",
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for config in &self.configs {
            let result = (|| -> Result<(), FbError> {
                let mut client = self.loading.try_to_client()?;
                let mut handle = client.attach_database(config, Dialect::D3, false)?;
                client.drop_database(&mut handle)
            })();
            assert!(
                result.is_ok(),
                "Could not drop an owned test database: {result:?}"
            );
        }
    }
}
fn scalar(tx: &mut Tx, database: usize, sql: &str) -> Result<String, FbError> {
    match &tx.query(database, sql, vec![])?[0][0].value {
        SqlType::Text(value) => Ok(value.trim().to_owned()),
        _ => Err("Expected text scalar".into()),
    }
}
fn post(fixture: &Fixture, commit: bool) -> Result<(), FbError> {
    let mut tx = fixture.start(false, true)?;
    tx.query(
        0,
        "execute block as begin post_event 'RSFB_MANAGED_TEST'; end",
        vec![],
    )?;
    if commit {
        tx.commit()
    } else {
        tx.rollback()
    }
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn query_each_streams_rows_and_preserves_conversion() -> TestResult {
    let f = Fixture::new()?;
    let mut tx = f.start(false, false)?;
    tx.query_each(0, "select 1 from rdb$database where 1=0", vec![], |_| {
        panic!("An empty result must not invoke the callback")
    })?;
    let mut count = 0;
    tx.query_each(
        0,
        "with recursive counter(n) as (select 1 from rdb$database union all select n+1 from counter where n < 200) select n from counter order by n",
        vec![],
        |row| {
            count += 1;
            assert!(matches!(&row[0].value, SqlType::Text(s) if s.trim() == count.to_string()));
            Ok(())
        },
    )?;
    assert_eq!(count, 200);
    let mut calls = 0;
    tx.query_each(
        0,
        "select cast(? as numeric(18,4)), cast(? as varchar(40)), cast(null as integer), cast('' as varchar(1)), cast(? as blob sub_type 0), cast(? as timestamp) from rdb$database",
        vec![
            SqlType::Text("-90071992547409.1234".into()),
            SqlType::Text("İşlem ığüşöç".into()),
            SqlType::Binary(vec![0, 255, 65]),
            SqlType::Text("2026-09-27 12:34:56.1234".into()),
        ],
        |row| {
            calls += 1;
            assert!(matches!(&row[0].value, SqlType::Text(s) if s.trim() == "-90071992547409.1234"));
            assert!(matches!(&row[1].value, SqlType::Text(s) if s == "İşlem ığüşöç"));
            assert!(matches!(row[2].value, SqlType::Null));
            assert!(matches!(&row[3].value, SqlType::Text(s) if s.is_empty()));
            assert!(matches!(&row[4].value, SqlType::Binary(b) if b == &[0, 255, 65]));
            assert!(matches!(&row[5].value, SqlType::Text(s) if s.contains("12:34:56.1234")));
            Ok(())
        },
    )?;
    assert_eq!(calls, 1);
    tx.rollback()?;
    let mut native = NativeTransaction::start(
        f.loading.try_to_client()?,
        &f.configs[..1],
        Dialect::D3,
        Default::default(),
        RowConversion::Native,
    )?;
    native.query_each(
        0,
        "select cast(42 as bigint), cast(12.25 as numeric(18,2)) from rdb$database",
        vec![],
        |row| {
            assert!(matches!(row[0].value, SqlType::Integer(42)));
            assert!(matches!(row[1].value, SqlType::Floating(value) if value == 12.25));
            Ok(())
        },
    )?;
    native.rollback()?;
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn query_each_supports_returning_and_statements_without_output() -> TestResult {
    let f = Fixture::new()?;
    f.table()?;
    let mut tx = f.start(true, true)?;
    let mut returned = 0;
    tx.query_each(
        1,
        "insert into rsfb_managed (id) values (?) returning id",
        vec![SqlType::Integer(7)],
        |row| {
            returned += 1;
            assert!(matches!(&row[0].value, SqlType::Text(s) if s.trim() == "7"));
            Ok(())
        },
    )?;
    assert_eq!(returned, 1);
    tx.query_each(
        1,
        "update rsfb_managed set amount=12.5 where id=7",
        vec![],
        |_| panic!("A statement without output must not invoke the callback"),
    )?;
    assert_eq!(
        scalar(&mut tx, 1, "select amount from rsfb_managed where id=7")?,
        "12.5000"
    );
    tx.query_each(1, "delete from rsfb_managed where id=7", vec![], |_| {
        panic!("A statement without output must not invoke the callback")
    })?;
    assert_eq!(
        scalar(&mut tx, 1, "select count(*) from rsfb_managed")?,
        "0"
    );
    tx.rollback()?;
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn query_each_callback_errors_release_statements_and_keep_transaction() -> TestResult {
    let f = Fixture::new()?;
    f.table()?;
    let mut tx = f.start(false, true)?;
    for _ in 0..30 {
        let mut visited = 0;
        let error = tx
            .query_each(
                0,
                "select 1 from rdb$database union all select 2 from rdb$database",
                vec![],
                |_| {
                    visited += 1;
                    Err("intentional callback failure".into())
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("intentional callback failure"));
        assert_eq!(visited, 1);
        assert!(tx
            .query_each(
                0,
                "select cast(? as integer) from rdb$database",
                vec![],
                |_| Ok(())
            )
            .is_err());
        assert!(tx
            .query_each(0, "select absent_column from rdb$database", vec![], |_| Ok(
                ()
            ))
            .is_err());
    }
    assert_eq!(
        scalar(
            &mut tx,
            0,
            "select count(*) from mon$statements where mon$attachment_id=current_connection"
        )?,
        "1"
    );
    let error = tx
        .query_each(
            0,
            "insert into rsfb_managed (id) values (1) returning id",
            vec![],
            |_| Err("returning callback failure".into()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("returning callback failure"));
    // Callback errors do not implicitly undo already executed DML.
    assert_eq!(
        scalar(&mut tx, 0, "select count(*) from rsfb_managed")?,
        "1"
    );
    assert!(tx
        .query_each(9, "select 1 from rdb$database", vec![], |_| Ok(()))
        .is_err());
    tx.rollback()?;
    assert!(tx
        .query_each(0, "select 1 from rdb$database", vec![], |_| Ok(()))
        .is_err());
    assert_eq!(f.scalar("select count(*) from rsfb_managed")?, "0");
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn native_typed_conversion_is_unchanged() -> TestResult {
    let f = Fixture::new()?;
    let mut tx = NativeTransaction::start(
        f.loading.try_to_client()?,
        &f.configs[..1],
        Dialect::D3,
        Default::default(),
        RowConversion::Native,
    )?;
    let rows = tx.query(0, "select cast(42 as bigint), cast(12.25 as numeric(18,2)), cast('İşlem' as varchar(20)) from rdb$database", vec![])?;
    assert!(matches!(rows[0][0].value, SqlType::Integer(42)));
    assert!(matches!(rows[0][1].value, SqlType::Floating(f) if f == 12.25));
    assert!(matches!(&rows[0][2].value, SqlType::Text(s) if s == "İşlem"));
    tx.rollback()?;
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn exact_scalars_blobs_and_statement_error_cleanup() -> TestResult {
    let f = Fixture::new()?;
    let mut tx = f.start(false, true)?;
    tx.query(0, "create table rsfb_values (id integer not null primary key, n numeric(18,2), d numeric(18,4), txt varchar(80), empty_text varchar(80), nullable varchar(80), ts timestamp, dt date, tm time, binary_data blob sub_type 0, text_data blob sub_type 1 character set win1254)", vec![])?;
    tx.commit()?;
    let text = "İşlem ığüşöç".repeat(12_000);
    let binary: Vec<u8> = (0..180_000).map(|n| (n % 256) as u8).collect();
    let mut tx = f.start(false, true)?;
    let result = tx.query(
        0,
        "insert into rsfb_values values (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) returning id",
        vec![
            SqlType::Text("90071992547409.93".into()),
            SqlType::Text("-90071992547409.1234".into()),
            SqlType::Text("İşlem ığüşöç".into()),
            SqlType::Text(String::new()),
            SqlType::Null,
            SqlType::Text("2026-09-17 14:20:23.5660".into()),
            SqlType::Text("2026-09-17".into()),
            SqlType::Text("14:20:23.5660".into()),
            SqlType::Binary(binary.clone()),
            SqlType::Text(text.clone()),
        ],
    )?;
    assert!(matches!(&result[0][0].value, SqlType::Text(s) if s.trim() == "1"));
    tx.commit()?;
    assert!(tx.query(0, "select 1 from rdb$database", vec![]).is_err());
    assert!(tx.commit().is_err());
    assert!(tx.rollback().is_err());
    let mut tx = f.start(false, false)?;
    let rows = tx.query(
        0,
        "select n,d,txt,empty_text,nullable,ts,dt,tm,binary_data,text_data from rsfb_values",
        vec![],
    )?;
    for (index, expected) in [
        (0, "90071992547409.93"),
        (1, "-90071992547409.1234"),
        (2, "İşlem ığüşöç"),
        (3, ""),
    ] {
        assert!(
            matches!(&rows[0][index].value, SqlType::Text(s) if s.trim() == expected),
            "column {index}: {:?}",
            rows[0][index].value
        );
    }
    assert!(matches!(rows[0][4].value, SqlType::Null));
    assert!(matches!(&rows[0][5].value, SqlType::Text(s) if s.contains("14:20:23.566")));
    assert!(matches!(&rows[0][6].value, SqlType::Text(s) if s.contains("2026")));
    assert!(matches!(&rows[0][7].value, SqlType::Text(s) if s.contains("14:20:23.566")));
    assert!(matches!(&rows[0][8].value, SqlType::Binary(b) if b == &binary));
    let expected = f.loading.charset.encode(text)?.into_owned();
    assert!(matches!(&rows[0][9].value, SqlType::Binary(b) if b == &expected));
    assert!(tx.query(0, "delete from rsfb_values", vec![]).is_err());
    tx.rollback()?;
    let mut tx = f.start(false, true)?;
    for _ in 0..30 {
        assert!(tx
            .query(0, "select absent_column from rdb$database", vec![])
            .is_err());
        assert!(tx
            .query(0, "select id from rsfb_values where id=?", vec![])
            .is_err());
        assert!(tx
            .query(
                0,
                "select id from rsfb_values where id=?",
                vec![SqlType::Integer(1), SqlType::Integer(2)]
            )
            .is_err());
        assert!(tx
            .query(0, "insert into rsfb_values (id) values (1)", vec![])
            .is_err());
        // A fetch conversion error also releases the prepared statement.
        assert!(tx
            .query(0, "select cast(txt as integer) from rsfb_values", vec![])
            .is_err());
        assert_eq!(scalar(&mut tx, 0, "select count(*) from rsfb_values")?, "1");
    }
    assert_eq!(
        scalar(
            &mut tx,
            0,
            "select count(*) from mon$statements where mon$attachment_id=current_connection"
        )?,
        "1"
    );
    tx.rollback()?;
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn isolation_drop_rollback_and_failed_participant_attachment() -> TestResult {
    let f = Fixture::new()?;
    f.table()?;
    let mut initial = f.start(false, true)?;
    initial.query(
        0,
        "insert into rsfb_managed values (1, 10, 'before')",
        vec![],
    )?;
    initial.commit()?;
    drop(initial);
    let mut snapshot = f.start(false, false)?;
    assert_eq!(
        scalar(&mut snapshot, 0, "select txt from rsfb_managed")?,
        "before"
    );
    let mut writer = f.start(false, true)?;
    writer.query(0, "update rsfb_managed set txt='after' where id=1", vec![])?;
    let mut competitor = f.start(false, true)?;
    let started = Instant::now();
    assert!(competitor
        .query(
            0,
            "update rsfb_managed set txt='conflict' where id=1",
            vec![]
        )
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    competitor.rollback()?;
    writer.commit()?;
    assert_eq!(
        scalar(&mut snapshot, 0, "select txt from rsfb_managed")?,
        "before"
    );
    snapshot.rollback()?;
    assert_eq!(f.scalar("select txt from rsfb_managed")?, "after");
    {
        let mut dropped = f.start(false, true)?;
        dropped.query(0, "delete from rsfb_managed", vec![])?;
    }
    assert_eq!(f.scalar("select count(*) from rsfb_managed")?, "1");
    let mut invalid = f.configs.clone();
    invalid[1].db_name.push_str(".missing");
    assert!(NativeTransaction::start(
        f.loading.try_to_client()?,
        &invalid,
        Dialect::D3,
        Default::default(),
        RowConversion::Native
    )
    .is_err());
    assert!(NativeTransaction::start(
        f.loading.try_to_client()?,
        &[],
        Dialect::D3,
        Default::default(),
        RowConversion::Native
    )
    .is_err());
    assert!(snapshot
        .query(9, "select 1 from rdb$database", vec![])
        .is_err());
    drop(snapshot);
    drop(writer);
    drop(competitor);
    assert_eq!(f.scalar("select count(*) from mon$attachments")?, "1");
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn distributed_prepare_commit_rollback_and_limbo_recovery() -> TestResult {
    let f = Fixture::new()?;
    f.table()?;
    for (id, prepare, commit) in [(1, false, false), (2, true, false), (3, true, true)] {
        let mut tx = f.start(true, true)?;
        for database in 0..2 {
            tx.query(
                database,
                "insert into rsfb_managed (id) values (?)",
                vec![SqlType::Integer(id)],
            )?;
        }
        assert!(tx.prepare(&[]).is_err());
        if prepare {
            tx.prepare(b"rsfbclient native integration")?;
            assert!(tx.query(0, "select 1 from rdb$database", vec![]).is_err());
        }
        if commit {
            tx.commit()?;
        } else {
            tx.rollback()?;
        }
        let mut read = f.start(true, false)?;
        for database in 0..2 {
            assert_eq!(
                scalar(
                    &mut read,
                    database,
                    &format!("select count(*) from rsfb_managed where id={id}")
                )?,
                if commit { "1" } else { "0" }
            );
        }
    }
    for (id, commit) in [(4, false), (5, true)] {
        let mut tx = f.start(true, true)?;
        let mut ids = Vec::new();
        for database in 0..2 {
            tx.query(
                database,
                "insert into rsfb_managed (id) values (?)",
                vec![SqlType::Integer(id)],
            )?;
            ids.push(
                scalar(
                    &mut tx,
                    database,
                    "select current_transaction from rdb$database",
                )?
                .parse::<u32>()?,
            );
        }
        tx.prepare(b"rsfbclient detached prepared transaction")?;
        drop(tx);
        let mut recovery = f.start(true, false)?;
        for (database, transaction_id) in ids.into_iter().enumerate() {
            assert!(recovery.limbo_ids(database)?.contains(&transaction_id));
            recovery.resolve_limbo(database, transaction_id, commit)?;
            assert!(!recovery.limbo_ids(database)?.contains(&transaction_id));
            assert!(recovery
                .resolve_limbo(database, transaction_id, commit)
                .is_err());
        }
        drop(recovery);
        let mut read = f.start(true, false)?;
        for database in 0..2 {
            assert_eq!(
                scalar(
                    &mut read,
                    database,
                    &format!("select count(*) from rsfb_managed where id={id}")
                )?,
                if commit { "1" } else { "0" }
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicit native Firebird test environment"]
fn finite_event_wait_commit_rollback_rearm_and_disconnect() -> TestResult {
    let f = Fixture::new()?;
    for name in ["", "not\0valid", "İşlem"] {
        assert!(NativeEvents::subscribe(
            f.loading.try_to_client()?,
            &f.configs[0],
            Dialect::D3,
            name
        )
        .is_err());
    }
    for _ in 0..8 {
        let mut events = f.subscribe()?;
        assert!(events.wait(Duration::from_secs(3))?);
        let started = Instant::now();
        assert!(!events.wait(Duration::from_millis(60))?);
        assert!(
            started.elapsed() >= Duration::from_millis(50)
                && started.elapsed() < Duration::from_secs(2)
        );
        post(&f, false)?;
        assert!(!events.wait(Duration::from_millis(60))?);
        for _ in 0..5 {
            post(&f, true)?;
        }
        assert!(events.wait(Duration::from_secs(3))?);
        // Re-arming consumes notifications posted while the application was busy.
        while events.wait(Duration::from_millis(60))? {}
        post(&f, true)?;
        assert!(events.wait(Duration::from_secs(3))?);
        events.ping()?;
        let started = Instant::now();
        drop(events);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(f.scalar("select count(*) from mon$attachments")?, "1");
    }
    let mut events = f.subscribe()?;
    assert!(events.wait(Duration::from_secs(3))?);
    let mut killer = f.start(false, true)?;
    killer.query(
        0,
        "delete from mon$attachments where mon$attachment_id<>current_connection",
        vec![],
    )?;
    killer.commit()?;
    drop(killer);
    assert!(events.ping().is_err());
    drop(events);
    let mut events = f.subscribe()?;
    assert!(events.wait(Duration::from_secs(3))?);
    post(&f, true)?;
    assert!(events.wait(Duration::from_secs(3))?);
    Ok(())
}
