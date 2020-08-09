//!
//! Rust Firebird Client
//!
//! Connection functions
//!

#[cfg(feature = "pool")]
pub mod pool;
pub mod stmt_cache;

use rsfbclient_core::{Dialect, FbError, FirebirdClient, FromRow, IntoParams};
use std::{cell::RefCell, marker};

use crate::{query::Queryable, statement::StatementData, Execute, Transaction};
use stmt_cache::{StmtCache, StmtCacheData};

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
    #[cfg(all(feature = "native", feature = "dynamic_loading"))]
    fbclient_path: String
}

#[cfg(all(feature = "native", not(feature = "dynamic_loading")))]
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
        }
    }
}

impl ConnectionBuilder {
    #[cfg(all(feature = "native", feature = "dynamic_loading"))]
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
    pub fn with_client<S: Into<String>>(fbclient: S) -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            stmt_cache_size: 20,
            fbclient_path: fbclient.into()
        }
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

    #[cfg(all(feature = "native", not(feature = "dynamic_loading")))]
    /// Open a new connection to the database
    pub fn connect(&self) -> Result<Connection<rsfbclient_native::NativeFbClient>, FbError> {
        Connection::open(
            self,
            rsfbclient_native::NativeFbClient::new(self.host.clone(), self.port),
        )
    }

    #[cfg(all(feature = "native", feature = "dynamic_loading"))]
    /// Open a new connection to the database
    pub fn connect(&self) -> Result<Connection<rsfbclient_native::NativeFbClient>, FbError> {
        Connection::open(
            self,
            rsfbclient_native::NativeFbClient::new(self.host.clone(), self.port, &self.fbclient_path)?,
        )
    }
}

/// A connection to a firebird database
pub struct Connection<C>
where
    C: FirebirdClient,
{
    /// Database handler
    pub(crate) handle: C::DbHandle,

    /// Firebird dialect for the statements
    pub(crate) dialect: Dialect,

    /// Cache for the prepared statements
    pub(crate) stmt_cache: RefCell<StmtCache<StatementData<C::StmtHandle>>>,

    /// Firebird client
    pub(crate) cli: RefCell<C>,
}

impl<C> Connection<C>
where
    C: FirebirdClient,
{
    /// Open a new connection to the remote database
    fn open(builder: &ConnectionBuilder, mut cli: C) -> Result<Connection<C>, FbError> {
        let handle = cli.attach_database(&builder.db_name, &builder.user, &builder.pass)?;

        let stmt_cache = RefCell::new(StmtCache::new(builder.stmt_cache_size));

        Ok(Connection {
            handle,
            dialect: builder.dialect,
            stmt_cache,
            cli: RefCell::new(cli),
        })
    }

    /// Drop the current database
    pub fn drop_database(mut self) -> Result<(), FbError> {
        self.cli.get_mut().drop_database(self.handle)?;

        Ok(())
    }

    /// Run a closure with a transaction, if the closure returns an error
    /// the transaction will rollback, else it will be committed
    pub fn with_transaction<T>(
        &self,
        closure: impl FnOnce(&mut Transaction<C>) -> Result<T, FbError>,
    ) -> Result<T, FbError> {
        let mut tr = Transaction::new(self)?;

        let res = closure(&mut tr);

        if res.is_ok() {
            tr.commit_retaining()?;
        } else {
            tr.rollback_retaining()?;
        };

        res
    }

    /// Close the current connection
    pub fn close(mut self) -> Result<(), FbError> {
        self.__close()
    }

    /// Close the current connection. With an `&mut self` to be used in the drop code too
    fn __close(&mut self) -> Result<(), FbError> {
        self.stmt_cache.borrow_mut().close_all(self);

        self.cli.get_mut().detach_database(self.handle)?;

        Ok(())
    }
}

impl<C> Drop for Connection<C>
where
    C: FirebirdClient,
{
    fn drop(&mut self) {
        self.__close().ok();
    }
}

/// Variant of the `StatementIter` that owns the `Transaction` and uses the statement cache
pub struct StmtIter<'a, R, C: FirebirdClient> {
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData<StatementData<C::StmtHandle>>>,

    /// Transaction needs to be alive for the fetch to work
    tr: Transaction<'a, C>,

    _marker: marker::PhantomData<R>,
}

impl<R, C> Drop for StmtIter<'_, R, C>
where
    C: FirebirdClient,
{
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
            .insert_and_close(self.tr.conn, self.stmt_cache_data.take().unwrap())
            .ok();

        // Commit the transaction
        self.tr.commit_retaining().ok();
    }
}

impl<R, C> Iterator for StmtIter<'_, R, C>
where
    R: FromRow,
    C: FirebirdClient,
{
    type Item = Result<R, FbError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.stmt_cache_data
            .as_mut()
            .unwrap()
            .stmt
            .fetch(&self.tr.conn)
            .and_then(|row| row.map(FromRow::try_from).transpose())
            .transpose()
    }
}

impl<'a, R, C> Queryable<'a, R> for Connection<C>
where
    R: FromRow + 'a,
    C: FirebirdClient + 'a,
{
    type Iter = StmtIter<'a, R, C>;

    /// Prepare, execute, return the rows and commit the sql query
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_iter<P>(&'a mut self, sql: &str, params: P) -> Result<Self::Iter, FbError>
    where
        P: IntoParams,
    {
        let mut tr = Transaction::new(self)?;

        // Get a statement from the cache
        let mut stmt_cache_data =
            self.stmt_cache
                .borrow_mut()
                .get_or_prepare(self, &mut tr.data, sql)?;

        match stmt_cache_data.stmt.query(self, &mut tr.data, params) {
            Ok(_) => {
                let iter = StmtIter {
                    stmt_cache_data: Some(stmt_cache_data),
                    tr,
                    _marker: Default::default(),
                };

                Ok(iter)
            }
            Err(e) => {
                // Return the statement to the cache
                self.stmt_cache
                    .borrow_mut()
                    .insert_and_close(self, stmt_cache_data)?;

                Err(e)
            }
        }
    }
}

impl<C> Execute for Connection<C>
where
    C: FirebirdClient,
{
    /// Prepare, execute and commit the sql query
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams,
    {
        let mut tr = Transaction::new(self)?;

        // Get a statement from the cache
        let mut stmt_cache_data =
            self.stmt_cache
                .borrow_mut()
                .get_or_prepare(self, &mut tr.data, sql)?;

        // Do not return now in case of error, because we need to return the statement to the cache
        let res = stmt_cache_data.stmt.execute(self, &mut tr.data, params);

        // Return the statement to the cache
        self.stmt_cache
            .borrow_mut()
            .insert_and_close(self, stmt_cache_data)?;

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

    fn setup() -> Connection<rsfbclient_native::NativeFbClient> {
        #[cfg(not(feature = "dynamic_loading"))]
        let conn = ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn = ConnectionBuilder::with_client("./fbclient.lib")
            .connect()
            .expect("Error on connect the test database");

        conn
    }
}
