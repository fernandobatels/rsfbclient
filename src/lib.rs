//!
//! Rust Firebird Client
//!

#[cfg(test)]
#[macro_use]
pub(crate) mod tests;

pub mod prelude {
    pub use crate::query::{Execute, Queryable};
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
pub use rsfbclient_core::{Dialect, FbError, IntoParam, Row};

#[cfg(feature = "pool")]
pub use crate::connection::pool::FirebirdConnectionManager;
