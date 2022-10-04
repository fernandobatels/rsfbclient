//! Types, traits and constants to abstract over the different
//! implementations of the firebird client

pub mod charset;
mod connection;
pub mod date_time;
pub(crate) mod error;
pub mod ibase;
mod params;
mod row;
mod transaction;

pub use charset::Charset;
pub use connection::*;
pub use error::FbError;
pub use params::*;
pub use row::*;
pub use transaction::*;

#[derive(Debug, Clone)]
/// Sql parameter / column data
pub enum SqlType {
    Text(String),

    Integer(i64),

    Floating(f64),

    Timestamp(chrono::NaiveDateTime),

    Binary(Vec<u8>),

    /// Only works in fb >= 3.0
    Boolean(bool),

    Null,
}

impl SqlType {
    /// Returns `true` if the type is `NULL`
    pub fn is_null(&self) -> bool {
        matches!(self, Null)
    }
}
