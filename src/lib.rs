//!
//! Rust Firebird Client
//!
//! ## How to use it
//!
//! 1. Start by choosing the lib variation you want
//! ```rust
//! // To use the offcial ('native') Firebird client .dll/.so/.dylib
//! // (needs to find dll at build time)
//! rsfbclient::builder_native().with_dyn_link()
//! // Or using dynamic loading
//! rsfbclient::builder_native().with_dyn_load("/my/firebird/here/lib/libfbclient.so")
//! // Or using the pure rust implementation
//! rsfbclient::builder_pure_rust()
//! ```
//!
//! 2. Set your connection params
//! ```rust
//! // For a remote server, using a dynamically linked native client
//! let mut conn = rsfbclient::builder_native()
//!     .with_dyn_link()
//!     .with_remote()
//!     .host("my.host.com.br")
//!     .db_name("awesome.fdb")
//!     .connect()?
//! // Or if you need a embedded/local only access
//! let mut conn = rsfbclient::builder_native()
//!     .with_dyn_link()
//!     .with_embedded()
//!     .db_name("/path/to/awesome.fdb")
//!     .connect()?
//! ```
//!
//! You also can choose a string connection configuration
//! ```rust
//! // Using the native Firebird client
//! rsfbclient::builder_native()
//!     .from_string("firebird://SYSDBA:masterkey@my.host.com.br:3050/awesome.fdb?charset=ascii")
//! // Or using the pure rust implementation
//! rsfbclient::builder_pure_rust()
//!     .from_string("firebird://SYSDBA:masterkey@my.host.com.br:3050/awesome.fdb?charset=ascii")
//! ```
//!
//! 3. Now you can use the lib
//! ```rust
//! let rows = conn.query_iter("select col_a, col_b, col_c from test", ())?;
//! ...
//! ```
//!
//! ## Cargo features
//! All features can be used at the same time if needed.
//!
//! ### `linking`
//! Will use the dynamic library of the official `fbclient` at runtime and compiletime. Used in systems where there is already a firebird client installed and configured.
//! ### `dynamic_loading`
//! Can find the official `fbclient` native library by path at runtime, does not need the library at compiletime. Useful when you need to build in a system without a firebird client installed.
//! ### `pure_rust`
//! Uses a pure rust implementation of the firebird wire protocol, does not need the native library at all. Useful for cross-compilation and allow a single binary to be deployed without needing to install the firebird client.

#[cfg(test)]
#[macro_use]
pub(crate) mod tests;

pub mod prelude {
    pub use crate::query::{Execute, Queryable};
    pub use rsfbclient_derive::IntoParams;
}

mod connection;
mod query;
mod statement;
mod transaction;

pub use crate::{
    connection::{Connection, ConnectionConfiguration, FirebirdClientFactory, SimpleConnection},
    query::{Execute, Queryable},
    statement::Statement,
    transaction::{SimpleTransaction, Transaction},
};
pub use rsfbclient_core::{
    Column, Dialect, FbError, FromRow, IntoParam, IntoParams, ParamsType, Row, SqlType,
};

#[doc(hidden)]
pub use rsfbclient_core::{charset, Charset};

#[cfg(feature = "pool")]
pub use crate::connection::pool::FirebirdConnectionManager;

//builders are behind feature gates inside this module
pub use crate::connection::builders;
pub use builders::*;
