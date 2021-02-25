//!
//! Rust Firebird Client
//!
//! Connection functions
//!
use rsfbclient_core::{Dialect, FbError, FirebirdClient, FirebirdClientDbOps, FromRow, IntoParams};
use std::{marker, mem};

use crate::{
    query::Queryable, statement::StatementData, transaction::TransactionData, Execute, Transaction,
};
use stmt_cache::{StmtCache, StmtCacheData};

pub mod builders {

    #![allow(unused_imports)]
    use super::{
        super::{charset, Charset},
        Connection, ConnectionConfiguration, Dialect, FbError, FirebirdClient,
        FirebirdClientFactory,
    };

    #[cfg(feature = "native_client")]
    mod builder_native;
    #[cfg(feature = "native_client")]
    pub use builder_native::*;

    #[cfg(feature = "pure_rust")]
    mod builder_pure_rust;
    #[cfg(feature = "pure_rust")]
    pub use builder_pure_rust::*;
}

pub(crate) mod conn_string;
pub(crate) mod stmt_cache;

pub(crate) mod simple;
pub use simple::SimpleConnection;

/// A generic factory for creating multiple preconfigured instances of a particular client implementation
/// Intended mainly for use by connection pool
pub trait FirebirdClientFactory {
    type C: FirebirdClient;

    /// Construct a new instance of a client
    fn new_instance(&self) -> Result<Self::C, FbError>;

    /// Pull the connection configuration details out as a borrow
    fn get_conn_conf(
        &self,
    ) -> &ConnectionConfiguration<<Self::C as FirebirdClientDbOps>::AttachmentConfig>;
}

/// Generic aggregate of configuration data for firebird db Connections
/// The data required for forming connections is partly client-implementation-dependent
#[derive(Clone)]
pub struct ConnectionConfiguration<A> {
    attachment_conf: A,
    dialect: Dialect,
    stmt_cache_size: usize,
}

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
    pub(crate) stmt_cache: StmtCache<StatementData<C>>,

    /// Default transaction to be used when no explicit
    /// transaction is used
    pub(crate) def_tr: Option<TransactionData<C>>,

    /// If true, methods in `Queryable` and `Executable` should not
    /// automatically commit and rollback
    pub(crate) in_transaction: bool,

    /// Firebird client
    pub(crate) cli: C,
}

impl<C: FirebirdClient> Connection<C> {
    /// Open the client connection.
    pub fn open(
        mut cli: C,
        conf: &ConnectionConfiguration<C::AttachmentConfig>,
    ) -> Result<Connection<C>, FbError> {
        let handle = cli.attach_database(&conf.attachment_conf)?;
        let stmt_cache = StmtCache::new(conf.stmt_cache_size);

        Ok(Connection {
            handle,
            dialect: conf.dialect,
            stmt_cache,
            def_tr: None,
            in_transaction: false,
            cli,
        })
    }

    /// Drop the current database
    pub fn drop_database(mut self) -> Result<(), FbError> {
        self.cli.drop_database(&mut self.handle)?;

        Ok(())
    }

    /// Close the current connection.
    pub fn close(mut self) -> Result<(), FbError> {
        let res = self.cleanup_and_detach();
        mem::forget(self);
        res
    }

    // Cleans up statement cache and releases the database handle
    fn cleanup_and_detach(&mut self) -> Result<(), FbError> {
        StmtCache::close_all(self);

        // Drop the default transaction
        if let Some(mut tr) = self.def_tr.take() {
            tr.rollback(self).ok();
        }

        self.cli.detach_database(&mut self.handle)?;

        Ok(())
    }

    /// Run a closure with a transaction, if the closure returns an error
    /// and the default transaction is not active, the transaction will rollback, else it will be committed
    pub fn with_transaction<T, F>(&mut self, closure: F) -> Result<T, FbError>
    where
        F: FnOnce(&mut Transaction<C>) -> Result<T, FbError>,
    {
        let in_transaction = self.in_transaction;

        let mut tr = if let Some(tr) = self.def_tr.take() {
            tr.into_transaction(self)
        } else {
            Transaction::new(self)?
        };

        let res = closure(&mut tr);

        if !in_transaction {
            if res.is_ok() {
                tr.commit_retaining()?;
            } else {
                tr.rollback_retaining()?;
            }
        }

        let tr = TransactionData::from_transaction(tr);

        if let Some(mut tr) = self.def_tr.replace(tr) {
            // Should never happen, but just to be sure
            tr.rollback(self).ok();
        }

        res
    }

    /// Run a closure with the default transaction, no rollback or commit will be automatically performed
    /// after the closure returns. The next call to this function will use the same transaction
    /// if it was not closed with `commit_retaining` or `rollback_retaining`
    fn use_transaction<T, F>(&mut self, closure: F) -> Result<T, FbError>
    where
        F: FnOnce(&mut Transaction<C>) -> Result<T, FbError>,
    {
        let mut tr = if let Some(tr) = self.def_tr.take() {
            tr.into_transaction(self)
        } else {
            Transaction::new(self)?
        };

        let res = closure(&mut tr);

        let tr = TransactionData::from_transaction(tr);

        if let Some(mut tr) = self.def_tr.replace(tr) {
            // Should never happen, but just to be sure
            tr.rollback(self).ok();
        }

        res
    }

