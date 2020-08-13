//!
//! Rust Firebird Client
//!
//! Preparation and execution of statements
//!

use crate::{
    transaction::{Transaction, TransactionData},
    Connection,
};
use rsfbclient_core::{Column, FbError, FirebirdClient, FreeStmtOp, FromRow, IntoParams, StmtType};

pub struct Statement<'c, C: FirebirdClient> {
    pub(crate) data: StatementData<C::StmtHandle>,
    pub(crate) conn: &'c Connection<C>,
}

impl<'c, C> Statement<'c, C>
where
    C: FirebirdClient,
{
    /// Prepare the statement that will be executed
    pub fn prepare(tr: &mut Transaction<'c, C>, sql: &str) -> Result<Self, FbError> {
        let data = StatementData::prepare(tr.conn, &mut tr.data, sql)?;

        Ok(Statement {
            data,
            conn: tr.conn,
        })
    }

    /// Execute the current statement without returnig any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute<T>(&mut self, tr: &mut Transaction<C>, params: T) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        self.data.execute(self.conn, &mut tr.data, params)
    }

    /// Execute the current statement
    /// and returns the lines founds
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn query<'s, R, P>(
        &'s mut self,
        tr: &'s mut Transaction<C>,
        params: P,
    ) -> Result<StatementFetch<'s, R, C>, FbError>
    where
        R: FromRow,
        P: IntoParams,
    {
        self.data.query(self.conn, &mut tr.data, params)?;

        Ok(StatementFetch {
            stmt: &mut self.data,
            _tr: tr,
            conn: self.conn,
            _marker: Default::default(),
        })
    }
}

impl<C> Drop for Statement<'_, C>
where
    C: FirebirdClient,
{
    fn drop(&mut self) {
        self.data.close(self.conn).ok();
    }
}
/// Cursor to fetch the results of a statement
pub struct StatementFetch<'s, R, C: FirebirdClient> {
    pub(crate) stmt: &'s mut StatementData<C::StmtHandle>,
    /// Transaction needs to be alive for the fetch to work
    pub(crate) _tr: &'s Transaction<'s, C>,
    pub(crate) conn: &'s Connection<C>,
    /// Type to convert the rows
    _marker: std::marker::PhantomData<R>,
}

// TODO: Make it an iterator directly
impl<'s, R, C> StatementFetch<'s, R, C>
where
    R: FromRow,
    C: FirebirdClient,
{
    /// Fetch for the next row
    pub fn fetch(&mut self) -> Result<Option<R>, FbError> {
        self.stmt
            .fetch(self.conn, &self._tr.data)
            .and_then(|row| row.map(FromRow::try_from).transpose())
    }
}

impl<T, C> Iterator for StatementFetch<'_, T, C>
where
    T: FromRow,
    C: FirebirdClient,
{
    type Item = Result<T, FbError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.fetch().transpose()
    }
}

impl<R, C> Drop for StatementFetch<'_, R, C>
where
    C: FirebirdClient,
{
    fn drop(&mut self) {
        self.stmt.close_cursor(&self.conn).ok();
    }
}

/// Low level statement handler.
///
/// Needs to be closed calling `close` before dropping.
pub struct StatementData<H> {
    pub(crate) handle: H,
    pub(crate) stmt_type: StmtType,
}

