//!
//! Rust Firebird Client
//!
//! Example of transaction builder and transaction utilization
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::{prelude::*, FbError};

fn main() -> Result<(), FbError> {
    #[cfg(feature = "linking")]
    let mut builder = rsfbclient::builder_native()
        .with_dyn_link()
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .clone();

    #[cfg(feature = "dynamic_loading")]
    let mut builder = rsfbclient::builder_native()
        .with_dyn_load("./fbclient.lib")
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .clone();

    #[cfg(feature = "pure_rust")]
    let mut builder = rsfbclient::builder_pure_rust()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .clone();

    let mut conn = builder
        .with_transaction(|tr| tr.read_write().wait(120).with_consistency())
        .connect()?;

    // Default transaction
    let _: Vec<(String, String)> = conn.query(
        "select mon$attachment_name, mon$user from mon$attachments",
        (),
    )?;

    conn.rollback()?;

    // New scoped transaction
    conn.with_transaction(|tr| {
        let _: Vec<(String, String)> = tr.query(
            "select mon$attachment_name, mon$user from mon$attachments",
            (),
        )?;

        Ok(())
    })?;

    // New scoped transaction, but with a diferent transaction conf
    conn.with_transaction_config(transaction_builder().no_wait().build(), |tr| {
        let _: Vec<(String, String)> = tr.query(
            "select mon$attachment_name, mon$user from mon$attachments",
            (),
        )?;

        Ok(())
    })?;

    // New default transaction, but with a diferent transaction conf too
    conn.begin_transaction_config(transaction_builder().read_only().build())?;

    Ok(())
}
