///
/// Rust Firebird Client 
///
/// Preparation and execution of statements 
///

use std::result::Result;
use std::os::raw::c_void;
use std::mem;
use std::ptr;
use std::cell::RefCell;
use std::ffi::CString;

use super::ibase;
use super::error::FbError;
use super::transaction::Transaction;

pub struct Statement<'a> {
    tr: &'a Transaction<'a>
}

impl<'a> Statement<'a> {

    /// Prepare the statement that will be executed
    pub fn prepare(sql: String) -> Result<Statement<'a>, FbError> {
        unimplemented!();
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(tr: &Transaction, sql: String) -> Result<(), FbError> {

        unsafe {
            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            
            let handle_ptr = tr.handle.as_ptr(); 
            let conn_handle_ptr = tr.conn.handle.as_ptr();

            let c_sql = match CString::new(sql) {
                Ok(c) => c.into_raw(),
                Err(e) => return Err(FbError { code: -1, msg: e.to_string() })
            };

            if ibase::isc_dsql_execute_immediate(status, conn_handle_ptr, handle_ptr, 0, c_sql, 1, ptr::null()) != 0 {
                return Err(FbError::from_status(status)); 
            }
            
            libc::free(status as *mut c_void);
        }
    
        Ok(())
    }
}
