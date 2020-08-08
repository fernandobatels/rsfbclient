//!
//! Rust Firebird Client
//!
//! Status of connetions, transactions...
//!

use std::fmt::Write;

use crate::{ibase, row::SqlType};

pub use rsfbclient_core::FbError;

pub struct Status(Box<ibase::ISC_STATUS_ARRAY>);

impl Default for Status {
    fn default() -> Self {
        Status(Box::new([0; 20]))
    }
}

impl Status {
    pub fn sql_code(&self, ibase: &ibase::IBase) -> i32 {
        unsafe { ibase.isc_sqlcode()(self.0.as_ptr()) }
    }

    pub fn message(&self, ibase: &ibase::IBase) -> String {
        let mut buffer: Vec<u8> = Vec::with_capacity(256);
        let mut msg = String::new();

        let mut ptr = self.0.as_ptr();

        loop {
            unsafe {
                let len = ibase.fb_interpret()(
                    buffer.as_mut_ptr() as *mut _,
                    buffer.capacity() as u32,
                    &mut ptr,
                );
                buffer.set_len(len as usize);
            }

            if buffer.is_empty() {
                break;
            }

            writeln!(
                &mut msg,
                "{}",
                std::str::from_utf8(&buffer).unwrap_or("Invalid error message")
            )
            .unwrap();
        }
        // Remove the last \n
        msg.pop();

        msg
    }

    pub fn as_error(&self, ibase: &ibase::IBase) -> FbError {
        FbError {
            code: self.sql_code(ibase),
            msg: self.message(ibase),
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut isize {
        self.0.as_mut_ptr()
    }
}

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
