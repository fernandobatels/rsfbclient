//! Error type for the connection

use thiserror::Error;

use crate::ColumnType;

#[derive(Debug, Error)]
pub enum FbError {
    #[error("sql error {code}: {msg}")]
    Sql { msg: String, code: i32 },

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

pub fn err_column_null(type_name: &str) -> FbError {
    FbError::Other(format!(
        "This is a null value. Use the Option<{}> to safe access this column and avoid errors",
        type_name
    ))
}

pub fn err_type_conv<T>(from: ColumnType, to: &str) -> Result<T, FbError> {
    Err(FbError::Other(format!(
        "Can't convert {:?} column to {}",
        from, to
    )))
}
