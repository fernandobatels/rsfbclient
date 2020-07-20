//!
//! Rust Firebird Client
//!

extern crate libc;

mod connection;
mod status;
#[allow(clippy::redundant_static_lifetimes)]
mod ibase;
mod row;
mod statement;
mod transaction;
mod xsqlda;

pub use self::connection::Connection;
pub use self::status::FbError;
pub use self::status::Status;
pub use self::row::Row;
pub use self::statement::Statement;
pub use self::transaction::Transaction;
