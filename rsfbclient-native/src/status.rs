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
        let mut buffer = vec![0u8; 1024];
        let mut msg = String::new();
        let mut ptr = self.0.as_ptr();
        loop {
            let entry = ptr;
            let len = loop {
                // fb_interpret advances the pointer even when the message is
                // clipped. Retry from the same entry with a larger buffer.
                ptr = entry;
                let len = unsafe {
                    ibase.fb_interpret()(
                        buffer.as_mut_ptr() as *mut _,
                        buffer.len() as u32,
                        &mut ptr,
                    )
                }
                .max(0) as usize;
                if len >= buffer.len() - 4 && buffer.len() < 16 * 1024 * 1024 {
                    buffer.resize(buffer.len() * 2, 0);
                    continue;
                }
                break len.min(buffer.len());
            };
            if len == 0 {
                if ptr == entry {
                    break;
                }
                continue;
            }
            writeln!(&mut msg, "{}", String::from_utf8_lossy(&buffer[..len])).unwrap();
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
