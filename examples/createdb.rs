//!
//! Rust Firebird Client
//!
//! Example of database creation
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::FbError;

fn main() -> Result<(), FbError> {
    #[cfg(feature = "linking")]
    let mut conn = rsfbclient::builder_native()
        .with_dyn_link()
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .page_size(8 * 1024) // Optional
        .dialect(rsfbclient::Dialect::D1)
        .create_database()?;

    #[cfg(feature = "dynamic_loading")]
    let mut conn = rsfbclient::builder_native()
        .with_dyn_load("./fbclient.lib")
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .page_size(16 * 1024) // Optional
        .create_database()?;

    #[cfg(feature = "pure_rust")]
    let mut conn = rsfbclient::builder_pure_rust()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .page_size(16 * 1024) // Optional
        .dialect(rsfbclient::Dialect::D3)
        .create_database()?;

    conn.close()?;

    Ok(())
}
