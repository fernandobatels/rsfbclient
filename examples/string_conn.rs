//!
//! Rust Firebird Client
//!
//! Example of string connection configuration
//!
//! How to use: `DATABASE_URL=firebird://SYSDBA:masterkey@localhost:3050/test.fdb?charset=ascii cargo run --example string_conn`

#![allow(unused_variables, unused_mut)]

use rsfbclient::{prelude::*, FbError};
use std::env;

fn main() -> Result<(), FbError> {
    let string_conf =
        env::var("DATABASE_URL").map_err(|e| FbError::from("DATABASE_URL env var is empty"))?;

    #[cfg(feature = "native_client")]
    let mut conn = rsfbclient::builder_native()
        .with_string(string_conf.clone())?
        .connect()?;

    #[cfg(feature = "pure_rust")]
    let mut conn = rsfbclient::builder_pure_rust()
        .with_string(string_conf.clone())?
        .connect()?;

    let rows = conn.query_iter("SELECT a.RDB$RELATION_NAME FROM RDB$RELATIONS a WHERE COALESCE(RDB$SYSTEM_FLAG, 0) = 0 AND RDB$RELATION_TYPE = 0", ())?;

    println!("-------------");
    println!("Table name");
    println!("-------------");

    for row in rows {
        let (r_name,): (String,) = row?;

        println!("{:^10}", r_name);
    }

    Ok(())
}
