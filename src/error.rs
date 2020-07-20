//!
//! Rust Firebird Client
//!
//! Error status
//!

use super::ibase;

#[derive(Debug)]
pub struct FbError {
    pub msg: String,
    pub code: i32,
}

pub struct Status(Box<ibase::ISC_STATUS_ARRAY>);

impl Default for Status {
    fn default() -> Self {
        Status(Box::new([0; 20]))
    }
}

impl Status {
    pub fn sql_code(&self) -> i32 {
        unsafe { ibase::isc_sqlcode(self.0.as_ptr()) }
    }

    pub fn message(&self) -> String {
        let mut buffer: Vec<u8> = Vec::with_capacity(256);

        unsafe {
            let len = ibase::fb_interpret(
                buffer.as_mut_ptr() as _,
                buffer.capacity() as _,
                &mut self.0.as_ptr(),
            );
            buffer.set_len(len as usize);
        }

        String::from_utf8(buffer).unwrap_or_else(|_| "Invalid error message".into())
    }

    pub fn as_error(&self) -> FbError {
        FbError {
            code: self.sql_code(),
            msg: self.message(),
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut isize {
        self.0.as_mut_ptr()
    }
}
