//!
//! Rust Firebird Client
//!
//! Services API tests
//!
//! The Services API needs the native client and a remote server, so
//! unlike the other test modules these are plain feature-gated tests
//! rather than mk_tests_default! copies: there is exactly one service
//! manager implementation per native linkage, and the pure-rust client
//! cannot reach it at all.
//!
//! Note that every path handed to a service action is a SERVER path;
//! the tests use relative paths, which the server resolves against its
//! own environment — the same convention the database tests use.

#![cfg(all(
    any(feature = "linking", feature = "dynamic_loading"),
    not(feature = "embedded_tests")
))]

use crate::services::*;
use crate::*;

#[cfg(feature = "linking")]
fn svc() -> Result<ServiceManager, FbError> {
    ServiceManager::builder()
        .host("localhost")
        .user("SYSDBA")
        .pass("masterkey")
        .attach()
}

#[cfg(all(feature = "dynamic_loading", not(feature = "linking")))]
fn svc() -> Result<ServiceManager, FbError> {
    #[cfg(target_os = "linux")]
    let libfbclient = "libfbclient.so";
    #[cfg(target_os = "windows")]
    let libfbclient = "fbclient.dll";
    #[cfg(target_os = "macos")]
    let libfbclient = "libfbclient.dylib";

    ServiceManager::builder()
        .host("localhost")
        .user("SYSDBA")
        .pass("masterkey")
        .with_dyn_load(libfbclient)
        .attach()
}

#[cfg(feature = "linking")]
fn cbuilder() -> builders::NativeConnectionBuilder<builders::DynLink, builders::ConnRemote> {
    crate::builder_native().with_dyn_link().with_remote()
}

#[cfg(all(feature = "dynamic_loading", not(feature = "linking")))]
fn cbuilder() -> builders::NativeConnectionBuilder<builders::DynLoad, builders::ConnRemote> {
    #[cfg(target_os = "linux")]
    let libfbclient = "libfbclient.so";
    #[cfg(target_os = "windows")]
    let libfbclient = "fbclient.dll";
    #[cfg(target_os = "macos")]
    let libfbclient = "libfbclient.dylib";

    crate::builder_native()
        .with_dyn_load(libfbclient)
        .with_remote()
}

#[test]
fn server_version() -> Result<(), FbError> {
    let mut svc = svc()?;
    let version = svc.server_version()?;

    // e.g. "LI-V4.0.2.2816 Firebird 4.0" — don't tie the test to a
    // specific version, only to the response being a plausible
    // version string.
    assert!(!version.is_empty(), "server version came back empty");
    assert!(
        version.contains("Firebird") || version.chars().any(|c| c.is_ascii_digit()),
        "unexpected server version string: {}",
        version
    );

    Ok(())
}

#[test]
fn backup_restore_roundtrip() -> Result<(), FbError> {
    let db = "test_svc_roundtrip.fdb";
    let fbk = "test_svc_roundtrip.fbk";
    let restored = "test_svc_roundtrip_restored.fdb";

    // 1. a database with known content
    {
        let mut conn = cbuilder()
            .db_name(db)
            .user("SYSDBA")
            .pass("masterkey")
            .connect()
            .or_else(|_| {
                cbuilder()
                    .db_name(db)
                    .user("SYSDBA")
                    .pass("masterkey")
                    .create_database()
            })?;
        let _ = conn.execute("drop table svc_probe", ());
        conn.execute("create table svc_probe (id int, name varchar(20))", ())?;
        conn.execute("insert into svc_probe values (1, 'alpha')", ())?;
        conn.execute("insert into svc_probe values (2, 'beta')", ())?;
        conn.commit()?;
        conn.close()?;
    }

    // 2. verbose backup: the gbak log must actually stream
    let mut svc = svc()?;
    let mut lines = 0usize;
    svc.backup_with_output(db, fbk, SvcBackupOptions::default(), |_line| lines += 1)?;
    assert!(lines > 0, "verbose backup produced no output lines");

    // 3. restore to a new name (replace, so reruns are stable)
    let opts = SvcRestoreOptions {
        replace: true,
        ..SvcRestoreOptions::default()
    };
    svc.restore(fbk, restored, opts)?;
    svc.detach()?;

    // 4. the restored copy has the data
    let mut conn = cbuilder()
        .db_name(restored)
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?;
    let rows: Vec<(i32, String)> = conn.query("select id, name from svc_probe order by id", ())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], (1, "alpha".to_string()));
    assert_eq!(rows[1], (2, "beta".to_string()));
    conn.drop_database()?;

    Ok(())
}

