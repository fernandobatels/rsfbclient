//!
//! Rust Firebird Client
//!
//! Transaction functions
//!

use std::{marker, ptr};

use super::connection::Connection;
use super::ibase;
use super::params::IntoParams;
use super::statement::Statement;
use super::status::FbError;
use crate::{
    connection::stmt_cache::StmtCacheData,
    params::Params,
    row::{ColumnBuffer, FromRow},
    Queryable,
};

pub struct Transaction<'c> {
    pub(crate) data: TransactionData,
    pub(crate) conn: &'c Connection,
}

impl<'c> Transaction<'c> {
    /// Start a new transaction
    pub fn start_transaction(conn: &Connection) -> Result<Transaction, FbError> {
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
    pub fn prepare(&mut self, sql: &str) -> Result<Statement<'c>, FbError> {
        Statement::prepare(self, sql)
    }
}

impl<'c> Drop for Transaction<'c> {
    fn drop(&mut self) {
        self.data.rollback(self.conn).ok();
    }
}

/// Variant of the `StatementIter` that uses the statement cache
struct StmtIter<'a, R> {
    /// Statement cache data. Wrapped in option to allow taking the value to send back to the cache
    stmt_cache_data: Option<StmtCacheData>,

    /// Buffers for the column data
    buffers: Vec<ColumnBuffer>,

    /// Transaction needs to be alive for the fetch to work
    tr: &'a Transaction<'a>,

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
            .insert_and_close(self.tr.conn, self.stmt_cache_data.take().unwrap())
            .ok();
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

impl Queryable for Transaction<'_> {
    /// Prepare, execute and return the rows of the sql query
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: crate::row::FromRow + 'a,
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

                Ok(Box::new(iter))
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

    /// Prepare and execute the sql query
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
pub struct TransactionData {
    pub(crate) handle: ibase::isc_tr_handle,
}

impl TransactionData {
    /// Start a new transaction
    fn new(conn: &Connection) -> Result<Self, FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let mut handle = 0;

        #[repr(C)]
        struct IscTeb {
            db_handle: *mut ibase::isc_db_handle,
            tpb_len: usize,
            tpb_ptr: *mut u8,
        }

        unsafe {
            if ibase.isc_start_multiple()(
                status.borrow_mut().as_mut_ptr(),
                &mut handle,
                1,
                &mut IscTeb {
                    db_handle: conn.handle.as_ptr(),
                    tpb_len: 0,
                    tpb_ptr: ptr::null_mut(),
                } as *mut _ as _,
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle, 0);

        Ok(Self { handle })
    }

    /// Execute the statement without returning any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute_immediate<T>(
        &mut self,
        conn: &Connection,
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
    pub fn commit_retaining(&mut self, conn: &Connection) -> Result<(), FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        unsafe {
            if ibase.isc_commit_retaining()(status.borrow_mut().as_mut_ptr(), &mut self.handle) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        Ok(())
    }

    /// Rollback the current transaction changes, but allowing to reuse the transaction
    pub fn rollback_retaining(&mut self, conn: &Connection) -> Result<(), FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        unsafe {
            if ibase.isc_rollback_retaining()(status.borrow_mut().as_mut_ptr(), &mut self.handle)
                != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        Ok(())
    }

    /// Rollback the transaction, invalidating it
    pub fn rollback(&mut self, conn: &Connection) -> Result<(), FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        // Rollback the transaction, if the handle is valid
        if self.handle != 0
            && unsafe {
                ibase.isc_rollback_transaction()(status.borrow_mut().as_mut_ptr(), &mut self.handle)
            } != 0
        {
            return Err(status.borrow().as_error(ibase));
        }

        // Assert that the handle is invalid
        debug_assert_eq!(self.handle, 0);

        Ok(())
    }
}