impl<H> StatementData<H>
where
    H: Send + Clone + Copy,
{
    /// Prepare the statement that will be executed
    pub fn prepare<C>(
        conn: &Connection<C>,
        tr: &mut TransactionData<C::TrHandle>,
        sql: &str,
    ) -> Result<Self, FbError>
    where
        C: FirebirdClient<StmtHandle = H>,
    {
        let (stmt_type, handle) =
            conn.cli
                .borrow_mut()
                .prepare_statement(conn.handle, tr.handle, conn.dialect, sql)?;

        Ok(Self { stmt_type, handle })
    }

    /// Execute the current statement without returnig any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute<T, C>(
        &mut self,
        conn: &Connection<C>,
        tr: &mut TransactionData<C::TrHandle>,
        params: T,
    ) -> Result<(), FbError>
    where
        T: IntoParams,
        C: FirebirdClient<StmtHandle = H>,
    {
        conn.cli
            .borrow_mut()
            .execute(tr.handle, self.handle, params.to_params())?;

        if self.stmt_type == StmtType::Select {
            // Close the cursor, as it will not be used
            self.close_cursor(conn)?;
        }

        Ok(())
    }

    /// Execute the current statement
    /// and returns the column buffer
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn query<'s, T, C>(
        &'s mut self,
        conn: &'s Connection<C>,
        tr: &mut TransactionData<C::TrHandle>,
        params: T,
    ) -> Result<(), FbError>
    where
        T: IntoParams,
        C: FirebirdClient<StmtHandle = H>,
    {
        conn.cli
            .borrow_mut()
            .execute(tr.handle, self.handle, params.to_params())
    }

    /// Fetch for the next row, needs to be called after `query`
    pub fn fetch<C>(
        &mut self,
        conn: &Connection<C>,
        tr: &TransactionData<C::TrHandle>,
    ) -> Result<Option<Vec<Column>>, FbError>
    where
        C: FirebirdClient<StmtHandle = H>,
    {
        conn.cli
            .borrow_mut()
            .fetch(conn.handle, tr.handle, self.handle)
    }

    /// Closes the statement cursor, if it was open
    pub fn close_cursor<C>(&mut self, conn: &Connection<C>) -> Result<(), FbError>
    where
        C: FirebirdClient<StmtHandle = H>,
    {
        conn.cli
            .borrow_mut()
            .free_statement(self.handle, FreeStmtOp::Close)
    }

    /// Closes the statement
    pub fn close<C>(&mut self, conn: &Connection<C>) -> Result<(), FbError>
    where
        C: FirebirdClient<StmtHandle = H>,
    {
        conn.cli
            .borrow_mut()
            .free_statement(self.handle, FreeStmtOp::Drop)
    }
}

