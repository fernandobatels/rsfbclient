//! The Firebird connection

use super::backend::Fb;
use diesel::connection::*;
use diesel::deserialize::*;
use diesel::query_builder::*;
use diesel::result::Error::DatabaseError;
use diesel::result::*;
use diesel::types::HasSqlType;
use rsfb::FirebirdClientFactory;
use rsfbclient as rsfb;
use rsfbclient_native as rsfbn;
use std::cell::RefCell;

type FbRawConnection = rsfb::Connection<rsfbn::NativeFbClient<rsfbn::DynLink>>;

pub struct FbConnection {
    pub raw: RefCell<FbRawConnection>,
}

unsafe impl Send for FbConnection {}

impl SimpleConnection for FbConnection {
    fn batch_execute(&self, query: &str) -> QueryResult<()> {
        use rsfbclient::Execute;

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
        let raw_builder = rsfb::builder_native().with_dyn_link().with_remote();

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
        use rsfbclient::Execute;

        self.raw
            .borrow_mut()
            .execute(query, ())
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))
    }

    fn query_by_index<T, U>(&self, source: T) -> QueryResult<Vec<U>>
    where
        T: AsQuery,
        T::Query: QueryFragment<Self::Backend> + QueryId,
        Self::Backend: HasSqlType<T::SqlType>,
        U: Queryable<T::SqlType, Self::Backend>,
    {
        todo!()
    }

    fn query_by_name<T, U>(&self, source: &T) -> QueryResult<Vec<U>>
    where
        T: QueryFragment<Self::Backend> + QueryId,
        U: QueryableByName<Self::Backend>,
    {
        todo!()
    }

    fn execute_returning_count<T>(&self, source: &T) -> QueryResult<usize>
    where
        T: QueryFragment<Self::Backend> + QueryId,
    {
        todo!()
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
