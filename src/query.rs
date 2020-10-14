//!
//! Rust Firebird Client
//!
//! High level api
//!

use rsfbclient_core::{FbError, FromRow, IntoParams};

/// Implemented for types that can be used to execute sql queries
pub trait Queryable {
    /// Returns the results of the query as an iterator.
    ///
    ///
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
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
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
    fn query<'a, P, R>(&'a mut self, sql: &str, params: P) -> Result<Vec<R>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        self.query_iter(sql, params)?.collect()
    }

    /// Returns the first result of the query, or None
    ///
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
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
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams;
}
