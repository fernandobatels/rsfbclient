//!
//! Rust Firebird Client
//!
//! High level api
//!

use rsfbclient_core::{FbError, FromRow, IntoParams};

/// Implemented for types that can be used to execute sql queries
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
        R: FromRow + 'static;

    /// Returns the results of the query as a `Vec`
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query<'a, P, R>(&'a mut self, sql: &str, params: P) -> Result<Vec<R>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        self.query_iter(sql, params)?.collect()
    }

    /// Returns the first result of the query, or None
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_first<'a, P, R>(&'a mut self, sql: &str, params: P) -> Result<Option<R>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        self.query_iter(sql, params)?.next().transpose()
    }
}

/// Implemented for types that can be used to execute sql statements
pub trait Execute {
    /// Execute a query, may or may not commit the changes
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams;

    /// Execute a query that will return data,
    /// like the 'insert ... returning ..' or 'execute block'
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<Vec<R>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static;

}
