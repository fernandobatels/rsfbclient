//!
//! Rust Firebird Client
//!
//! High level api
//!

use crate::{params::IntoParams, row::FromRow, FbError};

/// Implemented for types that can be used to execute sql queries
pub trait Queryable<'a, R>
where
    R: FromRow + 'a,
{
    type Iter: Iterator<Item = Result<R, FbError>> + 'a;

    /// Returns the results of the query as an iterator
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_iter<P>(&'a mut self, sql: &str, params: P) -> Result<Self::Iter, FbError>
    where
        P: IntoParams;

    /// Returns the results of the query as a `Vec`
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query<P>(&'a mut self, sql: &str, params: P) -> Result<Vec<R>, FbError>
    where
        P: IntoParams,
    {
        self.query_iter(sql, params)?.collect()
    }

    /// Returns the first result of the query, or None
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn query_first<P>(&'a mut self, sql: &str, params: P) -> Result<Option<R>, FbError>
    where
        P: IntoParams,
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
}
