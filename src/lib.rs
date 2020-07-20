//!
//! Rust Firebird Client
//!

extern crate libc;

mod connection;
#[allow(clippy::redundant_static_lifetimes)]
mod ibase;
mod row;
mod statement;
mod status;
mod transaction;
mod xsqlda;

pub use self::connection::Connection;
pub use self::row::Row;
pub use self::statement::Statement;
pub use self::status::FbError;
pub use self::status::Status;
pub use self::transaction::Transaction;
