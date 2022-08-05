//!
//! A generic connection API. Intended to work with
//! multiple connection types/variations.
//!

use crate::{Connection, Execute, FbError, FromRow, IntoParams, Queryable, SimpleTransaction};

#[cfg(feature = "linking")]
use rsfbclient_native::DynLink;
#[cfg(feature = "dynamic_loading")]
use rsfbclient_native::DynLoad;
#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
use rsfbclient_native::NativeFbClient;
#[cfg(feature = "pure_rust")]
use rsfbclient_rust::RustFbClient;
use std::convert::{From, TryFrom};

/// A connection API without client types
pub struct SimpleConnection {
    pub(crate) inner: TypeConnectionContainer,
}

pub(crate) enum TypeConnectionContainer {
    #[cfg(feature = "linking")]
    NativeDynLink(Connection<NativeFbClient<DynLink>>),
    #[cfg(feature = "dynamic_loading")]
    NativeDynLoad(Connection<NativeFbClient<DynLoad>>),
    #[cfg(feature = "pure_rust")]
    PureRust(Connection<RustFbClient>),
    #[cfg(all(
        not(feature = "pure_rust"),
        not(feature = "linking"),
        not(feature = "dynamic_loading")
    ))]
    Phantom,
}

#[cfg(feature = "linking")]
impl From<Connection<NativeFbClient<DynLink>>> for SimpleConnection {
    fn from(conn: Connection<NativeFbClient<DynLink>>) -> Self {
        let inner = TypeConnectionContainer::NativeDynLink(conn);
        SimpleConnection { inner }
    }
}

#[cfg(feature = "linking")]
impl TryFrom<SimpleConnection> for Connection<NativeFbClient<DynLink>> {
    type Error = FbError;

    fn try_from(conn: SimpleConnection) -> Result<Self, Self::Error> {
        #[allow(irrefutable_let_patterns)]
        if let TypeConnectionContainer::NativeDynLink(c) = conn.inner {
            Ok(c)
        } else {
            Err(FbError::from("This isn't a NativeDynLink connection"))
        }
    }
}

#[cfg(feature = "dynamic_loading")]
impl From<Connection<NativeFbClient<DynLoad>>> for SimpleConnection {
    fn from(conn: Connection<NativeFbClient<DynLoad>>) -> Self {
        let inner = TypeConnectionContainer::NativeDynLoad(conn);
        SimpleConnection { inner }
    }
}

#[cfg(feature = "dynamic_loading")]
impl TryFrom<SimpleConnection> for Connection<NativeFbClient<DynLoad>> {
    type Error = FbError;

    fn try_from(conn: SimpleConnection) -> Result<Self, Self::Error> {
        #[allow(irrefutable_let_patterns)]
        if let TypeConnectionContainer::NativeDynLoad(c) = conn.inner {
            Ok(c)
        } else {
            Err(FbError::from("This isn't a NativeDynLoad connection"))
        }
    }
}

#[cfg(feature = "pure_rust")]
impl From<Connection<RustFbClient>> for SimpleConnection {
    fn from(conn: Connection<RustFbClient>) -> Self {
        let inner = TypeConnectionContainer::PureRust(conn);
        SimpleConnection { inner }
    }
}

#[cfg(feature = "pure_rust")]
impl TryFrom<SimpleConnection> for Connection<RustFbClient> {
    type Error = FbError;

    fn try_from(conn: SimpleConnection) -> Result<Self, Self::Error> {
        #[allow(irrefutable_let_patterns)]
        if let TypeConnectionContainer::PureRust(c) = conn.inner {
            Ok(c)
        } else {
            Err(FbError::from("This isn't a PureRust connection"))
        }
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
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
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
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
        }
    }

    /// Run a closure with a transaction, if the closure returns an error
    /// the transaction will rollback, else it will be committed
    pub fn with_transaction<T, F>(&mut self, closure: F) -> Result<T, FbError>
    where
        F: FnOnce(&mut SimpleTransaction) -> Result<T, FbError>,
    {
        let mut tr = SimpleTransaction::new(self)?;

        let res = closure(&mut tr);

        if res.is_ok() {
            tr.commit_retaining()?;
        } else {
            tr.rollback_retaining()?;
        };

        res
    }

    /// Begins a new transaction, and instructs all the `query` and `execute` methods
    /// performed in the [`SimpleConnection`] type to not automatically commit and rollback
    /// until [`commit`][`SimpleConnection::commit`] or [`rollback`][`SimpleConnection::rollback`] are called
    pub fn begin_transaction(&mut self) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.begin_transaction(),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.begin_transaction(),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.begin_transaction(),
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
        }
    }

    /// Commit the default transaction
    pub fn commit(&mut self) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.commit(),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.commit(),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.commit(),
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
        }
    }

    /// Rollback the default transaction
    pub fn rollback(&mut self) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.rollback(),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.rollback(),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.rollback(),
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
        }
    }
}

impl Execute for SimpleConnection {
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<usize, FbError>
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
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
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
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
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
            #[cfg(all(
                not(feature = "pure_rust"),
                not(feature = "linking"),
                not(feature = "dynamic_loading")
            ))]
            TypeConnectionContainer::Phantom => panic!("Choose a connection type feature"),
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

    #[test]
    fn with_transaction() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder()
            .connect()?
            .into();

        conn.with_transaction(|tr| {

            let (a,): (i32,) = tr.query_first(
                "select cast(100 as int) from rdb$database",
                (),
            )?.unwrap();
            assert_eq!(100, a);

            Ok(())
        })?;

        Ok(())
    }
}
