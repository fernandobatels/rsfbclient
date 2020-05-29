///
/// Rust Firebird Client 
///
/// Transaction functions 
///

use std::result::Result;
use std::os::raw::c_void;
use std::mem;
use std::cell::RefCell;

use super::ibase;
use super::error::FbError;
use super::connection::Connection;

pub struct Transaction {
    handle: RefCell<ibase::isc_tr_handle>
}

impl Transaction {

    /// Start a new transaction
    pub fn start_transaction(conn: &mut Connection) -> Result<Transaction, FbError> {

        let handle = RefCell::new(0 as u32);

        unsafe {
            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            
            let handle_ptr = handle.as_ptr(); 
            let conn_handle_ptr = conn.handle.as_ptr();
            if ibase::isc_start_transaction(status, handle_ptr, 1, conn_handle_ptr, 0) != 0 {
                return Err(FbError::from_status(status)); 
            }
            
            libc::free(status as *mut c_void);
        }

        Ok(Transaction {
            handle: handle
        })
    }

    /// Commit the current transaction changes
    pub fn commit(self) -> Result<(), FbError> {

        unsafe {
        
            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            
            let handle_ptr = self.handle.as_ptr(); 
            if ibase::isc_commit_transaction(status, handle_ptr) != 0 {
                return Err(FbError::from_status(status)); 
            }
            
            libc::free(status as *mut c_void);
        
        }
        
        Ok(())
    }
}
