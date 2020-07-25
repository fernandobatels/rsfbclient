//!
//! Rust Firebird Client
//!
//! Connection functions
//!

use std::cell::{Cell, RefCell};

use crate::{
    ibase,
    status::{FbError, Status},
    Transaction,
};

pub struct Connection {
    pub(crate) handle: Cell<ibase::isc_db_handle>,
    pub(crate) status: RefCell<Status>,
    pub(crate) dialect: Dialect,
    pub(crate) ibase: ibase::IBase,
}

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum Dialect {
    D1 = 1,
    D2 = 2,
    D3 = 3,
}

#[derive(Debug, Clone)]
pub struct ConnectionBuilder {
    host: String,
    port: u16,
    db_name: String,
    user: String,
    pass: String,
    dialect: Dialect,
    ibase: ibase::IBase,
}

#[cfg(not(feature = "dynamic_loading"))]
impl Default for ConnectionBuilder {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            ibase: ibase::IBase,
        }
    }
}

impl ConnectionBuilder {
    #[cfg(feature = "dynamic_loading")]
    pub fn with_client(fbclient: &str) -> Result<Self, FbError> {
        Ok(Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            ibase: ibase::IBase::new(fbclient).map_err(|e| FbError {
                code: -1,
                msg: e.to_string(),
            })?,
        })
    }

    pub fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
        self.host = host.into();
        self
    }

    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = port;
        self
    }

    pub fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
        self.db_name = db_name.into();
        self
    }

    pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
        self.user = user.into();
        self
    }

    pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
        self.pass = pass.into();
        self
    }

    pub fn dialect(&mut self, dialect: Dialect) -> &mut Self {
        self.dialect = dialect;
        self
    }

    pub fn connect(&self) -> Result<Connection, FbError> {
        Connection::open(self)
    }
}

impl Connection {
    /// Open a new connection to the remote database
    fn open(builder: &ConnectionBuilder) -> Result<Connection, FbError> {
        let ibase = builder.ibase.clone();

        let handle = Cell::new(0);
        let status: RefCell<Status> = Default::default();

        let dpb = {
            let mut dpb: Vec<u8> = Vec::with_capacity(64);

            dpb.extend(&[ibase::isc_dpb_version1 as u8]);

            dpb.extend(&[ibase::isc_dpb_user_name as u8, builder.user.len() as u8]);
            dpb.extend(builder.user.bytes());

            dpb.extend(&[ibase::isc_dpb_password as u8, builder.pass.len() as u8]);
            dpb.extend(builder.pass.bytes());

            // Makes the database convert the strings to utf-8, allowing non ascii characters
            let charset = b"UTF-8";

            dpb.extend(&[ibase::isc_dpb_lc_ctype as u8, charset.len() as u8]);
            dpb.extend(charset);

            dpb
        };

        let conn_string = format!("{}/{}:{}", builder.host, builder.port, builder.db_name);

        unsafe {
            if ibase.isc_attach_database()(
                status.borrow_mut().as_mut_ptr(),
                conn_string.len() as i16,
                conn_string.as_ptr() as *const _,
                handle.as_ptr(),
                dpb.len() as i16,
                dpb.as_ptr() as *const _,
            ) != 0
            {
                return Err(status.borrow().as_error(&ibase));
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle.get(), 0);

        Ok(Connection {
            handle,
            status,
            dialect: builder.dialect,
            ibase,
        })
    }

    /// Drop the current database
    pub fn drop_database(self) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_drop_database()(
                self.status.borrow_mut().as_mut_ptr(),
                self.handle.as_ptr(),
            ) != 0
            {
                return Err(self.status.borrow().as_error(&self.ibase));
            }
        }

        Ok(())
    }

    /// Close the current connection
    pub fn close(self) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_detach_database()(
                self.status.borrow_mut().as_mut_ptr(),
                self.handle.as_ptr(),
            ) != 0
            {
                return Err(self.status.borrow().as_error(&self.ibase));
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
                self.ibase.isc_detach_database()(
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
    fn remote_connection() {
        #[cfg(not(feature = "dynamic_loading"))]
        let conn = ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn = ConnectionBuilder::with_client("./fbclient.lib")
            .expect("Error finding fbclient lib")
            .connect()
            .expect("Error on connect the test database");

        conn.close().expect("error closing the connection");
    }
}
