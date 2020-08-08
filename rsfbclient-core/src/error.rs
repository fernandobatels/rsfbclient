//! Error type for the connection

use std::fmt::Display;

use crate::ColumnType;

#[derive(Debug)]
pub struct FbError {
    pub msg: String,
    pub code: i32,
}

impl Display for FbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.msg)
    }
}

impl std::error::Error for FbError {}

pub fn err_column_null(type_name: &str) -> FbError {
    FbError {
        code: -1,
        msg: format!(
            "This is a null value. Use the Option<{}> to safe access this column and avoid errors",
            type_name
        ),
    }
}

pub fn err_type_conv<T>(from: ColumnType, to: &str) -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: format!("Can't convert {:?} column to {}", from, to),
    })
}
