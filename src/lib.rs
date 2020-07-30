//!
//! Rust Firebird Client
//!

pub mod prelude {
    pub use crate::query::{Execute, Queryable};
}

mod connection;
#[cfg(feature = "chrono")]
mod date_time;
#[allow(clippy::redundant_static_lifetimes)]
mod ibase;
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
    status::FbError,
    transaction::Transaction,
};

#[cfg(feature = "pool")]
pub use crate::connection::pool::FirebirdConnectionManager;
