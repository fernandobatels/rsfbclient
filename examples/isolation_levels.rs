//!
//! Rust Firebird Client
//!
//! Example of explicit transactions and isolation levels
//!
//! Two connections to the same database demonstrate what the
//! TransactionConfiguration options actually change inside Firebird:
//!
//!  1. Visibility: a Concurrency (SNAPSHOT) transaction does not see a
//!     commit that happens after it starts; a ReadCommited one does.
//!  2. Conflict policy: with TrLockResolution::NoWait, updating a row
//!     that another transaction updated first fails immediately with
//!     "update conflicts with concurrent update" instead of blocking.
//!
//! The example uses explicit `SimpleTransaction` objects instead of the
//! connection's default transaction on purpose: the default transaction
//! is started once with the configuration given at connect time, and
//! `Connection::commit`/`rollback` are RETAINING operations — the same
//! physical transaction (with its configuration and, under SNAPSHOT,
//! its snapshot) lives for the whole life of the connection. Explicit
//! transaction objects give each demonstration its own configuration
//! and a real, consuming commit.
//!
//! The table is created by the example itself (idempotent); you only
//! need an `examples.fdb` database.
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::{prelude::*, FbError, SimpleConnection, SimpleTransaction};

fn connect() -> Result<SimpleConnection, FbError> {
    #[cfg(feature = "linking")]
    let conn = rsfbclient::builder_native()
        .with_dyn_link()
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?
        .into();

    #[cfg(feature = "dynamic_loading")]
    let conn = rsfbclient::builder_native()
        .with_dyn_load("./fbclient.lib")
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?
        .into();

    #[cfg(feature = "pure_rust")]
    let conn = rsfbclient::builder_pure_rust()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?
        .into();

    Ok(conn)
}

fn main() -> Result<(), FbError> {
    let mut conn_a = connect()?;
    let mut conn_b = connect()?;

    let snapshot = TransactionConfiguration {
        isolation: TrIsolationLevel::Concurrency,
        lock_resolution: TrLockResolution::NoWait,
        ..TransactionConfiguration::default()
    };
    let read_committed = TransactionConfiguration {
        isolation: TrIsolationLevel::ReadCommited(TrRecordVersion::RecordVersion),
        lock_resolution: TrLockResolution::NoWait,
        ..TransactionConfiguration::default()
    };

    // Scratch table (idempotent)
    {
        let mut setup = SimpleTransaction::new(&mut conn_a, read_committed)?;
        let _ = setup.execute("drop table accounts", ());
        setup.execute(
            "create table accounts (id int not null primary key, balance int)",
            (),
        )?;
        setup.commit()?;

        let mut setup = SimpleTransaction::new(&mut conn_a, read_committed)?;
        setup.execute("insert into accounts values (1, 100)", ())?;
        setup.commit()?;
    }

    // 1. Visibility
    let mut tr_a = SimpleTransaction::new(&mut conn_a, snapshot)?;
    let (balance,): (i64,) = tr_a
        .query_first("select balance from accounts where id = 1", ())?
        .unwrap();
    println!("snapshot transaction sees balance {} at start", balance);

    {
        let mut tr_b = SimpleTransaction::new(&mut conn_b, read_committed)?;
        tr_b.execute("update accounts set balance = 150 where id = 1", ())?;
        tr_b.commit()?;
        println!("another connection committed balance = 150");
    }

    let (still,): (i64,) = tr_a
        .query_first("select balance from accounts where id = 1", ())?
        .unwrap();
    println!("snapshot transaction still sees {} (stable view)", still);
    tr_a.commit()?;

    let mut tr_rc = SimpleTransaction::new(&mut conn_a, read_committed)?;
    let (now,): (i64,) = tr_rc
        .query_first("select balance from accounts where id = 1", ())?
        .unwrap();
    println!("read committed transaction sees {} (follows commits)", now);
    tr_rc.commit()?;

    // 2. NoWait conflict
    let mut tr_a = SimpleTransaction::new(&mut conn_a, snapshot)?;
    let mut tr_b = SimpleTransaction::new(&mut conn_b, snapshot)?;
    tr_a.execute("update accounts set balance = balance + 1 where id = 1", ())?;
    println!("transaction A updated the row (uncommitted)");
    match tr_b.execute("update accounts set balance = balance + 2 where id = 1", ()) {
        Ok(_) => println!("unexpected: conflicting update succeeded"),
        Err(e) => println!("conflicting update failed as designed:\n    {}", e),
    }
    tr_b.rollback()?;
    tr_a.commit()?;

    Ok(())
}
