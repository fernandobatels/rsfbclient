//! The Firebird Diesel
//!
//! This crate only implements the firebird backend for [Diesel](https://docs.diesel.rs/2.0.x/diesel/index.html). To use diesel features, you must import it.
//!
//! By default the lib will use the [native client](../rsfbclient_native/struct.NativeFbClient.html). If you want
//! use the [pure rust client](../rsfbclient_rust/struct.RustFbClient.html), enable the `pure_rust` feature.
//!
//! ### Establishing a connection
//! ```rust,ignore
//! use diesel::prelude::*;
//! use rsfbclient_diesel::FbConnection;
//!
//! let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb");
//! ```

mod fb;
pub use fb::*;

#[cfg(any(feature = "linking", feature = "pure_rust"))]
#[cfg(test)]
mod tests;
