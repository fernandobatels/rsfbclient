//
// Rust Firebird Client
//
// Transaction functions
//

use std::cell::Cell;
use std::mem;
use std::os::raw::c_void;
use std::result::Result;

use super::connection::Connection;
use super::error::FbError;
use super::ibase;
use super::statement::Statement;

pub struct Transaction<'a> {
    pub handle: Cell<ibase::isc_tr_handle>,
    pub conn: &'a Connection,
}

impl<'a> Transaction<'a> {
    /// Start a new transaction
    pub fn start_transaction(conn: &Connection) -> Result<Transaction, FbError> {
        let handle = Cell::new(0 as u32);

        unsafe {
            let handle_ptr = handle.as_ptr();
            let conn_handle_ptr = conn.handle.as_ptr();

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_start_transaction(status, handle_ptr, 1, conn_handle_ptr, 0) != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(Transaction {
            handle: handle,
            conn: conn,
        })
    }

    /// Commit the current transaction changes
    pub fn commit(self) -> Result<(), FbError> {
        Statement::execute_immediate(&self, "commit;".to_string())
    }

    /// Rollback the current transaction changes
    pub fn rollback(self) -> Result<(), FbError> {
        Statement::execute_immediate(&self, "rollback;".to_string())
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(&self, sql: String) -> Result<(), FbError> {
        Statement::execute_immediate(self, sql)
    }

    /// Prepare a new statement for execute
    pub fn prepare(&self, sql: String) -> Result<Statement, FbError> {
        Statement::prepare(self, sql)
    }
}
