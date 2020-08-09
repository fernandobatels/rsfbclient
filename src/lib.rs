//!
//! Rust Firebird Client
//!

pub mod prelude {
    pub use crate::query::{Execute, Queryable};
}

mod connection;
mod params;
mod query;
mod row;
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
