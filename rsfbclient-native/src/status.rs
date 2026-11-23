//! Status of connetions, transactions...

pub use rsfbclient_core::FbError;
use std::{
    ffi::CStr,
    fmt::Write,
    ops::{Deref, DerefMut},
};

use crate::ibase::{self, IBase};
use rsfbclient_core::{ExternalPsqlContext, SqlDiagnostics};

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

    pub fn as_error<T: IBase>(&self, ibase: &T) -> FbError {
        let mut buffer = vec![0u8; 1024];
        let mut msg = String::new();
        let mut diagnostics = SqlDiagnostics::default();
        let mut state_vector = *self.0;
        let mut warning = false;
        let base = self.0.as_ptr();
        let mut ptr = base;
        loop {
            let entry = ptr;
            // fb_interpret returns a position in this same status vector.
            let offset = unsafe { entry.offset_from(base) } as usize;
            let Some(words) = self.0.get(offset..) else {
                break;
            };
            if words.is_empty() {
                break;
            }
            if words[0] == ibase::isc_arg_warning as isize && !warning {
                // An explicit SQLSTATE in a warning cannot describe this error.
                state_vector[offset] = ibase::isc_arg_end as isize;
                warning = true;
            }
            let code = match words {
                [tag, code, ..] if !warning && *tag == ibase::isc_arg_gds as isize => *code as u32,
                _ => 0,
            };
            if code != 0 {
                diagnostics.gds_codes.push(code);
            }
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
            let formatted = String::from_utf8_lossy(&buffer[..len]);
            let start = msg.len();
            if code == ibase::isc_stack_trace {
                diagnostics
                    .psql_contexts
                    .push(start..start + formatted.len());
            } else if matches!(code, ibase::isc_eds_statement | ibase::isc_eds_connection) {
                let end = unsafe { ptr.offset_from(base) } as usize;
                if let Some(arguments) = self
                    .0
                    .get(offset + 2..end)
                    .and_then(|words| unsafe { external_arguments(words) })
                {
                    if let Some(context) =
                        ExternalPsqlContext::from_status_entry(code, &formatted, &arguments, start)
                    {
                        diagnostics.external_psql_contexts.push(context);
                    }
                }
            }
            writeln!(&mut msg, "{formatted}").unwrap();
        }
        // Remove the last \n
        msg.pop();

        let mut state = [0; 6];
        // fb_sqlstate writes five characters and the trailing NUL.
        unsafe { ibase.fb_sqlstate()(state.as_mut_ptr(), state_vector.as_ptr()) };
        diagnostics.sql_state = CStr::from_bytes_until_nul(&state.map(|byte| byte as u8))
            .ok()
            .and_then(|state| state.to_str().ok())
            .filter(|state| state.len() == 5 && *state != "00000")
            .map(str::to_owned);

        FbError::Sql {
            code: self.sql_code(ibase),
            msg,
            diagnostics: Box::new(diagnostics),
        }
    }
}

// String pointers are owned by Firebird and valid while the status is live.
unsafe fn external_arguments(mut words: &[isize]) -> Option<Vec<String>> {
    let mut arguments = Vec::new();
    while !words.is_empty() {
        let (value, tail) = match words {
            [tag, address, tail @ ..]
                if *tag == ibase::isc_arg_string as isize && *address != 0 =>
            {
                let value = unsafe { CStr::from_ptr(*address as *const _) }
                    .to_str()
                    .ok()?;
                (value, tail)
            }
            [tag, len, address, tail @ ..]
                if *tag == ibase::isc_arg_cstring as isize && *address != 0 =>
            {
                let bytes = unsafe {
                    std::slice::from_raw_parts(*address as *const u8, usize::try_from(*len).ok()?)
                };
                (std::str::from_utf8(bytes).ok()?, tail)
            }
            _ => return None,
        };
        arguments.push(value.to_owned());
        words = tail;
    }
    Some(arguments)
}
