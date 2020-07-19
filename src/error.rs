//!
//! Rust Firebird Client
//!
//! Error status
//!

use std::ffi::CStr;
use std::mem;
use std::os::raw::c_char;
use std::os::raw::c_void;

use super::ibase;

#[derive(Debug)]
pub struct FbError {
    pub msg: String,
    pub code: i32,
}

impl FbError {
    pub unsafe fn from_status(mut status: *mut ibase::ISC_STATUS_ARRAY) -> FbError {
        let code = ibase::isc_sqlcode(status);
        let mut msg = String::new();
        let c_msg: *mut c_char = libc::malloc(1024 * mem::size_of::<c_char>()) as *mut c_char;

        while ibase::fb_interpret(c_msg, 1024, &mut status) != 0 {
            let s_str = CStr::from_ptr(c_msg)
                .to_str()
                .expect("Error on decode the error message")
                .to_string();

            msg.push_str(&s_str);
            msg.push('\n');
        }

        libc::free(c_msg as *mut c_void);

        FbError {
            code: code,
            msg: msg,
        }
    }
}
