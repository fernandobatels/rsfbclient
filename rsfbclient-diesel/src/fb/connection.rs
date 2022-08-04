//! The Firebird connection

use super::backend::Fb;
use super::query_builder::FbQueryBuilder;
use super::transaction::FbTransactionManager;
use super::value::FbRow;
use diesel::connection::*;
use diesel::query_builder::bind_collector::RawBytesBindCollector;
use diesel::query_builder::*;
use diesel::result::Error::DatabaseError;
use diesel::result::*;
use rsfbclient::{Execute, SqlType};
use rsfbclient::{Queryable, Row, SimpleConnection as FbRawConnection};

pub struct FbConnection {
    pub raw: FbRawConnection,
    tr_manager: FbTransactionManager,
}

impl SimpleConnection for FbConnection {
    fn batch_execute(&mut self, query: &str) -> QueryResult<()> {
        self.raw
            .execute(query, ())
            .map_err(|e| DatabaseError(DatabaseErrorKind::Unknown, Box::new(e.to_string())))
            .map(|_| ())
    }
}

impl<'conn, 'query> ConnectionGatWorkaround<'conn, 'query, Fb, DefaultLoadingMode>
    for FbConnection
{
    type Cursor = Box<dyn Iterator<Item = QueryResult<Self::Row>>>;
    type Row = FbRow;
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
            raw: raw.into(),
            tr_manager: FbTransactionManager::new(),
        })
    }

    fn execute_returning_count<T>(&mut self, source: &T) -> QueryResult<usize>
    where
        T: QueryFragment<Self::Backend> + QueryId,
    {
        let mut bc = RawBytesBindCollector::<Fb>::new();
        source.collect_binds(&mut bc, &mut (), &Fb)?;

        let mut qb = FbQueryBuilder::new();
        source.to_sql(&mut qb, &Fb)?;
        let sql = qb.finish();

        let params: Vec<SqlType> = bc
            .metadata
            .into_iter()
            .zip(bc.binds)
            .map(|(tp, val)| tp.into_param(val))
            .collect();

        self.raw
            .execute(&sql, params)
            .map_err(|e| DatabaseError(DatabaseErrorKind::Unknown, Box::new(e.to_string())))
    }

    fn transaction_state(
        &mut self,
    ) -> &mut <Self::TransactionManager as TransactionManager<Self>>::TransactionStateData {
        &mut self.tr_manager
    }
}

trait Helper {
    fn load<'conn, 'query, T>(
        conn: &'conn mut FbConnection,
        source: T,
    ) -> QueryResult<Box<dyn Iterator<Item = QueryResult<FbRow>>>>
    where
        T: Query + QueryFragment<Fb> + QueryId + 'query,
        Fb: diesel::expression::QueryMetadata<T::SqlType>;
}

impl Helper for ()
where
    for<'b> Fb: diesel::backend::HasBindCollector<'b, BindCollector = RawBytesBindCollector<Fb>>,
{
    fn load<'conn, 'query, T>(
        conn: &'conn mut FbConnection,
        source: T,
    ) -> QueryResult<Box<dyn Iterator<Item = QueryResult<FbRow>>>>
    where
        T: Query + QueryFragment<Fb> + QueryId + 'query,
        Fb: diesel::expression::QueryMetadata<T::SqlType>,
    {
        let source = &source.as_query();
        let mut bc = RawBytesBindCollector::<Fb>::new();
        source.collect_binds(&mut bc, &mut (), &Fb)?;

        let mut qb = FbQueryBuilder::new();
        source.to_sql(&mut qb, &Fb)?;
        let has_cursor = qb.has_cursor;
        let sql = qb.finish();

        let params: Vec<SqlType> = bc
            .metadata
            .into_iter()
            .zip(bc.binds)
            .map(|(tp, val)| tp.into_param(val))
            .collect();

        let results = if has_cursor {
            conn.raw.query::<Vec<SqlType>, Row>(&sql, params)
        } else {
            match conn
                .raw
                .execute_returnable::<Vec<SqlType>, Row>(&sql, params)
            {
                Ok(result) => Ok(vec![result]),
                Err(e) => Err(e),
            }
        };

        Ok(Box::new(
            results
                .map_err(|e| DatabaseError(DatabaseErrorKind::Unknown, Box::new(e.to_string())))?
                .into_iter()
                .map(FbRow::new)
                .map(Ok),
        ))
    }
}

impl LoadConnection<DefaultLoadingMode> for FbConnection
where
    // this additional trait is somehow required
    // because rustc fails to understand the bound here
    (): Helper,
{
    fn load<'conn, 'query, T>(
        &'conn mut self,
        source: T,
    ) -> QueryResult<LoadRowIter<'conn, 'query, Self, Self::Backend, DefaultLoadingMode>>
    where
        T: Query + QueryFragment<Self::Backend> + QueryId + 'query,
        Self::Backend: diesel::expression::QueryMetadata<T::SqlType>,
    {
        <() as Helper>::load(self, source)
    }
}

#[cfg(not(any(feature = "dynamic_loading", feature = "embedded_tests")))]
#[cfg(test)]
mod tests {

    use crate::FbConnection;
    use diesel::connection::SimpleConnection;
    use diesel::prelude::*;
    use diesel::result::Error;

    #[test]
    fn establish() -> Result<(), ConnectionError> {
        FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")?;

        Ok(())
    }

    #[test]
    fn execute() -> Result<(), Error> {
        let mut conn =
            FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb").unwrap();

        conn.batch_execute("drop table conn_exec").ok();

        conn.batch_execute("create table conn_exec(id int, name varchar(50))")?;

        let affected_rows =
            diesel::sql_query("insert into conn_exec(id, name) values (10, 'café')")
                .execute(&mut conn)?;
        assert_eq!(1, affected_rows);

        Ok(())
    }
}
