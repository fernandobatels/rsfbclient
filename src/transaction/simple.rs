//!
//! A generic transaction API. Intended to work with
//! multiple connection types/variations.
//!

use crate::{Transaction, Execute, FbError, FromRow, IntoParams, Queryable, SimpleConnection};
use crate::connection::simple::TypeConnectionContainer;
#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
use rsfbclient_native::NativeFbClient;
#[cfg(feature = "linking")]
use rsfbclient_native::DynLink;
#[cfg(feature = "dynamic_loading")]
use rsfbclient_native::DynLoad;
#[cfg(feature = "pure_rust")]
use rsfbclient_rust::RustFbClient;
use std::convert::From;

/// A transaction API without client types
pub struct SimpleTransaction<'c> {
    inner: TypeTransactionContainer<'c>,
}

enum TypeTransactionContainer<'c> {
    #[cfg(feature = "linking")]
    NativeDynLink(Transaction<'c, NativeFbClient<DynLink>>),
    #[cfg(feature = "dynamic_loading")]
    NativeDynLoad(Transaction<'c, NativeFbClient<DynLoad>>),
    #[cfg(feature = "pure_rust")]
    PureRust(Transaction<'c, RustFbClient>),
}

#[cfg(feature = "linking")]
impl<'c> From<Transaction<'c, NativeFbClient<DynLink>>> for SimpleTransaction<'c> {
    fn from(tr: Transaction<'c, NativeFbClient<DynLink>>) -> Self {
        let inner = TypeTransactionContainer::NativeDynLink(tr);
        SimpleTransaction { inner }
    }
}

#[cfg(feature = "linking")]
impl<'c> From<SimpleTransaction<'c>> for Result<Transaction<'c, NativeFbClient<DynLink>>, FbError> {
    fn from(tr: SimpleTransaction<'c>) -> Self {
        #[allow(irrefutable_let_patterns)]
        if let TypeTransactionContainer::NativeDynLink(t) = tr.inner {
            Ok(t)
        } else {
            Err(FbError::from("This isn't a NativeDynLink transaction"))
        }
    }
}

#[cfg(feature = "dynamic_loading")]
impl<'c> From<Transaction<'c, NativeFbClient<DynLoad>>> for SimpleTransaction<'c> {
    fn from(tr: Transaction<'c, NativeFbClient<DynLoad>>) -> Self {
        let inner = TypeTransactionContainer::NativeDynLoad(tr);
        SimpleTransaction { inner }
    }
}

#[cfg(feature = "dynamic_loading")]
impl<'c> From<SimpleTransaction<'c>> for Result<Transaction<'c, NativeFbClient<DynLoad>>, FbError> {
    fn from(tr: SimpleTransaction<'c>) -> Self {
        #[allow(irrefutable_let_patterns)]
        if let TypeTransactionContainer::NativeDynLoad(t) = tr.inner {
            Ok(t)
        } else {
            Err(FbError::from("This isn't a NativeDynLoad transaction"))
        }
    }
}

#[cfg(feature = "pure_rust")]
impl<'c> From<Transaction<'c, RustFbClient>> for SimpleTransaction<'c> {
    fn from(tr: Transaction<'c, RustFbClient>) -> Self {
        let inner = TypeTransactionContainer::PureRust(tr);
        SimpleTransaction { inner }
    }
}

#[cfg(feature = "pure_rust")]
impl<'c> From<SimpleTransaction<'c>> for Result<Transaction<'c, RustFbClient>, FbError> {
    fn from(tr: SimpleTransaction<'c>) -> Self {
        #[allow(irrefutable_let_patterns)]
        if let TypeTransactionContainer::PureRust(t) = tr.inner {
            Ok(t)
        } else {
            Err(FbError::from("This isn't a PureRust transaction"))
        }
    }
}

impl<'c> SimpleTransaction<'c> {

    /// Start a new transaction
    pub fn new(conn: &'c mut SimpleConnection) -> Result<Self, FbError> {
        match &mut conn.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(tr) => Ok(Transaction::new(tr)?.into()),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(tr) => Ok(Transaction::new(tr)?.into()),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(tr) => Ok(Transaction::new(tr)?.into()),
        }
    }

    /// Commit the current transaction changes
    pub fn commit(self) -> Result<(), FbError> {
        match self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.commit(),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.commit(),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.commit(),
        }
    }

    /// Commit the current transaction changes, but allowing to reuse the transaction
    pub fn commit_retaining(&mut self) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.commit_retaining(),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.commit_retaining(),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.commit_retaining(),
        }
    }

    /// Rollback the current transaction changes, but allowing to reuse the transaction
    pub fn rollback_retaining(&mut self) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.rollback_retaining(),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.rollback_retaining(),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.rollback_retaining(),
        }
    }

    /// Rollback the current transaction changes
    pub fn rollback(self) -> Result<(), FbError> {
        match self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.rollback(),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.rollback(),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.rollback(),
        }
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(&mut self, sql: &str) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.execute_immediate(sql),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.execute_immediate(sql),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.execute_immediate(sql),
        }
    }

    // TODO: add the prepare() method
}

impl<'c> Execute for SimpleTransaction<'c> {
    fn execute<P>(&mut self, sql: &str, params: P) -> Result<(), FbError>
    where
        P: IntoParams,
    {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.execute(sql, params),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.execute(sql, params),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.execute(sql, params),
        }
    }

    fn execute_returnable<P, R>(&mut self, sql: &str, params: P) -> Result<R, FbError>
    where
        P: IntoParams,
        R: FromRow + 'static,
    {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeTransactionContainer::NativeDynLink(tr) => tr.execute_returnable(sql, params),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.execute_returnable(sql, params),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.execute_returnable(sql, params),
        }
    }
}

impl<'c> Queryable for SimpleTransaction<'c> {
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
            TypeTransactionContainer::NativeDynLink(tr) => tr.query_iter(sql, params),
            #[cfg(feature = "dynamic_loading")]
            TypeTransactionContainer::NativeDynLoad(tr) => tr.query_iter(sql, params),
            #[cfg(feature = "pure_rust")]
            TypeTransactionContainer::PureRust(tr) => tr.query_iter(sql, params),
        }
    }
}

#[cfg(test)]
mk_tests_default! {
    use crate::*;

    #[test]
    fn new() -> Result<(), FbError> {
        let mut conn = cbuilder()
            .connect()?
            .into();

        SimpleTransaction::new(&mut conn)?;

        conn.close()?;

        Ok(())
    }

    #[test]
    fn execute() -> Result<(), FbError> {
        let mut conn = cbuilder()
            .connect()?
            .into();

        let mut tr = SimpleTransaction::new(&mut conn)?;

        tr.execute("DROP TABLE SIMPLE_TR_EXEC_TEST", ()).ok();
        tr.execute("CREATE TABLE SIMPLE_TR_EXEC_TEST (id int)", ())?;
        tr.commit_retaining()?;

        let returning: (i32,) = tr.execute_returnable("insert into SIMPLE_TR_EXEC_TEST (id) values (10) returning id", ())?;
        assert_eq!((10,), returning);

        Ok(())
    }

    #[test]
    fn query() -> Result<(), FbError> {
        let mut conn = cbuilder()
            .connect()?
            .into();

        let mut tr = SimpleTransaction::new(&mut conn)?;

        let (a,): (i32,) = tr.query_first(
                "select cast(100 as int) from rdb$database",
                (),
            )?.unwrap();
        assert_eq!(100, a);

        Ok(())
    }
}
