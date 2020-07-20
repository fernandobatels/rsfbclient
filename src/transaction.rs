//!
//! Rust Firebird Client
//!
//! Transaction functions
//!

use std::cell::Cell;

use super::connection::Connection;
use super::status::FbError;
use super::ibase;
use super::statement::Statement;

pub struct Transaction<'c> {
    pub(crate) handle: Cell<ibase::isc_tr_handle>,
    pub(crate) conn: &'c Connection,
}

impl<'c> Transaction<'c> {
    /// Start a new transaction
    pub fn start_transaction(conn: &Connection) -> Result<Transaction, FbError> {
        let handle = Cell::new(0);
        let status = &conn.status;

        unsafe {
            if ibase::isc_start_transaction(
                status.borrow_mut().as_mut_ptr(),
                handle.as_ptr(),
                1,
                conn.handle.as_ptr(),
                0,
                0,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle.get(), 0);

        Ok(Transaction { handle, conn })
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

impl<'c> Drop for Transaction<'c> {
    fn drop(&mut self) {
        // Rollback the transaction, if the handle is valid
        if self.handle.get() != 0 {
            unsafe {
                ibase::isc_rollback_transaction(
                    self.conn.status.borrow_mut().as_mut_ptr(),
                    self.handle.as_ptr(),
                );
            }
        }

        // Assert that the handle is invalid
        debug_assert_eq!(self.handle.get(), 0);
    }
}
