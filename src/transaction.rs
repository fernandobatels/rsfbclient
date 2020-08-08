//!
//! Rust Firebird Client
//!
//! Transaction functions
//!

use rsfbclient_core::{FbError, FirebirdClient, TrIsolationLevel, TrOp};
use std::{marker, ptr};

use super::{connection::Connection, params::IntoParams, statement::Statement};
use crate::{
    connection::stmt_cache::StmtCacheData,
    params::Params,
    row::{ColumnBuffer, FromRow},
    Execute, Queryable,
};

pub struct Transaction<'c, C>
where
    C: FirebirdClient,
{
    pub(crate) data: TransactionData<C::TrHandle>,
    pub(crate) conn: &'c Connection<C>,
}

impl<'c, C> Transaction<'c, C> {
    /// Start a new transaction
    pub fn new(conn: &Connection<C>) -> Result<Self, FbError> {
        let data = TransactionData::new(conn)?;

        Ok(Transaction { data, conn })
    }

    /// Commit the current transaction changes
    pub fn commit(mut self) -> Result<(), FbError> {
        self.data.commit_retaining(self.conn)
    }

    /// Commit the current transaction changes, but allowing to reuse the transaction
    pub fn commit_retaining(&mut self) -> Result<(), FbError> {
        self.data.commit_retaining(self.conn)
    }

    /// Rollback the current transaction changes, but allowing to reuse the transaction
    pub fn rollback_retaining(&mut self) -> Result<(), FbError> {
        self.data.rollback_retaining(self.conn)
    }

    /// Rollback the current transaction changes
    pub fn rollback(mut self) -> Result<(), FbError> {
        self.data.rollback(self.conn)
    }

    /// Execute the statement without returning any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute_immediate<T>(&mut self, sql: &str, params: T) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        self.data.execute_immediate(self.conn, sql, params)
    }

    /// Prepare a new statement for execute
    pub fn prepare(&mut self, sql: &str) -> Result<Statement<'c, C>, FbError> {
        Statement::prepare(self, sql)
    }
}

impl<'c, C> Drop for Transaction<'c, C> {
    fn drop(&mut self) {
        self.data.rollback(self.conn).ok();
    }
}

/// Variant of the `StatementIter` that uses the statement cache
pub struct StmtIter<'a, R, C>
where
    C: FirebirdClient,
{
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData<C::StmtHandle>>,

    /// Buffers for the column data
    buffers: Vec<ColumnBuffer>,

    /// Transaction needs to be alive for the fetch to work
    tr: &'a Transaction<'a, C>,

    _marker: marker::PhantomData<R>,
}

impl<R, C> Drop for StmtIter<'_, R, C> {
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
    }
}

impl<R, C> Iterator for StmtIter<'_, R, C>
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

impl<'a, R, C> Queryable<'a, R> for Transaction<'a, C>
where
    R: FromRow + 'a,
{
    type Iter = StmtIter<'a, R, C>;

    /// Prepare, execute and return the rows of the sql query
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_iter<P>(&'a mut self, sql: &str, params: P) -> Result<Self::Iter, FbError>
    where
        P: IntoParams,
    {
        // Get a statement from the cache
        let mut stmt_cache_data =
            self.conn
                .stmt_cache
                .borrow_mut()
                .get_or_prepare(self.conn, &mut self.data, sql)?;

        match stmt_cache_data
            .stmt
            .query(self.conn, &mut self.data, params)
        {
            Ok(buffers) => {
                let iter = StmtIter {
                    stmt_cache_data: Some(stmt_cache_data),
                    buffers,
                    tr: self,
                    _marker: Default::default(),
                };

                Ok(iter)
            }
            Err(e) => {
                // Return the statement to the cache
                self.conn
                    .stmt_cache
                    .borrow_mut()
                    .insert_and_close(self.conn, stmt_cache_data)?;

                Err(e)
            }
        }
    }
}

impl<C> Execute for Transaction<'_, C> {
    /// Prepare and execute the sql query
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams,
    {
        // Get a statement from the cache
        let mut stmt_cache_data =
            self.conn
                .stmt_cache
                .borrow_mut()
                .get_or_prepare(self.conn, &mut self.data, sql)?;

        // Do not return now in case of error, because we need to return the statement to the cache
        let res = stmt_cache_data
            .stmt
            .execute(self.conn, &mut self.data, params);

        // Return the statement to the cache
        self.conn
            .stmt_cache
            .borrow_mut()
            .insert_and_close(self.conn, stmt_cache_data)?;

        res?;

        Ok(())
    }
}

#[derive(Debug)]
/// Low level transaction handler.
///
/// Needs to be closed calling `rollback` before dropping.
pub struct TransactionData<H> {
    pub(crate) handle: H,
}

impl<H, C> TransactionData<H>
where
    C: FirebirdClient<TrHandle = H>,
{
    /// Start a new transaction
    fn new(conn: &Connection<C>) -> Result<Self, FbError> {
        conn.cli
            .borrow_mut()
            .begin_transaction(conn.handle, TrIsolationLevel::ReadCommited)
    }

    /// Execute the statement without returning any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute_immediate<T>(
        &mut self,
        conn: &Connection<C>,
        sql: &str,
        params: T,
    ) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let params = Params::new_immediate(params.to_params());

        unsafe {
            if ibase.isc_dsql_execute_immediate()(
                status.borrow_mut().as_mut_ptr(),
                conn.handle.as_ptr(),
                &mut self.handle,
                sql.len() as u16,
                sql.as_ptr() as *const _,
                conn.dialect as u16,
                if let Some(xsqlda) = &params.xsqlda {
                    &**xsqlda
                } else {
                    ptr::null()
                },
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        // Just to make sure the params are not dropped too soon
        drop(params);
        Ok(())
    }

    /// Commit the current transaction changes, but allowing to reuse the transaction
    pub fn commit_retaining(&mut self, conn: &Connection<C>) -> Result<(), FbError> {
        conn.cli
            .borrow_mut()
            .transaction_operation(self.handle, TrOp::CommitRetaining);
        Ok(())
    }

    /// Rollback the current transaction changes, but allowing to reuse the transaction
    pub fn rollback_retaining(&mut self, conn: &Connection<C>) -> Result<(), FbError> {
        conn.cli
            .borrow_mut()
            .transaction_operation(self.handle, TrOp::RollbackRetaining);
        Ok(())
    }

    /// Rollback the transaction, invalidating it
    pub fn rollback(&mut self, conn: &Connection<C>) -> Result<(), FbError> {
        conn.cli
            .borrow_mut()
            .transaction_operation(self.handle, TrOp::Rollback);
        Ok(())
    }
}
