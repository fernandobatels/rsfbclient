///
/// Rust Firebird Client 
///
/// Transaction functions 
///

use std::result::Result;
use std::os::raw::c_void;
use std::mem;
use std::ptr;
use std::cell::RefCell;
use std::ffi::CString;

use super::ibase;
use super::error::FbError;
use super::connection::Connection;

pub struct Transaction<'a> {
    handle: RefCell<ibase::isc_tr_handle>,
    conn: &'a mut Connection
}

impl Transaction<'_> {

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
            handle: handle,
            conn: conn
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

    pub fn test(&self) -> Result<(), FbError> {

        unsafe {
            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            
            let handle_ptr = self.handle.as_ptr(); 
            let conn_handle_ptr = self.conn.handle.as_ptr();

            let sql = "insert into cross_rate (from_currency, to_currency, conv_rate) values ('Dollar', 'Real', 10)".to_string();
            let c_sql = match CString::new(sql.clone()) {
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
