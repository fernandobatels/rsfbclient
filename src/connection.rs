//
// Rust Firebird Client
//
// Connection functions
//

use std::cell::Cell;
use std::ffi::CString;
use std::mem;
use std::os::raw::c_void;
use std::ptr;
use std::result::Result;

use super::error::FbError;
use super::ibase;
use super::transaction::Transaction;

pub struct Connection {
    pub handle: Cell<ibase::isc_db_handle>,
}

impl Connection {
    /// Open a new connection to the remote database
    pub fn open(
        host: String,
        port: u16,
        db_name: String,
        user: String,
        pass: String,
    ) -> Result<Connection, FbError> {
        let handle = Cell::new(0 as u32);

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

        let mut status: ibase::ISC_STATUS_ARRAY = [0; 20];

        unsafe {
            if ibase::isc_attach_database(
                &mut status,
                conn_string.len() as i16,
                conn_string.as_ptr() as *const i8,
                handle.as_ptr(),
                dpb.len() as i16,
                dpb.as_ptr() as *const i8,
            ) != 0
            {
                return Err(FbError::from_status(status.as_mut_ptr() as _));
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle.get(), 0);

        Ok(Connection { handle })
    }

    /// Open a new connection to the local database
    pub fn open_local(db_name: String) -> Result<Connection, FbError> {
        let handle = Cell::new(0 as u32);

        unsafe {
            let c_db_name = match CString::new(db_name) {
                Ok(c) => c.into_raw(),
                Err(e) => {
                    return Err(FbError {
                        code: -1,
                        msg: e.to_string(),
                    })
                }
            };

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            let handle_ptr = handle.as_ptr();
            if ibase::isc_attach_database(status, 0, c_db_name, handle_ptr, 0, ptr::null()) != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(Connection { handle })
    }

    /// Create a new local database
    pub fn create_local(db_name: String) -> Result<(), FbError> {
        let local = Connection {
            handle: Cell::new(0 as u32),
        };

        let local_tr = Transaction {
            handle: Cell::new(0 as u32),
            conn: &local,
        };

        let sql = format!("create database \"{}\"", db_name);

        if let Err(e) = local_tr.execute_immediate(sql) {
            return Err(e);
        }

        local.close()
    }

    /// Drop the current database
    pub fn drop_database(self) -> Result<(), FbError> {
        unsafe {
            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;

            let handle_ptr = self.handle.as_ptr();
            if ibase::isc_drop_database(status, handle_ptr) != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(())
    }

    // Drop the database, if exists, and create a new empty
    pub fn recreate_local(db_name: String) -> Result<(), FbError> {
        if let Ok(conn) = Self::open_local(db_name.clone()) {
            if let Err(e) = conn.drop_database() {
                return Err(e);
            }
        }

        Self::create_local(db_name)
    }

    /// Close the current connection
    pub fn close(self) -> Result<(), FbError> {
        unsafe {
            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;

            let handle_ptr = self.handle.as_ptr();
            if ibase::isc_detach_database(status, handle_ptr) != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(())
    }

    pub fn start_transaction(&self) -> Result<Transaction, FbError> {
        Transaction::start_transaction(self)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn local_connection() {
        Connection::recreate_local("test.fdb".to_string())
            .expect("Error on recreate the test database");

        let conn = Connection::open_local("test.fdb".to_string())
            .expect("Error on connect the test database");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn remote_connection() {
        let conn = Connection::open(
            "localhost".into(),
            3050,
            "test.fdb".into(),
            "SYSDBA".into(),
            "masterkey".into(),
        )
        .expect("Error connecting to the test database");

        conn.close().expect("error closing the connection");
    }
}
