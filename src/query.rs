//!
//! Rust Firebird Client
//!
//! High level api
//!

use crate::{params::IntoParams, row::FromRow, FbError};

/// Implemented for types that can be used to execute sql queries and statements
pub trait Queryable {
    /// Returns the results of the query as an iterator
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'a;

    /// Execute a query, may or may not commit the changes
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams;

    /// Returns the results of the query as a `Vec`
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query<P, R>(&mut self, sql: &str, params: P) -> Result<Vec<R>, FbError>
    where
        P: IntoParams,
        R: FromRow,
    {
        self.query_iter(sql, params)?.collect()
    }

    /// Returns the first result of the query, or None
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_first<P, R>(&mut self, sql: &str, params: P) -> Result<Option<R>, FbError>
    where
        P: IntoParams,
        R: FromRow,
    {
        self.query_iter(sql, params)?.next().transpose()
    }
}
