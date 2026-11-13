//!
//! Rust Firebird Client
//!
//! Transaction functions
//!

use rsfbclient_core::{
    ColumnDesc, FbError, FirebirdClient, FromRow, IntoParams, StmtPlan, TrOp,
    TransactionConfiguration,
};
use std::marker;
use std::mem;

use super::{connection::Connection, statement::Statement};
use crate::{
    connection::stmt_cache::StmtCache, connection::stmt_cache::StmtCacheData,
    statement::StatementData, Execute, PlanRead, Queryable,
};

mod simple;
pub use simple::SimpleTransaction;

mod builder;
pub use builder::{transaction_builder, TransactionConfigurationBuilder};

pub struct Transaction<'c, C>
where
    C: FirebirdClient,
{
    pub(crate) data: TransactionData<C>,
    pub(crate) conn: &'c mut Connection<C>,
}

impl<'c, C: FirebirdClient> Transaction<'c, C> {
    /// Start a new transaction
    pub fn new(
        conn: &'c mut Connection<C>,
        confs: TransactionConfiguration,
    ) -> Result<Self, FbError> {
        let data = TransactionData::new(conn, confs)?;

        Ok(Transaction { data, conn })
    }

    /// Commit the current transaction changes
    pub fn commit(mut self) -> Result<(), FbError> {
        let result = self.data.commit(self.conn);

        if result.is_ok() {
            mem::forget(self);
        } else {
            let _ = self.rollback();
        }

        result
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
        let result = self.data.rollback(self.conn);
        mem::forget(self);
        result
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(&mut self, sql: &str) -> Result<(), FbError> {
        self.data.execute_immediate(self.conn, sql)
    }

    /// Prepare a new statement for execute
    pub fn prepare<'t>(
        &'t mut self,
        sql: &str,
        named_params: bool,
    ) -> Result<Statement<'c, 't, C>, FbError> {
        Statement::prepare(self, sql, named_params)
    }
}

impl<'c, C: FirebirdClient> Drop for Transaction<'c, C> {
    fn drop(&mut self) {
        self.data.rollback(self.conn).ok();
    }
}

/// Variant of the `StatementIter` that uses the statement cache
pub struct StmtIter<'c, 'a, R, C>
where
    C: FirebirdClient,
{
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData<StatementData<C>>>,

    /// Transaction needs to be alive for the fetch to work
    tr: &'a mut Transaction<'c, C>,

    _marker: marker::PhantomData<R>,
}

impl<R, C> Drop for StmtIter<'_, '_, R, C>
where
    C: FirebirdClient,
{
    fn drop(&mut self) {
        let mut stmt_cache_data = self.stmt_cache_data.take().unwrap();

        if stmt_cache_data.stmt.close_cursor(self.tr.conn).is_ok() {
            // Only cache statements whose cursor was closed successfully.
            StmtCache::insert_and_close(self.tr.conn, stmt_cache_data).ok();
        } else {
            // A statement with an uncertain cursor state must not be reused.
            stmt_cache_data.stmt.close(self.tr.conn).ok();
        }
    }
}

impl<R, C> Iterator for StmtIter<'_, '_, R, C>
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
            .fetch(self.tr.conn, &mut self.tr.data)
            .and_then(|row| row.map(FromRow::try_from).transpose())
            .transpose()
    }
}

/// The plan a `with_plan` helper was asked for. `None` cannot happen when it
/// was asked, but a driver does not panic over it.
pub(crate) fn plan_read(plan: Option<PlanRead>) -> PlanRead {
    plan.unwrap_or_else(|| Err(FbError::from("the plan was not read")))
}

impl<'c, C: FirebirdClient> Transaction<'c, C> {
    /// Prepare `sql` (or take it from the cache), read its plan when asked —
    /// before the cursor opens, from the prepared statement — and open the
    /// cursor. A statement whose cursor failed to open is closed, not cached.
    fn open_cursor<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
        with_plan: bool,
    ) -> Result<(StmtIter<'c, 'a, R, C>, Option<PlanRead>), FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let params = params.to_params();

        // Get a statement from the cache
        let mut stmt_cache_data = StmtCache::get_or_prepare(self, sql, params.named())?;

        let plan = with_plan.then(|| stmt_cache_data.stmt.plan(self.conn));

        match stmt_cache_data
            .stmt
            .query(self.conn, &mut self.data, params)
        {
            Ok(_) => {
                let iter = StmtIter {
                    stmt_cache_data: Some(stmt_cache_data),
                    tr: self,
                    _marker: Default::default(),
                };

                Ok((iter, plan))
            }
            Err(e) => {
                // A statement that failed to open its cursor must not be reused.
                stmt_cache_data.stmt.close(self.conn).ok();

                Err(e)
            }
        }
    }

    /// Prepare `sql` (or take it from the cache), read its plan when asked,
    /// execute it and hand the statement back to the cache.
    fn exec_stmt<P>(
        &mut self,
        sql: &str,
        params: P,
        with_plan: bool,
    ) -> Result<(usize, Option<PlanRead>), FbError>
    where
        P: IntoParams,
    {
        let params = params.to_params();

        // Get a statement from the cache
        let mut stmt_cache_data = StmtCache::get_or_prepare(self, sql, params.named())?;

        let plan = with_plan.then(|| stmt_cache_data.stmt.plan(self.conn));

        // Do not return now in case of error, because we need to return the statement to the cache
        let res = stmt_cache_data
            .stmt
            .execute(self.conn, &mut self.data, params);

        match res {
            Ok(value) => {
                StmtCache::insert_and_close(self.conn, stmt_cache_data)?;
                Ok((value, plan))
            }
            Err(error) => {
                stmt_cache_data.stmt.close(self.conn).ok();
                Err(error)
            }
        }
    }
}

