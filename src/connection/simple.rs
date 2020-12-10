//!
//! A generic connection API. Intended to work with
//! multiple connection types/variations.
//!

use crate::{Connection, Execute, FbError, FromRow, IntoParams, Queryable};
use rsfbclient_native::{DynLink, DynLoad, NativeFbClient};
use rsfbclient_rust::RustFbClient;
use std::convert::From;

/// A connection API without client types
pub struct SimpleConnection {
    inner: TypeConnectionContainer,
}

enum TypeConnectionContainer {
    #[cfg(feature = "linking")]
    NativeDynLink(Connection<NativeFbClient<DynLink>>),
    #[cfg(feature = "dynamic_loading")]
    NativeDynLoad(Connection<NativeFbClient<DynLoad>>),
    #[cfg(feature = "pure_rust")]
    PureRust(Connection<RustFbClient>),
}

#[cfg(feature = "linking")]
impl From<Connection<NativeFbClient<DynLink>>> for SimpleConnection {
    fn from(conn: Connection<NativeFbClient<DynLink>>) -> Self {
        let inner = TypeConnectionContainer::NativeDynLink(conn);
        SimpleConnection { inner }
    }
}

#[cfg(feature = "dynamic_loading")]
impl From<Connection<NativeFbClient<DynLoad>>> for SimpleConnection {
    fn from(conn: Connection<NativeFbClient<DynLoad>>) -> Self {
        let inner = TypeConnectionContainer::NativeDynLoad(conn);
        SimpleConnection { inner }
    }
}

#[cfg(feature = "pure_rust")]
impl From<Connection<RustFbClient>> for SimpleConnection {
    fn from(conn: Connection<RustFbClient>) -> Self {
        let inner = TypeConnectionContainer::PureRust(conn);
        SimpleConnection { inner }
    }
}

impl SimpleConnection {
    /// Drop the current database
    pub fn drop_database(self) -> Result<(), FbError> {
        match self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.drop_database(),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.drop_database(),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.drop_database(),
        }
    }

    /// Close the current connection.
    pub fn close(self) -> Result<(), FbError> {
        match self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.close(),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.close(),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.close(),
        }
    }
}

impl Execute for SimpleConnection {
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams,
    {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.execute(sql, params),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.execute(sql, params),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.execute(sql, params),
        }
    }

    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<R, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.execute_returnable(sql, params),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.execute_returnable(sql, params),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.execute_returnable(sql, params),
        }
    }
}

impl Queryable for SimpleConnection {
    fn query_iter<'a, P, R>(
        &'a mut self,
        sql: &str,
        params: P,
    ) -> Result<Box<dyn Iterator<Item = Result<R, FbError>> + 'a>, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.query_iter(sql, params),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.query_iter(sql, params),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.query_iter(sql, params),
        }
    }
}

#[cfg(test)]
mk_tests_default! {
    use crate::*;

    #[test]
    fn into() -> Result<(), FbError> {
        let conn: SimpleConnection = cbuilder()
            .connect()?
            .into();

        conn.close()?;

        Ok(())
    }

    #[test]
    fn execute() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder()
            .connect()?
            .into();

        conn.execute("DROP TABLE SIMPLE_CONN_EXEC_TEST", ()).ok();
        conn.execute("CREATE TABLE SIMPLE_CONN_EXEC_TEST (id int)", ())?;

        let returning: (i32,) = conn.execute_returnable("insert into SIMPLE_CONN_EXEC_TEST (id) values (10) returning id", ())?;
        assert_eq!((10,), returning);

        Ok(())
    }

    #[test]
    fn query() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder()
            .connect()?
            .into();

        let (a,): (i32,) = conn.query_first(
                "select cast(100 as int) from rdb$database",
                (),
            )?.unwrap();
        assert_eq!(100, a);

        Ok(())
    }
}
