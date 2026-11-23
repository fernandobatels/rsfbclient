//!
//! A generic connection API. Intended to work with
//! multiple connection types/variations.
//!

use crate::{Connection, Execute, FbError, FromRow, IntoParams, Queryable};
use rsfbclient_core::TransactionConfiguration;

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

impl SimpleConnection {
    /// Borrow the underlying native dyn link connection without consuming it.
    /// Returns `None` for a different client implementation.
    #[cfg(feature = "linking")]
    #[allow(unreachable_patterns)]
    pub fn as_native_dyn_link(&self) -> Option<&Connection<NativeFbClient<DynLink>>> {
        match &self.inner {
            TypeConnectionContainer::NativeDynLink(connection) => Some(connection),
            _ => None,
        }
    }

    /// Borrow the underlying native dyn link connection without consuming it.
    /// Returns `None` for a different client implementation.
    #[cfg(feature = "linking")]
    #[allow(unreachable_patterns)]
    pub fn as_native_dyn_link_mut(&mut self) -> Option<&mut Connection<NativeFbClient<DynLink>>> {
        match &mut self.inner {
            TypeConnectionContainer::NativeDynLink(connection) => Some(connection),
            _ => None,
        }
    }

    /// Borrow the underlying native dyn load connection without consuming it.
    /// Returns `None` for a different client implementation.
    #[cfg(feature = "dynamic_loading")]
    #[allow(unreachable_patterns)]
    pub fn as_native_dyn_load(&self) -> Option<&Connection<NativeFbClient<DynLoad>>> {
        match &self.inner {
            TypeConnectionContainer::NativeDynLoad(connection) => Some(connection),
            _ => None,
        }
    }

    /// Borrow the underlying native dyn load connection without consuming it.
    /// Returns `None` for a different client implementation.
    #[cfg(feature = "dynamic_loading")]
    #[allow(unreachable_patterns)]
    pub fn as_native_dyn_load_mut(&mut self) -> Option<&mut Connection<NativeFbClient<DynLoad>>> {
        match &mut self.inner {
            TypeConnectionContainer::NativeDynLoad(connection) => Some(connection),
            _ => None,
        }
    }

    /// Borrow the underlying pure rust connection without consuming it.
    /// Returns `None` for a different client implementation.
    #[cfg(feature = "pure_rust")]
    #[allow(unreachable_patterns)]
    pub fn as_pure_rust(&self) -> Option<&Connection<RustFbClient>> {
        match &self.inner {
            TypeConnectionContainer::PureRust(connection) => Some(connection),
            _ => None,
        }
    }

    /// Borrow the underlying pure rust connection without consuming it.
    /// Returns `None` for a different client implementation.
    #[cfg(feature = "pure_rust")]
    #[allow(unreachable_patterns)]
    pub fn as_pure_rust_mut(&mut self) -> Option<&mut Connection<RustFbClient>> {
        match &mut self.inner {
            TypeConnectionContainer::PureRust(connection) => Some(connection),
            _ => None,
        }
    }
    /// Check an idle remote attachment without issuing SQL. On timeout the
    /// attachment is aborted because its unread response cannot be reused.
    /// Only call on TCP connections, with exclusive ownership for this call.
    pub fn probe_remote(&mut self, timeout: std::time::Duration) -> Result<(), FbError> {
        use std::sync::mpsc::{self, RecvTimeoutError};

        // The scope joins the watchdog before the connection can be dropped
        // or the caller can start a different operation on it.
        // An older native client may lack the asynchronous abort API.
        let Ok(interrupt) = (unsafe { self.disposable_interrupt() }) else {
            return Ok(());
        };
        std::thread::scope(|scope| {
            let (done, pending) = mpsc::channel::<()>();
            let watchdog = scope.spawn(move || {
                if pending.recv_timeout(timeout) == Err(RecvTimeoutError::Timeout) {
                    let _ = interrupt();
                    return true;
                }
                false
            });
            let result = match &mut self.inner {
                #[cfg(feature = "linking")]
                TypeConnectionContainer::NativeDynLink(connection) => {
                    connection.cli.probe_attachment(&mut connection.handle)
                }
                #[cfg(feature = "dynamic_loading")]
                TypeConnectionContainer::NativeDynLoad(connection) => {
                    connection.cli.probe_attachment(&mut connection.handle)
                }
                #[cfg(feature = "pure_rust")]
                TypeConnectionContainer::PureRust(connection) => {
                    connection.cli.probe_attachment(&mut connection.handle)
                }
            };
            drop(done);
            if watchdog
                .join()
                .map_err(|_| "Connection check watchdog failed")?
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Firebird did not answer the connection check",
                )
                .into());
            }
            result
        })
    }
}

pub(crate) enum TypeConnectionContainer {
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

    /// Prepare a statement without executing it and return its server classification.
    pub fn prepare_statement_type(
        &mut self,
        sql: &str,
    ) -> Result<rsfbclient_core::StmtType, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.prepare_statement_type(sql),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.prepare_statement_type(sql),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.prepare_statement_type(sql),
        }
    }

    /// Prepare and describe a statement without opening a cursor.
    ///
    /// Set `named_params` to parse named parameters as in [`Transaction::prepare`](crate::Transaction::prepare).
    pub fn describe_statement(
        &mut self,
        sql: &str,
        named_params: bool,
    ) -> Result<rsfbclient_core::StatementDescription, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.describe_statement(sql, named_params),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.describe_statement(sql, named_params),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.describe_statement(sql, named_params),
        }
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
        }
    }

    /// Begins a new transaction with a custom transaction configuration, and instructs
    /// all the `query` and `execute` methods performed in the [`SimpleConnection`] type
    /// to not automatically commit and rollback until [`commit`][`SimpleConnection::commit`]
    /// or [`rollback`][`SimpleConnection::rollback`] are called
    pub fn begin_transaction_config(
        &mut self,
        confs: TransactionConfiguration,
    ) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.begin_transaction_config(confs),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.begin_transaction_config(confs),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.begin_transaction_config(confs),
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
        }
    }

    /// Wait for an event to be posted on database
    pub fn wait_for_event(&mut self, name: String) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c.wait_for_event(name),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c.wait_for_event(name),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c.wait_for_event(name),
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
