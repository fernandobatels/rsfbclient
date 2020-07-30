//!
//! Rust Firebird Client
//!
//! Preparation and execution of statements
//!

use crate::{
    ibase,
    params::{IntoParams, Params},
    row::{ColumnBuffer, FromRow, Row},
    status::FbError,
    transaction::{Transaction, TransactionData},
    xsqlda::XSqlDa,
    Connection,
};
use std::ptr;

pub struct Statement<'c> {
    pub(crate) data: StatementData,
    pub(crate) conn: &'c Connection,
}

impl<'c> Statement<'c> {
    /// Prepare the statement that will be executed
    pub fn prepare(tr: &mut Transaction<'c>, sql: &str) -> Result<Self, FbError> {
        let data = StatementData::prepare(tr.conn, &mut tr.data, sql)?;

        Ok(Statement {
            data,
            conn: tr.conn,
        })
    }

    /// Execute the current statement without returnig any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute<T>(&mut self, tr: &mut Transaction, params: T) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        self.data.execute(self.conn, &mut tr.data, params)
    }

    /// Execute the current statement
    /// and returns the lines founds
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn query<'s, T>(
        &'s mut self,
        tr: &'s mut Transaction,
        params: T,
    ) -> Result<StatementFetch<'s>, FbError>
    where
        T: IntoParams,
    {
        let buffers = self.data.query(self.conn, &mut tr.data, params)?;

        Ok(StatementFetch {
            stmt: &mut self.data,
            buffers,
            _tr: tr,
            conn: self.conn,
        })
    }
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        self.data.close(self.conn).ok();
    }
}
/// Cursor to fetch the results of a statement
pub struct StatementFetch<'s> {
    pub(crate) stmt: &'s mut StatementData,
    pub(crate) buffers: Vec<ColumnBuffer>,
    /// Transaction needs to be alive for the fetch to work
    pub(crate) _tr: &'s Transaction<'s>,
    pub(crate) conn: &'s Connection,
}

impl<'s> StatementFetch<'s> {
    /// Fetch for the next row
    pub fn fetch(&mut self) -> Result<Option<Row>, FbError> {
        self.stmt.fetch(self.conn, &mut self.buffers)
    }

    pub fn into_iter<T>(self) -> StatementIter<'s, T>
    where
        T: FromRow,
    {
        StatementIter {
            stmt_ft: self,
            _marker: Default::default(),
        }
    }
}

impl Drop for StatementFetch<'_> {
    fn drop(&mut self) {
        self.stmt.close_cursor(&self.conn).ok();
    }
}

/// Iterator for the statement results
pub struct StatementIter<'s, T> {
    stmt_ft: StatementFetch<'s>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Iterator for StatementIter<'_, T>
where
    T: FromRow,
{
    type Item = Result<T, FbError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.stmt_ft
            .fetch()
            .and_then(|row| row.map(|row| row.get_all()).transpose())
            .transpose()
    }
}

/// Low level statement handler.
///
/// Needs to be closed calling `close` before dropping.
pub struct StatementData {
    pub(crate) handle: ibase::isc_stmt_handle,
    pub(crate) xsqlda: XSqlDa,
}

impl StatementData {
    /// Prepare the statement that will be executed
    pub fn prepare(
        conn: &Connection,
        tr: &mut TransactionData,
        sql: &str,
    ) -> Result<Self, FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let mut handle = 0;

        let mut xsqlda = XSqlDa::new(1);

        unsafe {
            if ibase.isc_dsql_allocate_statement()(
                status.borrow_mut().as_mut_ptr(),
                conn.handle.as_ptr(),
                &mut handle,
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }

            if ibase.isc_dsql_prepare()(
                status.borrow_mut().as_mut_ptr(),
                &mut tr.handle,
                &mut handle,
                sql.len() as u16,
                sql.as_ptr() as *const _,
                conn.dialect as u16,
                &mut *xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        Ok(Self { handle, xsqlda })
    }

    /// Execute the current statement without returnig any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute<T>(
        &mut self,
        conn: &Connection,
        tr: &mut TransactionData,
        params: T,
    ) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let params = Params::new(conn, self, params.to_params())?;

        unsafe {
            if ibase.isc_dsql_execute()(
                status.borrow_mut().as_mut_ptr(),
                &mut tr.handle,
                &mut self.handle,
                1,
                if let Some(xsqlda) = &params.xsqlda {
                    &**xsqlda
                } else {
                    ptr::null()
                },
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        // Just to make sure the params are not dropped too soon
        drop(params);

        // Close the cursor, as it will not be used
        // ignoring the error, as if it was not a select statement,
        // the cursor will already be closed
        self.close_cursor(conn).ok();

        Ok(())
    }

    /// Execute the current statement
    /// and returns the column buffer
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn query<'s, T>(
        &'s mut self,
        conn: &'s Connection,
        tr: &mut TransactionData,
        params: T,
    ) -> Result<Vec<ColumnBuffer>, FbError>
    where
        T: IntoParams,
    {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let row_count = self.xsqlda.sqld;

        // Need more XSQLVARs
        if row_count > self.xsqlda.sqln {
            self.xsqlda = XSqlDa::new(row_count);
        }

        unsafe {
            if ibase.isc_dsql_describe()(
                status.borrow_mut().as_mut_ptr(),
                &mut self.handle,
                1,
                &mut *self.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        let params = Params::new(conn, self, params.to_params())?;

        unsafe {
            if ibase.isc_dsql_execute()(
                status.borrow_mut().as_mut_ptr(),
                &mut tr.handle,
                &mut self.handle,
                1,
                if let Some(xsqlda) = &params.xsqlda {
                    &**xsqlda
                } else {
                    ptr::null()
                },
            ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        }

        // Just to make sure the params are not dropped too soon
        drop(params);

        let col_buffers = (0..self.xsqlda.sqln)
            .map(|col| {
                let xcol = self.xsqlda.get_xsqlvar_mut(col as usize).unwrap();

                ColumnBuffer::from_xsqlvar(xcol)
            })
            .collect::<Result<_, _>>()?;

        Ok(col_buffers)
    }

    /// Fetch for the next row, needs to be called after `query`
    pub fn fetch<'a>(
        &mut self,
        conn: &Connection,
        buffers: &'a mut Vec<ColumnBuffer>,
    ) -> Result<Option<Row<'a>>, FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let result_fetch = unsafe {
            ibase.isc_dsql_fetch()(
                status.borrow_mut().as_mut_ptr(),
                &mut self.handle,
                1,
                &*self.xsqlda,
            )
        };
        // 100 indicates that no more rows: http://docwiki.embarcadero.com/InterBase/2020/en/Isc_dsql_fetch()
        if result_fetch == 100 {
            return Ok(None);
        }

        if result_fetch != 0 {
            return Err(status.borrow().as_error(ibase));
        }

        let row = Row { buffers };

        Ok(Some(row))
    }

