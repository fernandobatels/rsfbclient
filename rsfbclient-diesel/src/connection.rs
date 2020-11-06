//! The Firebird connection

use super::backend::Fb;
use super::query_builder::FbQueryBuilder;
use super::value::FbRow;
use diesel::connection::*;
use diesel::deserialize::*;
use diesel::expression::QueryMetadata;
use diesel::query_builder::bind_collector::RawBytesBindCollector;
use diesel::query_builder::*;
use diesel::result::Error::DatabaseError;
use diesel::result::Error::DeserializationError;
use diesel::result::*;
use rsfbclient::Queryable;
use rsfbclient::{Execute, FirebirdClientFactory, SqlType};
use rsfbclient_native::*;
use std::cell::RefCell;

type FbRawConnection = rsfbclient::Connection<NativeFbClient<DynLink>>;

pub struct FbConnection {
    pub raw: RefCell<FbRawConnection>,
}

unsafe impl Send for FbConnection {}

impl SimpleConnection for FbConnection {
    fn batch_execute(&self, query: &str) -> QueryResult<()> {
        self.raw
            .borrow_mut()
            .execute(query, ())
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))
            .map(|_| ())
    }
}

#[allow(unused_variables)]
impl Connection for FbConnection {
    type TransactionManager = AnsiTransactionManager;
    type Backend = Fb;

    fn establish(database_url: &str) -> ConnectionResult<Self> {
        let raw_builder = rsfbclient::builder_native().with_dyn_link().with_remote();

        let raw = FbRawConnection::open(
            raw_builder.new_instance().unwrap(), //note this can fail for dyn load if the lib isn't found
            raw_builder.get_conn_conf(),
        )
        .map_err(|e| ConnectionError::BadConnection(e.to_string()))
        .unwrap();

        Ok(FbConnection {
            raw: RefCell::new(raw),
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
        let sql = qb.finish();

        let params: Vec<SqlType> = bc
            .metadata
            .into_iter()
            .zip(bc.binds)
            .map(|(tp, val)| tp.to_param(val))
            .collect();

        self.raw
            .borrow_mut()
            .query::<Vec<SqlType>, FbRow>(&sql, params)
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?
            .iter()
            .map(|row| U::build_from_row(row).map_err(DeserializationError))
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
            .map(|(tp, val)| tp.to_param(val))
            .collect();

        self.raw
            .borrow_mut()
            .execute(&sql, params)
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))
    }

    fn transaction_manager(&self) -> &Self::TransactionManager {
        todo!()
    }
}

#[cfg(test)]
mod tests {

    use crate::connection::FbConnection;
    use diesel::connection::SimpleConnection;
    use diesel::prelude::*;
    use diesel::result::Error;

    #[test]
    fn establish() -> Result<(), ConnectionError> {
        FbConnection::establish("teste")?;

        Ok(())
    }

    #[test]
    fn execute() -> Result<(), Error> {
        let conn = FbConnection::establish("teste").unwrap();

        conn.batch_execute("drop table conn_exec").ok();

        conn.batch_execute("create table conn_exec(id int, name varchar(50))")?;

        let affected_rows = conn.execute("insert into conn_exec(id, name) values (10, 'café')")?;
        assert_eq!(1, affected_rows);

        Ok(())
    }
}
