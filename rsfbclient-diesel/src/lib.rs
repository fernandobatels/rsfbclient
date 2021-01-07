//! The Firebird Diesel
//!
//! This crate re-export all diesel API with the intention of simulate the others [Diesel](https://docs.diesel.rs/diesel/index.html) implementations.
//!
//! ### To use, you must use this create as an alias to the official diesel crate
//! ```rust,ignore
//! // Cargo.toml
//! diesel = { package = "rsfbclient-diesel", version = "..." }
//! ```
//! By default the lib will use the [native client](../rsfbclient_native/struct.NativeFbClient.html). If you want
//! use the [pure rust client](../rsfbclient_rust/struct.RustFbClient.html), enable the `pure_rust` feature:
//! ```rust,ignore
//! // Cargo.toml
//! diesel = { package = "rsfbclient-diesel", version = "...", features = ["pure_rust"] }
//! ```
//!
//! ### Establishing a connection
//! ```rust,ignore
//! use diesel::prelude::*;
//! use diesel::fb::FbConnection;
//!
//! let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb");
//! ```

pub mod fb;
use diesel;

pub use crate::diesel::*;

#[cfg(not(any(feature = "dynamic_loading", feature = "embedded_tests")))]
#[cfg(test)]
mod tests;