    /// Begins a new transaction, and instructs all the `query` and `execute` methods
    /// performed in the [`Connection`] type to not automatically commit and rollback
    /// until [`commit`][`Connection::commit`] or [`rollback`][`Connection::rollback`] are called
    pub fn begin_transaction(&mut self) -> Result<(), FbError> {
        let tr = if let Some(tr) = self.def_tr.take() {
            tr.into_transaction(self)
        } else {
            Transaction::new(self)?
        };

        let tr = TransactionData::from_transaction(tr);

        if let Some(mut tr) = self.def_tr.replace(tr) {
            // Should never happen, but just to be sure
            tr.rollback(self).ok();
        }

        self.in_transaction = true;

        Ok(())
    }

    /// Commit the default transaction
    pub fn commit(&mut self) -> Result<(), FbError> {
        self.in_transaction = false;

        self.use_transaction(|tr| tr.commit_retaining())
    }

    /// Rollback the default transaction
    pub fn rollback(&mut self) -> Result<(), FbError> {
        self.in_transaction = false;

        self.use_transaction(|tr| tr.rollback_retaining())
    }
}

impl<C: FirebirdClient> Drop for Connection<C> {
    fn drop(&mut self) {
        // Ignore the possible error value
        let _ = self.cleanup_and_detach();
    }
}

/// Variant of the `StatementIter` borrows `Connection` and uses the statement cache
pub struct StmtIter<'a, R, C: FirebirdClient> {
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData<StatementData<C>>>,

    conn: &'a mut Connection<C>,

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
            .close_cursor(self.conn)
            .ok();

        // Send the statement back to the cache
        StmtCache::insert_and_close(self.conn, self.stmt_cache_data.take().unwrap()).ok();

        if !self.conn.in_transaction {
            // Commit the transaction
            self.conn.commit().ok();
        }
    }
}

impl<R, C> Iterator for StmtIter<'_, R, C>
where
    R: FromRow,
    C: FirebirdClient,
{
    type Item = Result<R, FbError>;

    fn next(&mut self) -> Option<Self::Item> {
        let stmt_cache_data = self.stmt_cache_data.as_mut().unwrap();

        self.conn
            .use_transaction(move |tr| {
                Ok(stmt_cache_data
                    .stmt
                    .fetch(tr.conn, &mut tr.data)
                    .and_then(|row| row.map(FromRow::try_from).transpose())
                    .transpose())
            })
            .unwrap_or_default()
    }
}

impl<C> Queryable for Connection<C>
where
    C: FirebirdClient,
{
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let stmt_cache_data = self.use_transaction(|tr| {
            let params = params.to_params();

            // Get a statement from the cache
            let mut stmt_cache_data = StmtCache::get_or_prepare(tr, sql, params.named())?;

            match stmt_cache_data.stmt.query(tr.conn, &mut tr.data, params) {
                Ok(_) => Ok(stmt_cache_data),
                Err(e) => {
                    // Return the statement to the cache
                    StmtCache::insert_and_close(tr.conn, stmt_cache_data)?;

                    if !tr.conn.in_transaction {
                        tr.rollback_retaining().ok();
                    }

                    Err(e)
                }
            }
        })?;

        let iter = StmtIter {
            stmt_cache_data: Some(stmt_cache_data),
            conn: self,
            _marker: Default::default(),
        };

        Ok(Box::new(iter))
    }
}

impl<C> Execute for Connection<C>
where
    C: FirebirdClient,
{
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<usize, FbError>
    where
        P: IntoParams,
    {
        let params = params.to_params();

        self.with_transaction(|tr| {
            // Get a statement from the cache
            let mut stmt_cache_data = StmtCache::get_or_prepare(tr, sql, params.named())?;

            // Do not return now in case of error, because we need to return the statement to the cache
            let res = stmt_cache_data.stmt.execute(tr.conn, &mut tr.data, params);

            // Return the statement to the cache
            StmtCache::insert_and_close(tr.conn, stmt_cache_data)?;

            res
        })
    }

    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<R, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let params = params.to_params();

        self.with_transaction(|tr| {
            // Get a statement from the cache
            let mut stmt_cache_data = StmtCache::get_or_prepare(tr, sql, params.named())?;

            // Do not return now in case of error, because we need to return the statement to the cache
            let res = stmt_cache_data.stmt.execute2(tr.conn, &mut tr.data, params);

            // Return the statement to the cache
            StmtCache::insert_and_close(tr.conn, stmt_cache_data)?;

            let f_res = FromRow::try_from(res?)?;

            Ok(f_res)
        })
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
