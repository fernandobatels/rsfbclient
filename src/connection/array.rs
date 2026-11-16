//! ARRAY slices accessed through the transaction that owns them.

use super::{simple::TypeConnectionContainer, Connection, SimpleConnection};
use crate::{FbError, Queryable};
use rsfbclient_core::FirebirdClient;

/// Access ARRAY slices through an existing connection and transaction.
///
/// Obtain this handle from [`Connection::arrays`] or [`SimpleConnection::arrays`].
pub struct Array<'a, C = SimpleConnection> {
    connection: &'a mut C,
}

impl<C: FirebirdClient> Connection<C> {
    /// Access ARRAY values in the explicitly active transaction.
    pub fn arrays(&mut self) -> Array<'_, Self> {
        Array { connection: self }
    }
}

impl SimpleConnection {
    /// Access ARRAY values in the explicitly active transaction.
    pub fn arrays(&mut self) -> Array<'_, Self> {
        Array { connection: self }
    }
}

impl<C: FirebirdClient> Array<'_, Connection<C>> {
    pub fn read_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
    ) -> Result<Vec<u8>, FbError> {
        if !self.connection.in_transaction {
            return Err(
                "An ARRAY slice requires the active transaction that fetched its identifier".into(),
            );
        }
        self.connection.with_transaction(|tr| {
            ArrayReadContext::read_array(tr, id, relation, field, bounds, element)
        })
    }

    pub fn put_array(
        &mut self,
        temporary: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
        values: &[u8],
    ) -> Result<rsfbclient_core::ArrayId, FbError> {
        if temporary.high != 0 {
            return Err("ARRAY slice writes require a new or temporary array identifier".into());
        }
        if !self.connection.in_transaction {
            return Err("An ARRAY write requires an active transaction".into());
        }
        let slice = rsfbclient_core::ArraySlice::new(
            &self.connection.cli.encode_text(relation)?,
            &self.connection.cli.encode_text(field)?,
            bounds,
            element,
        )?;
        slice.validate(values)?;
        self.connection.with_transaction(|tr| {
            tr.conn.cli.put_array(
                &mut tr.conn.handle,
                &mut tr.data.handle,
                temporary,
                &slice,
                values,
            )
        })
    }

    pub fn read_integer_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
    ) -> Result<Vec<i32>, FbError> {
        if !self.connection.in_transaction {
            return Err(
                "An ARRAY slice requires the active transaction that fetched its identifier".into(),
            );
        }
        self.connection.with_transaction(|tr| {
            ArrayReadContext::read_integer_array(tr, id, relation, field, bounds)
        })
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
        if temporary.high != 0 {
            return Err("ARRAY slice writes require a new or temporary array identifier".into());
        }
        if !self.connection.in_transaction {
            return Err("An ARRAY write requires an active transaction".into());
        }
        let slice = rsfbclient_core::IntegerArraySlice::new(
            &self.connection.cli.encode_text(relation)?,
            &self.connection.cli.encode_text(field)?,
            bounds,
        )?;
        self.connection.with_transaction(|tr| {
            tr.conn.cli.put_integer_array(
                &mut tr.conn.handle,
                &mut tr.data.handle,
                temporary,
                &slice,
                values,
            )
        })
    }
}

impl Array<'_> {
    pub fn read_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
        element: rsfbclient_core::ArrayElement,
    ) -> Result<Vec<u8>, FbError> {
        match &mut self.connection.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => {
                c.arrays().read_array(id, relation, field, bounds, element)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => {
                c.arrays().read_array(id, relation, field, bounds, element)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => {
                c.arrays().read_array(id, relation, field, bounds, element)
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
        match &mut self.connection.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c
                .arrays()
                .put_array(id, relation, field, bounds, element, values),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c
                .arrays()
                .put_array(id, relation, field, bounds, element, values),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c
                .arrays()
                .put_array(id, relation, field, bounds, element, values),
        }
    }

    pub fn read_integer_array(
        &mut self,
        id: rsfbclient_core::ArrayId,
        relation: &str,
        field: &str,
        bounds: &[(i32, i32)],
    ) -> Result<Vec<i32>, FbError> {
        match &mut self.connection.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => {
                c.arrays().read_integer_array(id, relation, field, bounds)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => {
                c.arrays().read_integer_array(id, relation, field, bounds)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => {
                c.arrays().read_integer_array(id, relation, field, bounds)
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
        match &mut self.connection.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(c) => c
                .arrays()
                .put_integer_array(temporary, relation, field, bounds, values),
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(c) => c
                .arrays()
                .put_integer_array(temporary, relation, field, bounds, values),
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(c) => c
                .arrays()
                .put_integer_array(temporary, relation, field, bounds, values),
        }
    }
}

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
