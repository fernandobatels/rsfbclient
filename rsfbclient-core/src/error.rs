//! Error type for the connection

use std::str::Utf8Error;
use std::string::FromUtf8Error;
use thiserror::Error;

use crate::SqlType;

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

impl From<FromUtf8Error> for FbError {
    fn from(e: FromUtf8Error) -> Self {
        Self::Other(format!("Found column with an invalid UTF-8 string: {}", e))
    }
}

impl From<Utf8Error> for FbError {
    fn from(e: Utf8Error) -> Self {
        Self::Other(format!("Found column with an invalid UTF-8 string: {}", e))
    }
}

pub fn err_column_null(type_name: &str) -> FbError {
    FbError::Other(format!(
        "This is a null value. Use the Option<{}> to safe access this column and avoid errors",
        type_name
    ))
}

pub fn err_type_conv<T>(from: SqlType, to: &str) -> Result<T, FbError> {
    Err(FbError::Other(format!(
        "Can't convert {:?} column to {}",
        from, to
    )))
}