impl<'c, C: FirebirdClient> Queryable for Transaction<'c, C> {
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let (iter, _) = self.open_cursor(sql, params, false)?;

        Ok(Box::new(iter))
    }

    fn query_iter_with_plan<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<(Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, PlanRead), FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let (iter, plan) = self.open_cursor(sql, params, true)?;

        Ok((Box::new(iter), plan_read(plan)))
    }

    fn describe(&mut self, sql: &str) -> Result<Vec<ColumnDesc>, FbError> {
        // Prepared here, then handed to the cache: the query that usually
        // follows a describe reuses the same statement.
        let mut stmt_cache_data = StmtCache::get_or_prepare(self, sql, false)?;

        match stmt_cache_data.stmt.describe(self.conn) {
            Ok(columns) => {
                StmtCache::insert_and_close(self.conn, stmt_cache_data)?;
                Ok(columns)
            }
            Err(e) => {
                stmt_cache_data.stmt.close(self.conn).ok();
                Err(e)
            }
        }
    }

    fn plan(&mut self, sql: &str) -> Result<StmtPlan, FbError> {
        let mut stmt_cache_data = StmtCache::get_or_prepare(self, sql, false)?;

        match stmt_cache_data.stmt.plan(self.conn) {
            Ok(plan) => {
                StmtCache::insert_and_close(self.conn, stmt_cache_data)?;
                Ok(plan)
            }
            Err(e) => {
                stmt_cache_data.stmt.close(self.conn).ok();
                Err(e)
            }
        }
    }
}

impl<C: FirebirdClient> Execute for Transaction<'_, C> {
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<usize, FbError>
    where
        P: IntoParams,
    {
        self.exec_stmt(sql, params, false).map(|(n, _)| n)
    }

    fn execute_with_plan<P>(&mut self, sql: &str, params: P) -> Result<(usize, PlanRead), FbError>
    where
        P: IntoParams,
    {
        self.exec_stmt(sql, params, true)
            .map(|(n, plan)| (n, plan_read(plan)))
    }

    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<R, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let params = params.to_params();

        // Get a statement from the cache
        let mut stmt_cache_data = StmtCache::get_or_prepare(self, sql, params.named())?;

        // Do not return now in case of error, because we need to return the statement to the cache
        let res = stmt_cache_data
            .stmt
            .execute2(self.conn, &mut self.data, params);

        match res {
            Ok(row) => {
                StmtCache::insert_and_close(self.conn, stmt_cache_data)?;
                FromRow::try_from(row)
            }
            Err(error) => {
                stmt_cache_data.stmt.close(self.conn).ok();
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
/// Low level transaction handler.
///
/// Needs to be closed calling `rollback` before dropping.
pub struct TransactionData<C: FirebirdClient> {
    pub(crate) handle: C::TrHandle,
}

impl<C: FirebirdClient> TransactionData<C>
where
    C::TrHandle: Send,
{
    /// Start a new transaction
    fn new(conn: &mut Connection<C>, confs: TransactionConfiguration) -> Result<Self, FbError> {
        let handle = conn.cli.begin_transaction(&mut conn.handle, confs)?;

        Ok(Self { handle })
    }

    /// Execute the statement without returning any row
    fn execute_immediate(&mut self, conn: &mut Connection<C>, sql: &str) -> Result<(), FbError> {
        conn.cli
            .exec_immediate(&mut conn.handle, &mut self.handle, conn.dialect, sql)
    }

    /// Commit the current transaction changes, not allowing to reuse the transaction
    pub fn commit(&mut self, conn: &mut Connection<C>) -> Result<(), FbError> {
        conn.cli
            .transaction_operation(&mut self.handle, TrOp::Commit)
    }

    /// Commit the current transaction changes, but allowing to reuse the transaction
    pub fn commit_retaining(&mut self, conn: &mut Connection<C>) -> Result<(), FbError> {
        conn.cli
            .transaction_operation(&mut self.handle, TrOp::CommitRetaining)
    }

    /// Rollback the current transaction changes, but allowing to reuse the transaction
    pub fn rollback_retaining(&mut self, conn: &mut Connection<C>) -> Result<(), FbError> {
        conn.cli
            .transaction_operation(&mut self.handle, TrOp::RollbackRetaining)
    }

    /// Rollback the transaction, invalidating it
    pub fn rollback(&mut self, conn: &mut Connection<C>) -> Result<(), FbError> {
        conn.cli
            .transaction_operation(&mut self.handle, TrOp::Rollback)
    }

    /// Creates a transaction from this handle
    pub fn into_transaction(self, conn: &mut Connection<C>) -> Transaction<C> {
        Transaction { data: self, conn }
    }

    /// Extracts the `TransactionData` from the transaction
    pub fn from_transaction(tr: Transaction<C>) -> Self {
        let tr = mem::ManuallyDrop::new(tr);

        // Make a copy the handler. Is safe as the original copy cant be dropped as it is inside the `ManuallyDrop`
        unsafe { std::ptr::read(&tr.data) }
    }
}
