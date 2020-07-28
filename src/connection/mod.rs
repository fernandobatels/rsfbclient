//!
//! Rust Firebird Client
//!
//! Connection functions
//!

pub mod stmt_cache;

use std::{
    cell::{Cell, RefCell},
    marker,
};

use crate::{
    ibase,
    params::IntoParams,
    query::Queryable,
    row::{ColumnBuffer, FromRow},
    status::{FbError, Status},
    Transaction,
};
use stmt_cache::{StmtCache, StmtCacheData};

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum Dialect {
    D1 = 1,
    D2 = 2,
    D3 = 3,
}

#[derive(Debug, Clone)]
/// Builder for creating database connections
pub struct ConnectionBuilder {
    host: String,
    port: u16,
    db_name: String,
    user: String,
    pass: String,
    dialect: Dialect,
    stmt_cache_size: usize,
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
            stmt_cache_size: 20,
            ibase: ibase::IBase,
        }
    }
}

impl ConnectionBuilder {
    #[cfg(feature = "dynamic_loading")]
    /// Searches for the firebird client at runtime, in the specified path.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rsfbclient::ConnectionBuilder;
    ///
    /// // On windows
    /// ConnectionBuilder::with_client("fbclient.dll");
    ///
    /// // On linux
    /// ConnectionBuilder::with_client("libfbclient.so");
    ///
    /// // Any platform, file located relative to the
    /// // folder where the executable was run
    /// ConnectionBuilder::with_client("./fbclient.lib");
    /// ```
    pub fn with_client(fbclient: &str) -> Result<Self, FbError> {
        Ok(Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            stmt_cache_size: 20,
            ibase: ibase::IBase::new(fbclient).map_err(|e| FbError {
                code: -1,
                msg: e.to_string(),
            })?,
        })
    }

    /// Hostname or IP address of the server. Default: localhost
    pub fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
        self.host = host.into();
        self
    }

    /// TCP Port of the server. Default: 3050
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = port;
        self
    }

    /// Database name or path. Default: test.fdb
    pub fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
        self.db_name = db_name.into();
        self
    }

    /// Username. Default: SYSDBA
    pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
        self.user = user.into();
        self
    }

    /// Password. Default: masterkey
    pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
        self.pass = pass.into();
        self
    }

    /// SQL Dialect. Default: 3
    pub fn dialect(&mut self, dialect: Dialect) -> &mut Self {
        self.dialect = dialect;
        self
    }

    /// Statement cache size. Default: 20
    pub fn stmt_cache_size(&mut self, stmt_cache_size: usize) -> &mut Self {
        self.stmt_cache_size = stmt_cache_size;
        self
    }

    /// Open a new connection to the database
    pub fn connect(&self) -> Result<Connection, FbError> {
        Connection::open(self)
    }
}

/// A connection to a firebird database
pub struct Connection {
    /// Database handler
    pub(crate) handle: Cell<ibase::isc_db_handle>,

    /// Status for the client calls
    pub(crate) status: RefCell<Status>,

    /// Firebird dialect for the statements
    pub(crate) dialect: Dialect,

    /// Cache for the prepared statements
    pub(crate) stmt_cache: RefCell<StmtCache>,

    /// Firebird client functions
    pub(crate) ibase: ibase::IBase,
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

        let stmt_cache = RefCell::new(StmtCache::new(builder.stmt_cache_size));

