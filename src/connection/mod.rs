//!
//! Rust Firebird Client
//!
//! Connection functions
//!

#[cfg(feature = "pool")]
pub mod pool;
pub mod stmt_cache;

use rsfbclient_core::{
    Dialect, FbError, FirebirdClient, FirebirdClientEmbeddedAttach, FirebirdClientRemoteAttach,
    FromRow, IntoParams,
};
use std::{cell::RefCell, marker};

use crate::{query::Queryable, statement::StatementData, Execute, Transaction};
use stmt_cache::{StmtCache, StmtCacheData};

/// The default builder for creating database connections
pub struct ConnectionBuilder<C: FirebirdClient> {
    host: String,
    port: u16,
    pass: String,
    db_name: String,
    user: String,
    dialect: Dialect,
    stmt_cache_size: usize,
    cli_args: C::Args,
    _cli_type: marker::PhantomData<C>,
}

/// The builder for creating database connections using the embedded firebird
pub struct ConnectionBuilderEmbedded<C: FirebirdClient> {
    db_name: String,
    user: String,
    dialect: Dialect,
    stmt_cache_size: usize,
    cli_args: C::Args,
    _cli_type: marker::PhantomData<C>,
}

/// The `PhantomMarker` makes it not Sync, but it is not true,
/// as the `ConnectionBuilder` does not store `C`
unsafe impl<C> Sync for ConnectionBuilder<C> where C: FirebirdClient {}
unsafe impl<C> Sync for ConnectionBuilderEmbedded<C> where C: FirebirdClient {}

impl<C> Clone for ConnectionBuilder<C>
where
    C: FirebirdClient,
{
    fn clone(&self) -> Self {
        Self {
            db_name: self.db_name.clone(),
            pass: self.pass.clone(),
            port: self.port,
            host: self.host.clone(),
            user: self.user.clone(),
            dialect: self.dialect,
            stmt_cache_size: self.stmt_cache_size,
            cli_args: self.cli_args.clone(),
            _cli_type: Default::default(),
        }
    }
}

#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
impl ConnectionBuilder<rsfbclient_native::NativeFbClient> {
    #[cfg(feature = "linking")]
    /// Uses the firebird client linked with the application at compile time
    pub fn linked() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            stmt_cache_size: 20,
            cli_args: rsfbclient_native::Args::Linking,
            _cli_type: Default::default(),
        }
    }

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
    pub fn with_client<S: Into<String>>(fbclient: S) -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            stmt_cache_size: 20,
            cli_args: rsfbclient_native::Args::DynamicLoading {
                lib_path: fbclient.into(),
            },
            _cli_type: Default::default(),
        }
    }

    /// Force the embedded server utilization. Host, port and pass
    /// will be ignored.
    pub fn embedded(self) -> ConnectionBuilderEmbedded<rsfbclient_native::NativeFbClient> {
        ConnectionBuilderEmbedded {
            db_name: self.db_name,
            user: self.user,
            dialect: self.dialect,
            stmt_cache_size: self.stmt_cache_size,
            cli_args: self.cli_args,
            _cli_type: Default::default(),
        }
    }
}

#[cfg(feature = "pure_rust")]
impl ConnectionBuilder<rsfbclient_rust::RustFbClient> {
    /// Uses the pure rust implementation of the firebird client
    pub fn pure_rust() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            db_name: "test.fdb".to_string(),
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            dialect: Dialect::D3,
            stmt_cache_size: 20,
            cli_args: (),
            _cli_type: Default::default(),
        }
    }
}

impl<C> ConnectionBuilder<C>
where
    C: FirebirdClient + FirebirdClientRemoteAttach,
{
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
    pub fn connect(&self) -> Result<Connection<C>, FbError> {
        Connection::open_remote(self, C::new(self.cli_args.clone())?)
    }
}

impl<C> ConnectionBuilderEmbedded<C>
where
    C: FirebirdClient + FirebirdClientEmbeddedAttach,
{
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
    pub fn connect(&self) -> Result<Connection<C>, FbError> {
        Connection::open_embedded(self, C::new(self.cli_args.clone())?)
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
    C: FirebirdClient + FirebirdClientRemoteAttach,
{
    /// Open a new connection to the database
    fn open_remote(builder: &ConnectionBuilder<C>, mut cli: C) -> Result<Connection<C>, FbError> {
        let handle = cli.attach_database(
            &builder.host,
            builder.port,
            &builder.db_name,
            &builder.user,
            &builder.pass,
        )?;

        let stmt_cache = RefCell::new(StmtCache::new(builder.stmt_cache_size));

        Ok(Connection {
            handle,
            dialect: builder.dialect,
            stmt_cache,
            cli: RefCell::new(cli),
        })
    }
}

impl<C> Connection<C>
where
    C: FirebirdClient + FirebirdClientEmbeddedAttach,
{
    /// Open a new connection to the database
    fn open_embedded(
        builder: &ConnectionBuilderEmbedded<C>,
        mut cli: C,
    ) -> Result<Connection<C>, FbError> {
        let handle = cli.attach_database(&builder.db_name, &builder.user)?;

        let stmt_cache = RefCell::new(StmtCache::new(builder.stmt_cache_size));

        Ok(Connection {
            handle,
            dialect: builder.dialect,
            stmt_cache,
            cli: RefCell::new(cli),
        })
    }
}

impl<C> Connection<C>
where
    C: FirebirdClient,
{
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
            .fetch(&self.tr.conn, &self.tr.data)
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
mk_tests_default! {
    use crate::*;

    #[test]
    fn remote_connection() {
        let conn = connect();

        conn.close().expect("error closing the connection");
    }

    #[test]
    fn query_iter() {
        let mut conn = connect();

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
}
