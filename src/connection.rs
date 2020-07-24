//!
//! Rust Firebird Client
//!
//! Connection functions
//!

use std::{
    cell::{Cell, RefCell},
    ptr,
};

use crate::{ibase, FbError, Status, Transaction};

pub struct Connection {
    pub(crate) handle: Cell<ibase::isc_db_handle>,
    pub(crate) status: RefCell<Status>,
}

impl Connection {
    /// Open a new connection to the remote database
    pub fn open(
        host: &str,
        port: u16,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<Connection, FbError> {
        let handle = Cell::new(0);
        let status: RefCell<Status> = Default::default();

        let dpb = {
            let mut dpb: Vec<u8> = Vec::with_capacity(64);

            dpb.extend(&[ibase::isc_dpb_version1 as u8]);

            dpb.extend(&[ibase::isc_dpb_user_name as u8, user.len() as u8]);
            dpb.extend(user.bytes());

            dpb.extend(&[ibase::isc_dpb_password as u8, pass.len() as u8]);
            dpb.extend(pass.bytes());

            // Makes the database convert the strings to utf-8, allowing non ascii characters
            let charset = b"UTF-8";

            dpb.extend(&[ibase::isc_dpb_lc_ctype as u8, charset.len() as u8]);
            dpb.extend(charset);

            dpb
        };

        let conn_string = format!("{}/{}:{}", host, port, db_name);

        unsafe {
            if ibase::isc_attach_database(
                status.borrow_mut().as_mut_ptr(),
                conn_string.len() as i16,
                conn_string.as_ptr() as *const _,
                handle.as_ptr(),
                dpb.len() as i16,
                dpb.as_ptr() as *const _,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle.get(), 0);

        Ok(Connection { handle, status })
    }

    /// Open a new connection to the local database
    pub fn open_local(db_name: &str) -> Result<Connection, FbError> {
        let handle = Cell::new(0);
        let status: RefCell<Status> = Default::default();

        unsafe {
            if ibase::isc_attach_database(
                status.borrow_mut().as_mut_ptr(),
                db_name.len() as i16,
                db_name.as_ptr() as *const _,
                handle.as_ptr(),
                0,
                ptr::null(),
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle.get(), 0);

        Ok(Connection { handle, status })
    }

    /// Create a new local database
    pub fn create_local(db_name: &str) -> Result<(), FbError> {
        let local = Connection {
            handle: Cell::new(0),
            status: Default::default(),
        };

        let local_tr = Transaction {
            handle: Cell::new(0),
            conn: &local,
        };

        // CREATE DATABASE does not work with parameters
        let sql = format!("create database \"{}\"", db_name);

        if let Err(e) = local_tr.execute_immediate(&sql, ()) {
            return Err(e);
        }

        drop(local_tr);
        local.close()
    }

    /// Drop the current database
    pub fn drop_database(self) -> Result<(), FbError> {
        unsafe {
            if ibase::isc_drop_database(self.status.borrow_mut().as_mut_ptr(), self.handle.as_ptr())
                != 0
            {
                return Err(self.status.borrow().as_error());
            }
        }

        Ok(())
    }

    // Drop the database, if exists, and create a new empty
    pub fn recreate_local(db_name: &str) -> Result<(), FbError> {
        if let Ok(conn) = Self::open_local(db_name) {
            if let Err(e) = conn.drop_database() {
                return Err(e);
            }
        }

        Self::create_local(db_name)
    }

    /// Close the current connection
    pub fn close(self) -> Result<(), FbError> {
        unsafe {
            if ibase::isc_detach_database(
                self.status.borrow_mut().as_mut_ptr(),
                self.handle.as_ptr(),
            ) != 0
            {
                return Err(self.status.borrow().as_error());
            }
        }

        // Assert that the handle is invalid
        debug_assert_eq!(self.handle.get(), 0);

        Ok(())
    }

    /// Run a closure with a transaction, if the closure returns an error
    /// the transaction will rollback, else it will be committed
    pub fn with_transaction<T>(
        &self,
        closure: impl FnOnce(&mut Transaction) -> Result<T, FbError>,
    ) -> Result<T, FbError> {
        let mut tr = Transaction::start_transaction(self)?;

        let res = closure(&mut tr);

        if res.is_ok() {
            tr.commit()?;
        }

        res
    }

    /// Starts a new transaction
    pub fn transaction(&self) -> Result<Transaction, FbError> {
        Transaction::start_transaction(self)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // Close the connection, if the handle is valid
        if self.handle.get() != 0 {
            unsafe {
                ibase::isc_detach_database(
                    self.status.borrow_mut().as_mut_ptr(),
                    self.handle.as_ptr(),
                );
            }
        }

        // Assert that the handle is invalid
        debug_assert_eq!(self.handle.get(), 0);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn local_connection() {
        Connection::recreate_local("test.fdb").expect("Error on recreate the test database");

        let conn = Connection::open_local("test.fdb").expect("Error on connect the test database");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn remote_connection() {
        let conn = Connection::open("localhost", 3050, "test.fdb", "SYSDBA", "masterkey")
            .expect("Error connecting to the test database");

        conn.close().expect("error closing the connection");
    }
}
