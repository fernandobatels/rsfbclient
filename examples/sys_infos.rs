//!
//! Rust Firebird Client
//!
//! Examples of SystemInfos traits
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::{FbError, SystemInfos};

fn main() -> Result<(), FbError> {
    #[cfg(feature = "linking")]
    let mut conn = rsfbclient::builder_native()
        .with_dyn_link()
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?;

    #[cfg(feature = "dynamic_loading")]
    let mut conn = rsfbclient::builder_native()
        .with_dyn_load("./fbclient.lib")
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?;

    #[cfg(feature = "pure_rust")]
    let mut conn = rsfbclient::builder_pure_rust()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?;

    let version = conn.server_engine()?;
    println!("Server version: {:?}", version);

    let db_name = conn.db_name()?;
    println!("Db path: {}", db_name);

    Ok(())
}
