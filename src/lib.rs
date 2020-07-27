//!
//! Rust Firebird Client
//!

pub mod prelude {
    pub use crate::query::Queryable;
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
    query::Queryable,
    row::Row,
    statement::Statement,
    status::FbError,
    transaction::Transaction,
};
