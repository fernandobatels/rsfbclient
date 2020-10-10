//!
//! Rust Firebird Client
//!
//! Connection functions
//!

#[cfg(feature = "pool")]
pub mod pool;

#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
pub mod builder_native;

#[cfg(feature = "pure_rust")]
pub mod builder_pure_rust;

pub mod stmt_cache;

use rsfbclient_core::{Dialect, FbError, FirebirdClient, FirebirdClientDbOps, FromRow, IntoParams};
use std::{cell::RefCell, marker, mem::ManuallyDrop};

use crate::{query::Queryable, statement::StatementData, Execute, Transaction};
use stmt_cache::{StmtCache, StmtCacheData};

// A generic factory for creating multiple instances of a particular client implementation
// Intended mainly for use by connection pool
pub trait FirebirdClientFactory {
    type C: FirebirdClient;
    fn new_instance(&self) -> Result<Self::C, FbError>;
}

#[doc(hidden)]
#[derive(Clone)]
pub struct ConnectionConfiguration<A> {
    attachment_conf: A,
    dialect: Dialect,
    stmt_cache_size: usize,
}

#[doc(hidden)]
impl<A: Default> Default for ConnectionConfiguration<A> {
    fn default() -> Self {
        Self {
            attachment_conf: Default::default(),
            dialect: Dialect::D3,
            stmt_cache_size: 20,
        }
    }
}

/// A connection to a firebird database
pub struct Connection<C: FirebirdClient> {
    /// Database handler
    pub(crate) handle: <C as FirebirdClientDbOps>::DbHandle,

    /// Firebird dialect for the statements
    pub(crate) dialect: Dialect,

    /// Cache for the prepared statements
    pub(crate) stmt_cache: RefCell<StmtCache<StatementData<C>>>,

    /// Firebird client
    pub(crate) cli: RefCell<C>,
}

impl<C: FirebirdClient> Connection<C> {
    fn open(
        mut cli: C,
        conf: &ConnectionConfiguration<C::AttachmentConfig>,
    ) -> Result<Connection<C>, FbError> {
        let handle = cli.attach_database(&conf.attachment_conf)?;
        let stmt_cache = RefCell::new(StmtCache::new(conf.stmt_cache_size));

        Ok(Connection {
            handle,
            dialect: conf.dialect,
            stmt_cache,
            cli: RefCell::new(cli),
        })
    }

    /// Drop the current database
    pub fn drop_database(mut self) -> Result<(), FbError> {
        self.cli.get_mut().drop_database(self.handle)?;

        Ok(())
    }

    /// Close the current connection.
    pub fn close(mut self) -> Result<(), FbError> {
        let res = self.cleanup_and_detach();
        ManuallyDrop::new(self);
        res
    }

    //cleans up statement cache and releases the database handle
    fn cleanup_and_detach(&mut self) -> Result<(), FbError> {
        self.stmt_cache.borrow_mut().close_all(self);

        self.cli.get_mut().detach_database(self.handle)?;

        Ok(())
    }

    /// Run a closure with a transaction, if the closure returns an error
    /// the transaction will rollback, else it will be committed
    pub fn with_transaction<T, F>(&self, closure: F) -> Result<T, FbError>
    where
        F: FnOnce(&mut Transaction<C>) -> Result<T, FbError>,
    {
        let mut tr = Transaction::new(self)?;

        let res = closure(&mut tr);

        if res.is_ok() {
            tr.commit_retaining()?;
        } else {
            tr.rollback_retaining()?;
        };

        res
    }
}

impl<C: FirebirdClient> Drop for Connection<C> {
    fn drop(&mut self) {
        //ignore the possible error value
        let _ = self.cleanup_and_detach();
    }
}

/// Variant of the `StatementIter` that owns the `Transaction` and uses the statement cache
pub struct StmtIter<'a, R, C: FirebirdClient> {
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData<StatementData<C>>>,

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

impl<C> Queryable for Connection<C>
where
    C: FirebirdClient,
{
    /// Prepare, execute, return the rows and commit the sql query
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
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

                Ok(Box::new(iter))
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
    fn remote_connection() -> Result<(), FbError> {
        let conn = cbuilder().connect()?;

        conn.close().expect("error closing the connection");

        Ok(())
    }

    #[test]
    fn query_iter() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let mut rows = 0;

        for row in conn
            .query_iter("SELECT -3 FROM RDB$DATABASE WHERE 1 = ?", (1,))?
        {
            let (v,): (i32,) = row?;

            assert_eq!(v, -3);

            rows += 1;
        }

        assert_eq!(rows, 1);

        Ok(())
    }
}
