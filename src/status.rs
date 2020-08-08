//!
//! Rust Firebird Client
//!
//! Status of connetions, transactions...
//!

use rsfbclient_core::FbError;

pub fn err_idx_not_exist<T>() -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: "This index doesn't exists".to_string(),
    })
}

pub fn err_column_null<T>(type_name: &str) -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: format!(
            "This is a null value. Use the Option<{}> to safe access this column and avoid errors",
            type_name
        ),
    })
}

pub fn err_type_conv<T>(from: SqlType, to: &str) -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: format!("Can't convert {:?} column to {}", from, to),
    })
}

pub fn err_buffer_len<T>(expected: usize, found: usize, type_name: &str) -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: format!(
            "Invalid buffer size for type {:?} (expected: {}, found: {})",
            type_name, expected, found
        ),
    })
}
