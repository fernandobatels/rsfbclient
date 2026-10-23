//!
//! Rust Firebird Client
//!
//! High level api
//!

use rsfbclient_core::{ColumnDesc, FbError, FromRow, IntoParams, StmtPlan};

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

    /// Prepare `sql` (or take it from the statement cache) and describe its
    /// output columns without executing it: the name each has in the result
    /// set, the relation / context alias / field it comes from, and its type.
    ///
    /// Costs one prepare, which the cache then reuses for the next
    /// `query_iter` of the same text. With the pure-rust client the
    /// description itself is free: it is part of the prepare response.
    fn describe(&mut self, sql: &str) -> Result<Vec<ColumnDesc>, FbError>;

    /// The access plan the server would use for `sql` (see [`StmtPlan`]):
    /// the legacy `PLAN` line and, on Firebird 3+, the explained tree.
    ///
    /// Costs one prepare (cached like `describe`) and one info round trip.
    /// The statement is never executed.
    fn plan(&mut self, sql: &str) -> Result<StmtPlan, FbError>;

    /// [`query_iter`](Self::query_iter), plus the plan of the very statement
    /// that runs: read from the prepared statement right before its cursor
    /// opens, so it costs one info round trip on the same connection and no
    /// second prepare. The plan comes back as its own `Result`: a failure to
    /// read it (an old server, a transient info error) does not fail the
    /// query, whose rows are already on their way.
    fn query_iter_with_plan<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<(Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, PlanRead), FbError>
    where
        P: IntoParams,
        R: FromRow + 'static;

    /// [`query`](Self::query), plus the plan of the statement that ran (see
    /// [`query_iter_with_plan`](Self::query_iter_with_plan)).
    fn query_with_plan<P, R>(&mut self, sql: &str, params: P) -> Result<(Vec<R>, PlanRead), FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        let (iter, plan) = self.query_iter_with_plan(sql, params)?;
        let rows = iter.collect::<Result<Vec<R>, FbError>>()?;
        Ok((rows, plan))
    }
}

/// The plan read from a statement just before it ran, or why it could not
/// be: the statement itself ran either way. See
/// [`Queryable::query_iter_with_plan`] and [`Execute::execute_with_plan`].
pub type PlanRead = Result<StmtPlan, FbError>;

/// Implemented for types that can be used to execute sql statements
pub trait Execute {
    /// Execute a query, may or may not commit the changes
    /// and returns the affected rows count
    ///
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<usize, FbError>
    where
        P: IntoParams;

    /// Execute a query that will return data, like the 'insert ... returning ..' or 'execute procedure'.
    ///
    /// This method is designated for use in cases when you don't have
    /// a cursor to fetch. [This link](https://www.ibexpert.net/ibe/pmwiki.php?n=Doc.DataManipulationLanguage#EXECUTEBLOCKStatement)
    /// explain the Firebird behavior for this cases.
    ///
    /// possible values for argument `params`:
    ///
    /// `()`: no parameters,
    ///
    /// `(param0, param1, param2...)`: a tuple of `IntoParam` values corresponding to positional `?` sql parameters
    ///
    /// A struct for which `IntoParams` has been derived ([see there for details](prelude/derive.IntoParams.html))
    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<R, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static;

    /// [`execute`](Self::execute), plus the plan of the statement that ran
    /// (an `UPDATE … WHERE` has one too): read from the prepared statement
    /// right before it executes, one info round trip, no second prepare. As
    /// with [`Queryable::query_iter_with_plan`], a plan that could not be
    /// read does not fail the statement.
    fn execute_with_plan<P>(&mut self, sql: &str, params: P) -> Result<(usize, PlanRead), FbError>
    where
        P: IntoParams;
}
