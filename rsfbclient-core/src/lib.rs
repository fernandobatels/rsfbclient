//! Types, traits and constants to abstract over the different
//! implementations of the firebird client

pub mod charset;
mod connection;
pub mod date_time;
mod decimal;
pub(crate) mod error;
pub mod ibase;
mod params;
mod row;
mod sql_params;
mod statement_info;
mod transaction;

pub use charset::Charset;
pub use connection::*;
pub use error::{ExternalPsqlContext, FbError, SqlDiagnostics};
pub use params::*;
pub use row::*;
pub use statement_info::*;
pub use transaction::*;

pub use decimal::FixedDecimal;

#[derive(Debug, Clone)]
/// Sql parameter / column data
pub enum SqlType {
    Text(String),

    /// Bind UTF-8 text as a text BLOB. Result columns still use [`Self::Text`].
    TextBlob(String),

    Integer(i64),

    /// Firebird 4+ `INT128` value.
    Int128(i128),

    Floating(f64),

    /// Fixed-point NUMERIC/DECIMAL without a floating-point conversion.
    Decimal(FixedDecimal),

    Timestamp(chrono::NaiveDateTime),

    Binary(Vec<u8>),

    /// Bytes in a fixed or varying character string with charset OCTETS.
    /// This is distinct from a binary BLOB.
    Octets(Vec<u8>),

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
