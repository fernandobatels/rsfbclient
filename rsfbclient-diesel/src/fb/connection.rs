//! The Firebird connection

use super::backend::Fb;
use super::query_builder::FbQueryBuilder;
use super::transaction::FbTransactionManager;
use super::value::FbRow;
use diesel::connection::*;
use diesel::deserialize::*;
use diesel::expression::QueryMetadata;
use diesel::query_builder::bind_collector::RawBytesBindCollector;
use diesel::query_builder::*;
use diesel::result::Error::DatabaseError;
use diesel::result::Error::DeserializationError;
use diesel::result::*;
use rsfbclient::SimpleConnection as FbRawConnection;
use rsfbclient::{Execute, Queryable, Row, SqlType};
use std::cell::RefCell;

pub struct FbConnection {
    pub raw: RefCell<FbRawConnection>,
    tr_manager: FbTransactionManager,
}

impl SimpleConnection for FbConnection {
    fn batch_execute(&self, query: &str) -> QueryResult<()> {
        self.raw
            .borrow_mut()
            .execute(query, ())
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))
            .map(|_| ())
    }
}

impl Connection for FbConnection {
    type TransactionManager = FbTransactionManager;
    type Backend = Fb;

    fn establish(database_url: &str) -> ConnectionResult<Self> {
        #[cfg(all(
            feature = "pure_rust",
            not(any(feature = "linking", feature = "dynamic_loading"))
        ))]
        let mut raw_builder = rsfbclient::builder_pure_rust();
        #[cfg(any(feature = "linking", feature = "dynamic_loading"))]
        let raw_builder = rsfbclient::builder_native();

        let raw = raw_builder
            .from_string(database_url)
            .map_err(|e| ConnectionError::BadConnection(e.to_string()))?
            .connect()
            .map_err(|e| ConnectionError::BadConnection(e.to_string()))?;

        Ok(FbConnection {
            raw: RefCell::new(raw.into()),
            tr_manager: FbTransactionManager::new(),
        })
    }

    fn execute(&self, query: &str) -> QueryResult<usize> {
        self.raw
            .borrow_mut()
            .execute(query, ())
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))
    }

    fn load<T, U>(&self, source: T) -> QueryResult<Vec<U>>
    where
        T: AsQuery,
        T::Query: QueryFragment<Self::Backend> + QueryId,
        U: FromSqlRow<T::SqlType, Self::Backend>,
        Self::Backend: QueryMetadata<T::SqlType>,
    {
        let source = &source.as_query();
        let mut bc = RawBytesBindCollector::<Fb>::new();
        source.collect_binds(&mut bc, &())?;

        let mut qb = FbQueryBuilder::new();
        source.to_sql(&mut qb)?;
        let has_cursor = qb.has_cursor;
        let sql = qb.finish();

        let params: Vec<SqlType> = bc
            .metadata
            .into_iter()
            .zip(bc.binds)
            .map(|(tp, val)| tp.into_param(val))
            .collect();

        let results = if has_cursor {
            self.raw
                .borrow_mut()
                .query::<Vec<SqlType>, Row>(&sql, params)
        } else {
            match self
                .raw
                .borrow_mut()
                .execute_returnable::<Vec<SqlType>, Row>(&sql, params)
            {
                Ok(result) => Ok(vec![result]),
                Err(e) => Err(e),
            }
        };

        results
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?
            .iter()
            .map(|row| U::build_from_row(&FbRow::new(row)).map_err(DeserializationError))
            .collect()
    }

    fn execute_returning_count<T>(&self, source: &T) -> QueryResult<usize>
    where
        T: QueryFragment<Self::Backend> + QueryId,
    {
        let mut bc = RawBytesBindCollector::<Fb>::new();
        source.collect_binds(&mut bc, &())?;

        let mut qb = FbQueryBuilder::new();
        source.to_sql(&mut qb)?;
        let sql = qb.finish();

        let params: Vec<SqlType> = bc
            .metadata
            .into_iter()
            .zip(bc.binds)
            .map(|(tp, val)| tp.into_param(val))
            .collect();

        self.raw
            .borrow_mut()
            .execute(&sql, params)
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))
    }

    fn transaction_manager(&self) -> &Self::TransactionManager {
        &self.tr_manager
    }
}

#[cfg(not(any(feature = "dynamic_loading", feature = "embedded_tests")))]
#[cfg(test)]
mod tests {

    use crate::connection::SimpleConnection;
    use crate::fb::FbConnection;
    use crate::prelude::*;
    use crate::result::Error;

    #[test]
    fn establish() -> Result<(), ConnectionError> {
        FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")?;

        Ok(())
    }

    #[test]
    fn execute() -> Result<(), Error> {
        let conn =
            FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb").unwrap();

        conn.batch_execute("drop table conn_exec").ok();

        conn.batch_execute("create table conn_exec(id int, name varchar(50))")?;

        let affected_rows = conn.execute("insert into conn_exec(id, name) values (10, 'café')")?;
        assert_eq!(1, affected_rows);

        Ok(())
    }
}
