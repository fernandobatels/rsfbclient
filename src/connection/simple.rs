//!
//! A generic connection API. Intended to work with
//! multiple connection types/variations.
//!

use crate::{Connection, Execute, FbError, FromRow, IntoParams, Queryable};
use rsfbclient_core::{StmtType, TransactionConfiguration};

#[cfg(feature = "linking")]
use rsfbclient_native::DynLink;
#[cfg(feature = "dynamic_loading")]
use rsfbclient_native::DynLoad;
#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
use rsfbclient_native::NativeFbClient;
#[cfg(feature = "pure_rust")]
use rsfbclient_rust::RustFbClient;
use std::convert::{From, TryFrom};

/// Access the same transaction while a result row is being processed.
pub trait ArrayReadContext {
    fn read_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
    ) -> Result<Vec<u8>, FbError>;
    fn query_rows(
        &mut self,
        sql: &str,
        params: Vec<crate::SqlType>,
    ) -> Result<Vec<crate::Row>, FbError>;
    fn read_integer_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
    ) -> Result<Vec<i32>, FbError>;
}

impl<C: rsfbclient_core::FirebirdClient> ArrayReadContext for crate::Transaction<'_, C> {
    fn read_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
    ) -> Result<Vec<u8>, FbError> {
        let slice = rsfbclient_core::ArraySlice::new(
            &self.conn.cli.encode_text(relation)?,
            &self.conn.cli.encode_text(field)?,
            bounds,
            element,
        )?;
        self.conn
            .cli
            .read_array(&mut self.conn.handle, &mut self.data.handle, id, &slice)
    }
    fn query_rows(
        &mut self,
        sql: &str,
        params: Vec<crate::SqlType>,
    ) -> Result<Vec<crate::Row>, FbError> {
        self.query(sql, params)
    }

    fn read_integer_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
    ) -> Result<Vec<i32>, FbError> {
        let slice = rsfbclient_core::IntegerArraySlice::new(
            &self.conn.cli.encode_text(relation)?,
            &self.conn.cli.encode_text(field)?,
            bounds,
        )?;
        self.conn
            .cli
            .read_integer_array(&mut self.conn.handle, &mut self.data.handle, id, &slice)
    }
}

/// A SELECT cursor with access to the transaction that owns its ARRAY values.
pub trait QueryCursor {
    fn fetch(&mut self) -> Result<Option<crate::Row>, FbError>;
    fn array_reader(&mut self) -> &mut dyn ArrayReadContext;
}

impl<C: rsfbclient_core::FirebirdClient> QueryCursor
    for crate::statement::StatementFetch<'_, '_, crate::Row, C>
{
    fn fetch(&mut self) -> Result<Option<crate::Row>, FbError> {
        crate::statement::StatementFetch::fetch(self)
    }

    fn array_reader(&mut self) -> &mut dyn ArrayReadContext {
        self.tr
    }
}

fn with_query_cursor<C: rsfbclient_core::FirebirdClient, T>(
    connection: &mut Connection<C>,
    sql: &str,
    params: Vec<crate::SqlType>,
    consume: impl FnOnce(&[String], &mut dyn QueryCursor) -> Result<T, FbError>,
) -> Result<T, FbError> {
    connection.with_transaction(|transaction| {
        let mut statement = crate::Statement::prepare(transaction, sql, false)?;
        if !matches!(
            statement.data.stmt_type,
            StmtType::Select | StmtType::SelectForUpd
        ) {
            return Err("This method requires a SELECT statement".into());
        }
        let columns = statement
            .tr
            .conn
            .cli
            .output_column_names(&statement.data.handle);
        let mut rows = statement.query::<crate::Row, _>(params)?;
        consume(&columns, &mut rows)
    })
}

/// A connection API without client types
pub struct SimpleConnection {
    pub(crate) inner: TypeConnectionContainer,
}

impl SimpleConnection {
    /// Read prepared column names and stream rows from the same SELECT.
    pub fn with_query_cursor<T>(
        &mut self,
        sql: &str,
        consume: impl FnOnce(&[String], &mut dyn QueryCursor) -> Result<T, FbError>,
    ) -> Result<T, FbError> {
        self.with_query_cursor_params(sql, Vec::new(), consume)
    }

    pub fn with_query_cursor_params<T>(
        &mut self,
        sql: &str,
        params: Vec<crate::SqlType>,
        consume: impl FnOnce(&[String], &mut dyn QueryCursor) -> Result<T, FbError>,
    ) -> Result<T, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => with_query_cursor(c, sql, params, consume),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => with_query_cursor(c, sql, params, consume),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => with_query_cursor(c, sql, params, consume),
        }
    }

    pub fn read_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
    ) -> Result<Vec<u8>, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => {
                c.read_array(id, relation, field, bounds, element)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => {
                c.read_array(id, relation, field, bounds, element)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => {
                c.read_array(id, relation, field, bounds, element)
            }
        }
    }

    pub fn put_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
        values: &[u8],
    ) -> Result<rsfbclient_core::ArrayId, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => {
                c.put_array(id, relation, field, bounds, element, values)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => {
                c.put_array(id, relation, field, bounds, element, values)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => {
                c.put_array(id, relation, field, bounds, element, values)
            }
        }
    }

    pub fn read_integer_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
    ) -> Result<Vec<i32>, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => {
                c.read_integer_array(id, relation, field, bounds)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => {
                c.read_integer_array(id, relation, field, bounds)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => {
                c.read_integer_array(id, relation, field, bounds)
            }
        }
    }

    pub fn create_integer_array(
        &mut self,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        values: &[i32],
    ) -> Result<rsfbclient_core::ArrayId, FbError> {
        self.put_integer_array(
            rsfbclient_core::ArrayId::default(),
            relation,
            field,
            bounds,
            values,
        )
    }

    /// Fill a slice of a temporary array in the current transaction.
    /// A zero identifier starts a new temporary array.
    pub fn put_integer_array(
        &mut self,
        temporary: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        values: &[i32],
    ) -> Result<rsfbclient_core::ArrayId, FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => {
                c.put_integer_array(temporary, relation, field, bounds, values)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => {
                c.put_integer_array(temporary, relation, field, bounds, values)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => {
                c.put_integer_array(temporary, relation, field, bounds, values)
            }
        }
    }

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

impl ArrayReadContext for SimpleConnection {
    fn read_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
    ) -> Result<Vec<u8>, FbError> {
        SimpleConnection::read_array(self, id, relation, field, bounds, element)
    }
    fn query_rows(
        &mut self,
        sql: &str,
        params: Vec<crate::SqlType>,
    ) -> Result<Vec<crate::Row>, FbError> {
        self.query(sql, params)
    }

    fn read_integer_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
    ) -> Result<Vec<i32>, FbError> {
        SimpleConnection::read_integer_array(self, id, relation, field, bounds)
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
