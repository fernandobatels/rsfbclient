//!
//! Rust Firebird Client
//!

mod connection;
#[cfg(feature = "chrono")]
mod date_time;
#[allow(clippy::redundant_static_lifetimes)]
mod ibase;
mod params;
mod row;
mod statement;
mod status;
mod transaction;
mod xsqlda;

pub use crate::{
    connection::{Connection, ConnectionBuilder, Dialect},
    row::Row,
    statement::Statement,
    status::FbError,
    transaction::Transaction,
};
