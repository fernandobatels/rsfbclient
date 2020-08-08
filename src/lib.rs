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
mod status;
mod transaction;
mod xsqlda;

pub use crate::{
    connection::{Connection, ConnectionBuilder, Dialect},
    query::{Execute, Queryable},
    row::Row,
    statement::Statement,
    transaction::Transaction,
};
pub use rsfbclient_core::FbError;

#[cfg(feature = "pool")]
pub use crate::connection::pool::FirebirdConnectionManager;
