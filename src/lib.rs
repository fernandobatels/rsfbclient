//!
//! Rust Firebird Client
//!

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
    connection::{Connection, ConnectionBuilder},
    query::{Execute, Queryable},
    statement::Statement,
    transaction::Transaction,
};
pub use rsfbclient_core::{charset, Column, Dialect, FbError, FromRow, IntoParam, IntoParams, ParamsType, Row, SqlType};

#[cfg(feature = "pool")]
pub use crate::connection::pool::FirebirdConnectionManager;

#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
pub use rsfbclient_native;

#[cfg(feature = "pure_rust")]
pub use rsfbclient_rust;