#[test]
fn restore_without_replace_fails_on_existing() -> Result<(), FbError> {
    let db = "test_svc_noreplace.fdb";
    let fbk = "test_svc_noreplace.fbk";

    {
        let conn = cbuilder()
            .db_name(db)
            .user("SYSDBA")
            .pass("masterkey")
            .connect()
            .or_else(|_| {
                cbuilder()
                    .db_name(db)
                    .user("SYSDBA")
                    .pass("masterkey")
                    .create_database()
            })?;
        conn.close()?;
    }

    let mut svc = svc()?;
    svc.backup(db, fbk, SvcBackupOptions::default())?;

    // restoring over the still-existing source without replace must
    // fail — the isc_spb_res_create semantics
    let result = svc.restore(fbk, db, SvcRestoreOptions::default());
    assert!(
        result.is_err(),
        "restore without replace over an existing database should fail"
    );

    Ok(())
}

#[test]
fn backup_and_restore_can_filter_table_data_and_use_parallel_workers() -> Result<(), FbError> {
    let mut svc = svc()?;
    let version = svc.server_version()?;
    let major = version
        .split_once("-V")
        .and_then(|(_, version)| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
        .expect("Firebird service version should contain -V followed by its major version");
    if major < 4 {
        return Ok(());
    }
    let workers = (major >= 5).then_some(2);
    let db = "test_svc_filtered.fdb";
    let partial = "test_svc_filtered.fbk";
    let full = "test_svc_filtered_full.fbk";
    let restored = "test_svc_filtered_restored.fdb";
    let mut source = cbuilder()
        .db_name(db)
        .connect()
        .or_else(|_| cbuilder().db_name(db).create_database())?;
    for table in ["KEEP_DATA", "KEEP_SKIP", "OMIT_DATA"] {
        let _ = source.execute(&format!("DROP TABLE {table}"), ());
        source.execute(&format!("CREATE TABLE {table} (ID INT PRIMARY KEY)"), ())?;
        source.execute(&format!("INSERT INTO {table} VALUES (7)"), ())?;
    }
    source.close()?;

    svc.backup(
        db,
        partial,
        SvcBackupOptions {
            include_data: Some("KEEP%".into()),
            skip_data: Some("KEEP_SKIP".into()),
            parallel_workers: workers,
            ..SvcBackupOptions::default()
        },
    )?;
    svc.backup(db, full, SvcBackupOptions::default())?;
    for (archive, options) in [
        (
            partial,
            SvcRestoreOptions {
                replace: true,
                parallel_workers: workers,
                ..SvcRestoreOptions::default()
            },
        ),
        (
            full,
            SvcRestoreOptions {
                replace: true,
                include_data: Some("KEEP%".into()),
                skip_data: Some("KEEP_SKIP".into()),
                parallel_workers: workers,
                ..SvcRestoreOptions::default()
            },
        ),
    ] {
        svc.restore(archive, restored, options)?;
        let mut conn = cbuilder().db_name(restored).connect()?;
        let rows: Vec<(i32,)> = conn.query("SELECT ID FROM KEEP_DATA", ())?;
        assert_eq!(rows, [(7,)]);
        for table in ["KEEP_SKIP", "OMIT_DATA"] {
            let rows: Vec<(i32,)> = conn.query(&format!("SELECT ID FROM {table}"), ())?;
            assert!(rows.is_empty(), "{table} rows must be skipped");
        }
        conn.drop_database()?;
    }
    cbuilder().db_name(db).connect()?.drop_database()?;
    Ok(())
}
