//! Status of connetions, transactions...

pub use rsfbclient_core::FbError;
use std::{
    fmt::Write,
    ops::{Deref, DerefMut},
};

use crate::ibase::{self, IBase};

pub struct Status(Box<ibase::ISC_STATUS_ARRAY>);

impl Default for Status {
    fn default() -> Self {
        Status(Box::new([0; 20]))
    }
}

impl Deref for Status {
    type Target = Box<ibase::ISC_STATUS_ARRAY>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Status {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// Firebird status vector
impl Status {
    pub fn sql_code<T: IBase>(&self, ibase: &T) -> i32 {
        unsafe { ibase.isc_sqlcode()(self.0.as_ptr()) }
    }

    pub fn message<T: IBase>(&self, ibase: &T) -> String {
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

    pub fn as_error<T: IBase>(&self, ibase: &T) -> FbError {
        FbError::Sql {
            code: self.sql_code(ibase),
            msg: self.message(ibase),
        }
    }
}