#[cfg(test)]
/// Counter to allow the tests to be run in parallel without interfering in each other
static TABLE_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(test)]
mk_tests_default! {
    use crate::{prelude::*, Connection, Row, Transaction};
    use rsfbclient_core::FirebirdClient;

    #[test]
    fn statements() {
        let conn1 = connect();

        let conn2 = connect();

        let mut t1c1 = Transaction::new(&conn1).unwrap();
        let mut t2c2 = Transaction::new(&conn2).unwrap();
        let mut t3c1 = Transaction::new(&conn1).unwrap();

        println!("T1 {}", t1c1.data.handle);
        println!("T2 {}", t2c2.data.handle);
        println!("T3 {}", t3c1.data.handle);

        let mut stmt = t1c1.prepare("SELECT 1 FROM RDB$DATABASE").unwrap();

        stmt.execute(&mut t1c1, ())
            .expect("Error on execute with t1 from conn1");

        stmt.execute(&mut t2c2, ())
            .expect_err("Can't use a transaction from conn2 in a statement of the conn1");

        stmt.execute(&mut t3c1, ())
            .expect("Error on execute with t3 from conn1");
    }

    #[test]
    fn new_api_select() {
        let (mut conn, table) = setup();

        let vals = vec![
            (Some(2), "coffee".to_string()),
            (Some(3), "milk".to_string()),
            (None, "fail coffee".to_string()),
        ];

        conn.with_transaction(|tr| {
            for val in &vals {
                tr.execute(&format!("insert into {} (id, name) values (?, ?)", table), val.clone())
                    .expect("Error on insert");
            }

            Ok(())
        })
        .expect("Error commiting the transaction");

        let rows = conn
            .query(&format!("select id, name from {}", table), ())
            .expect("Error executing query");

        // Asserts that all values are equal
        assert_eq!(vals, rows);
    }

    #[test]
    fn old_api_select() {
        let (conn, table) = setup();

        let vals = vec![
            (Some(2), "coffee".to_string()),
            (Some(3), "milk".to_string()),
            (None, "fail coffee".to_string()),
        ];

        conn.with_transaction(|tr| {
            let mut stmt = tr
                .prepare(&format!("insert into {} (id, name) values (?, ?)", table))
                .expect("Error preparing the insert statement");

            for val in &vals {
                stmt.execute(tr, val.clone()).expect("Error on insert");
            }

            Ok(())
        })
        .expect("Error commiting the transaction");

        conn.with_transaction(|tr| {
            let mut stmt = tr
                .prepare(&format!("select id, name from {}", table))
                .expect("Error on prepare the select");

            let rows: Vec<(Option<i32>, String)> = stmt
                .query(tr, ())
                .expect("Error on query")
                .collect::<Result<_, _>>()
                .expect("Error on fetch");

            // Asserts that all values are equal
            assert_eq!(vals, rows);

            let mut rows = stmt.query(tr, ()).expect("Error on query");

            let row1: Row = rows
                .fetch()
                .expect("Error on fetch the next row")
                .expect("No more rows");

            assert_eq!(
                2,
                row1.get::<i32>(0)
                    .expect("Error on get the first column value")
            );
            assert_eq!(
                "coffee".to_string(),
                row1.get::<String>(1)
                    .expect("Error on get the second column value")
            );

            let row = rows
                .fetch()
                .expect("Error on fetch the next row")
                .expect("No more rows");

            assert_eq!(
                3,
                row.get::<i32>(0)
                    .expect("Error on get the first column value")
            );
            assert_eq!(
                "milk".to_string(),
                row.get::<String>(1)
                    .expect("Error on get the second column value")
            );

            let row = rows
                .fetch()
                .expect("Error on fetch the next row")
                .expect("No more rows");

            assert!(
                row.get::<i32>(0).is_err(),
                "The 3° row have a null value, then should return an error"
            ); // null value
            assert!(
                row.get::<Option<i32>>(0)
                    .expect("Error on get the first column value")
                    .is_none(),
                "The 3° row have a null value, then should return a None"
            ); // null value
            assert_eq!(
                "fail coffee".to_string(),
                row.get::<String>(1)
                    .expect("Error on get the second column value")
            );

            let row = rows.fetch().expect("Error on fetch the next row");

            assert!(
                row.is_none(),
                "The 4° row dont exists, then should return a None"
            ); // null value

            Ok(())
        })
        .expect("Error commiting the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn prepared_insert() {
        let (conn, table) = setup();

        let vals = vec![(Some(9), "apple"), (Some(12), "jack"), (None, "coffee")];

        conn.with_transaction(|tr| {
            for val in vals.into_iter() {
                tr.execute(&format!("insert into {} (id, name) values (?, ?)", table), val)
                    .expect("Error on insert");
            }

            Ok(())
        })
        .expect("Error in the transaction");

        conn.close().expect("error on close the connection");
    }

    // #[test]
    // fn immediate_insert() {
    //     let (mut conn, table) = setup();

    //     conn.with_transaction(|tr| {
    //         tr.execute_immediate(&format!("insert into {} (id, name) values (?, ?)", (1, "apple", table)))
    //             .expect("Error on 1° insert");

    //         tr.execute_immediate(&format!("insert into {} (id, name) values (?, ?)", (2, "coffe", table)))
    //             .expect("Error on 2° insert");

    //         Ok(())
    //     })
    //     .expect("Error in the transaction");

    //     conn.close().expect("error on close the connection");
    // }

    fn setup() -> (Connection<impl FirebirdClient>, String) {
        let conn = connect();

        let table_num = super::TABLE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let table = format!("product{}", table_num);

        conn.with_transaction(|tr| {
            tr.execute_immediate(&format!("DROP TABLE {}", table)).ok();

            tr.execute_immediate(&format!("CREATE TABLE {} (id int, name varchar(60), quantity int)", table))
                .expect("Error on create the table product");

            Ok(())
        })
        .expect("Error in the transaction");

        (conn, table)
    }
}