        Ok(Connection {
            handle,
            status,
            dialect: builder.dialect,
            stmt_cache,
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

    /// Run a closure with a transaction, if the closure returns an error
    /// the transaction will rollback, else it will be committed
    pub fn with_transaction<T>(
        &self,
        closure: impl FnOnce(&mut Transaction) -> Result<T, FbError>,
    ) -> Result<T, FbError> {
        let mut tr = Transaction::start_transaction(self)?;

        let res = closure(&mut tr);

        if res.is_ok() {
            tr.commit_retaining()?;
        } else {
            tr.rollback_retaining()?;
        };

        res
    }

    /// Starts a new transaction
    pub fn transaction(&self) -> Result<Transaction, FbError> {
        Transaction::start_transaction(self)
    }

    /// Close the current connection
    pub fn close(mut self) -> Result<(), FbError> {
        self.__close()
    }

    /// Close the current connection. With an `&mut self` to be used in the drop code too
    fn __close(&mut self) -> Result<(), FbError> {
        self.stmt_cache.borrow_mut().close_all(self);

        unsafe {
            // Close the connection, if the handle is valid
            if self.handle.get() != 0
                && self.ibase.isc_detach_database()(
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
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.__close().ok();
    }
}

/// Variant of the `StatementIter` that owns the `Transaction` and uses the statement cache
struct StmtIter<'a, R> {
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData>,

    /// Buffers for the column data
    buffers: Vec<ColumnBuffer>,

    /// Transaction needs to be alive for the fetch to work
    tr: Transaction<'a>,

    _marker: marker::PhantomData<R>,
}

impl<R> Drop for StmtIter<'_, R> {
    fn drop(&mut self) {
        // Close the cursor
        self.stmt_cache_data
            .as_mut()
            .unwrap()
            .stmt
            .close_cursor(self.tr.conn)
            .ok();

        // Send the statement back to the cache
        self.tr
            .conn
            .stmt_cache
            .borrow_mut()
            .insert(self.tr.conn, self.stmt_cache_data.take().unwrap())
            .ok();

        // Commit the transaction
        self.tr.commit_retaining().ok();
    }
}

impl<R> Iterator for StmtIter<'_, R>
where
    R: FromRow,
{
    type Item = Result<R, FbError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.stmt_cache_data
            .as_mut()
            .unwrap()
            .stmt
            .fetch(&self.tr.conn, &mut self.buffers)
            .and_then(|row| row.map(|row| row.get_all()).transpose())
            .transpose()
    }
}

impl Queryable for Connection {
    /// Prepare, execute, return the rows and commit the sql query
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'a,
    {
        let mut tr = self.transaction()?;

        // Get a statement from the cache
        let mut stmt_cache_data = self.stmt_cache.borrow_mut().get(self, &mut tr.data, sql)?;

        match stmt_cache_data.stmt.query(self, &mut tr.data, params) {
            Ok(buffers) => {
                let iter = StmtIter {
                    stmt_cache_data: Some(stmt_cache_data),
                    buffers,
                    tr,
                    _marker: Default::default(),
                };

                Ok(Box::new(iter))
            }
            Err(e) => {
                // Return the statement to the cache
                self.stmt_cache.borrow_mut().insert(self, stmt_cache_data)?;

                Err(e)
            }
        }
    }

    /// Prepare, execute and commit the sql query
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: crate::params::IntoParams,
    {
        let mut tr = self.transaction()?;

        // Get a statement from the cache
        let mut stmt_cache_data = self.stmt_cache.borrow_mut().get(self, &mut tr.data, sql)?;

        // Do not return now in case of error, because we need to return the statement to the cache
        let res = stmt_cache_data.stmt.execute(self, &mut tr.data, params);

        // Return the statement to the cache
        self.stmt_cache.borrow_mut().insert(self, stmt_cache_data)?;

        res?;

        tr.commit()?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::*;

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

    #[test]
    fn query_iter() {
        let mut conn = setup();

        let mut rows = 0;

        for row in conn
            .query_iter("SELECT -3 FROM RDB$DATABASE WHERE 1 = ?", (1,))
            .expect("Error on the query")
        {
            let (v,): (i32,) = row.expect("");

            assert_eq!(v, -3);

            rows += 1;
        }

        assert_eq!(rows, 1);
    }

    fn setup() -> Connection {
        #[cfg(not(feature = "dynamic_loading"))]
        let conn = ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn = ConnectionBuilder::with_client("./fbclient.lib")
            .expect("Error finding fbclient lib")
            .connect()
            .expect("Error on connect the test database");

        conn
    }
}
