//!
//! Rust Firebird Client
//!
//! Status of connetions, transactions...
//!

use thiserror::Error;

use crate::row::ColumnType;

#[derive(Debug, Error)]
pub enum FbError {
    #[error("sql error {}: {}", .0.code, .0.msg)]
    Sql(SqlError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("error: {0}")]
    Other(String),
}

impl From<String> for FbError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

impl From<&str> for FbError {
    fn from(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

#[derive(Debug)]
pub struct SqlError {
    pub msg: String,
    pub code: i32,
}

pub fn err_idx_not_exist<T>() -> Result<T, FbError> {
    Err(FbError::Other("This index doesn't exists".to_string()))
}

pub fn err_column_null(type_name: &str) -> FbError {
    format!(
        "This is a null value. Use the Option<{}> to safe access this column and avoid errors",
        type_name
    )
    .into()
}

pub fn err_type_conv<T>(from: ColumnType, to: &str) -> Result<T, FbError> {
    Err(FbError::Other(format!(
        "Can't convert {:?} column to {}",
        from, to
    )))
}

pub fn err_buffer_len<T>(expected: usize, found: usize, type_name: &str) -> Result<T, FbError> {
    Err(FbError::Other(format!(
        "Invalid buffer size for type {:?} (expected: {}, found: {})",
        type_name, expected, found
    )))
}
