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
    /// The query must be return an open cursor, so for cases like 'insert .. returning'
    /// you will need to use the [execute_returnable](prelude/trait.Execute.html#tymethod.execute_returnable) method instead.
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
    /// The query must be return an open cursor, so for cases like 'insert .. returning'
    /// you will need to use the [execute_returnable](prelude/trait.Execute.html#tymethod.execute_returnable) method instead.
    ///
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
    fn query<P, R>(&mut self, sql: &str, params: P) -> Result<Vec<R>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        self.query_iter(sql, params)?.collect()
    }

    /// Returns the first result of the query, or None.
    ///
    /// The query must be return an open cursor, so for cases like 'insert .. returning'
    /// you will need to use the [execute_returnable](prelude/trait.Execute.html#tymethod.execute_returnable) method instead.
    ///
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
    fn query_first<P, R>(&mut self, sql: &str, params: P) -> Result<Option<R>, FbError>
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

    /// Execute a query that will return data, like the 'insert ... returning ..' or 'execute procedure'.
    ///
    /// This method is designated for use in cases when you don't have
    /// a cursor to fetch. [This link](https://www.ibexpert.net/ibe/pmwiki.php?n=Doc.DataManipulationLanguage#EXECUTEBLOCKStatement)
    /// explain the Firebird behavior for this cases.
    ///
    /// Use `()` for no parameters or a tuple of parameters
    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<R, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static;
}