    /// Closes the statement cursor, if it was open
    pub fn close_cursor(&mut self, conn: &Connection) -> Result<(), FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        unsafe {
            // Close the cursor
            if ibase.isc_dsql_free_statement()(
                status.borrow_mut().as_mut_ptr(),
                &mut self.handle,
                ibase::DSQL_close as u16,
            ) != 0
            {
                return Err(status.borrow().as_error(&ibase));
            }
        };

        Ok(())
    }

    /// Closes the statement
    pub fn close(&mut self, conn: &Connection) -> Result<(), FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        unsafe {
            if self.handle != 0
                && ibase.isc_dsql_free_statement()(
                    status.borrow_mut().as_mut_ptr(),
                    &mut self.handle,
                    ibase::DSQL_drop as u16,
                ) != 0
            {
                return Err(status.borrow().as_error(ibase));
            }
        };

        // Assert that the handle is invalid
        debug_assert_eq!(self.handle, 0);

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{prelude::*, Connection, Transaction};

    #[test]
    fn statements() {
        let conn1 = setup();

        #[cfg(not(feature = "dynamic_loading"))]
        let conn2 = crate::ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn2 = crate::ConnectionBuilder::with_client("./fbclient.lib")
            .expect("Error finding fbclient lib")
            .connect()
            .expect("Error on connect the test database");

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
        let mut conn = setup();

        let vals = vec![
            (Some(2), "coffee".to_string()),
            (Some(3), "milk".to_string()),
            (None, "fail coffee".to_string()),
        ];

        conn.with_transaction(|tr| {
            for val in &vals {
                tr.execute("insert into product (id, name) values (?, ?)", val.clone())
                    .expect("Error on insert");
            }

            Ok(())
        })
        .expect("Error commiting the transaction");

        let rows = conn
            .query("select id, name from product", ())
            .expect("Error executing query");

        // Asserts that all values are equal
        assert_eq!(vals, rows);
    }

    #[test]
    fn old_api_select() {
        let conn = setup();

        let vals = vec![
            (Some(2), "coffee".to_string()),
            (Some(3), "milk".to_string()),
            (None, "fail coffee".to_string()),
        ];

        conn.with_transaction(|tr| {
            let mut stmt = tr
                .prepare("insert into product (id, name) values (?, ?)")
                .expect("Error preparing the insert statement");

            for val in &vals {
                stmt.execute(tr, val.clone()).expect("Error on insert");
            }

            Ok(())
        })
        .expect("Error commiting the transaction");

        conn.with_transaction(|tr| {
            let mut stmt = tr
                .prepare("select id, name from product")
                .expect("Error on prepare the select");

            let rows: Vec<(Option<i32>, String)> = stmt
                .query(tr, ())
                .expect("Error on query")
                .into_iter()
                .collect::<Result<_, _>>()
                .expect("Error on fetch");

            // Asserts that all values are equal
            assert_eq!(vals, rows);

            let mut rows = stmt.query(tr, ()).expect("Error on query");

            let row1 = rows
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
        let conn = setup();

        let vals = vec![(Some(9), "apple"), (Some(12), "jack"), (None, "coffee")];

        conn.with_transaction(|tr| {
            for val in vals.into_iter() {
                tr.execute("insert into product (id, name) values (?, ?)", val)
                    .expect("Error on insert");
            }

            Ok(())
        })
        .expect("Error in the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn immediate_insert() {
        let conn = setup();

        conn.with_transaction(|tr| {
            tr.execute_immediate("insert into product (id, name) values (?, ?)", (1, "apple"))
                .expect("Error on 1° insert");

            tr.execute_immediate("insert into product (id, name) values (?, ?)", (2, "coffe"))
                .expect("Error on 2° insert");

            Ok(())
        })
        .expect("Error in the transaction");

        conn.close().expect("error on close the connection");
    }

    fn setup() -> Connection {
        #[cfg(not(feature = "dynamic_loading"))]
        let conn = crate::ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn = crate::ConnectionBuilder::with_client("./fbclient.lib")
            .expect("Error finding fbclient lib")
            .connect()
            .expect("Error on connect the test database");

        conn.with_transaction(|tr| {
            tr.execute_immediate("DROP TABLE product", ()).ok();

            tr.execute_immediate(
                "CREATE TABLE product (id int, name varchar(60), quantity int)",
                (),
            )
            .expect("Error on create the table product");

            Ok(())
        })
        .expect("Error in the transaction");

        conn
    }
}
