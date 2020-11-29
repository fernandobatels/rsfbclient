//!
//! Rust Firebird Client
//!
//! Connection tests
//!

use crate::*;

#[test]
#[cfg(all(feature = "linking", not(feature = "embedded_tests")))]
fn string_conn_linking_remote_host() -> Result<(), FbError> {
    builder_native()
        .with_string::<DynLink, &str>(
            "firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3",
        )?
        .connect()?;

    builder_native()
        .with_string::<DynLink, &str>(
            "firebird://localhost:3050/test.fdb",
        )?
        .connect()?;

    Ok(())
}

#[test]
#[cfg(all(feature = "linking", feature = "embedded_tests"))]
fn string_conn_linking_embedded() -> Result<(), FbError> {
    builder_native()
        .with_string::<DynLink, &str>(
            "firebird://test.fdb?dialect=3",
        )?
        .connect()?;

    Ok(())
}
