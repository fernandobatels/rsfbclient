///
/// Rust Firebird Client 
///

extern crate libc;

mod ibase;
mod error;
mod connection;
mod transaction;

pub use self::connection::Connection;
pub use self::error::FbError;
pub use self::transaction::Transaction;
