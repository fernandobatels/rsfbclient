//!
//! Rust Firebird Client
//!
//! Preparation and execution of statements
//!

use super::ibase;
use super::params::IntoParams;
use super::params::Params;
use super::row::ColumnBuffer;
use super::row::FromRow;
use super::row::Row;
use super::status::FbError;
use super::transaction::Transaction;
use super::xsqlda::XSqlDa;

pub struct Statement<'a> {
    pub(crate) handle: ibase::isc_stmt_handle,
    pub(crate) xsqlda: XSqlDa,
    pub(crate) tr: &'a Transaction<'a>,
}

impl<'a> Statement<'a> {
    /// Prepare the statement that will be executed
    pub fn prepare(tr: &'a Transaction<'a>, sql: &str) -> Result<Self, FbError> {
        let mut handle = 0;
        let status = &tr.conn.status;

        let mut xsqlda = XSqlDa::new(1);

        unsafe {
            if ibase::isc_dsql_allocate_statement(
                status.borrow_mut().as_mut_ptr(),
                tr.conn.handle.as_ptr(),
                &mut handle,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }

            if ibase::isc_dsql_prepare(
                status.borrow_mut().as_mut_ptr(),
                tr.handle.as_ptr(),
                &mut handle,
                sql.len() as u16,
                sql.as_ptr() as *const i8,
                3, // TODO: Add a way to select the dialect (1, 2 or 3)
                &mut *xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(Statement { handle, xsqlda, tr })
    }

    /// Execute the current statement without returnig any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute<T>(&mut self, params: T) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        let status = &self.tr.conn.status;

        let params = Params::new(self, params.to_params())?;

        unsafe {
            if ibase::isc_dsql_execute(
                status.borrow_mut().as_mut_ptr(),
                self.tr.handle.as_ptr(),
                &mut self.handle,
                3, // Dialect
                &*params.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(())
    }

    /// Execute the current statement
    /// and returns the lines founds
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn query<T>(mut self, params: T) -> Result<StatementFetch<'a>, FbError>
    where
        T: IntoParams,
    {
        let status = &self.tr.conn.status;
        let row_count = self.xsqlda.sqld;

        // Need more XSQLVARs
        if row_count > self.xsqlda.sqln {
            self.xsqlda = XSqlDa::new(row_count);
        }

        unsafe {
            if ibase::isc_dsql_describe(
                status.borrow_mut().as_mut_ptr(),
                &mut self.handle,
                1,
                &mut *self.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        let params = Params::new(&mut self, params.to_params())?;

        unsafe {
            if ibase::isc_dsql_execute(
                status.borrow_mut().as_mut_ptr(),
                self.tr.handle.as_ptr(),
                &mut self.handle,
                1,
                &*params.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        let col_buffers = (0..self.xsqlda.sqln)
            .map(|col| {
                let xcol = self.xsqlda.get_xsqlvar_mut(col as usize).unwrap();

                ColumnBuffer::from_xsqlvar(xcol)
            })
            .collect::<Result<_, _>>()?;

        Ok(StatementFetch {
            stmt: self,
            buffers: col_buffers,
        })
    }

    /// Execute the statement without returning any row
    ///
    /// Use `()` for no parameters or a tuple of parameters
    pub fn execute_immediate<T>(
        tr: &'a Transaction<'a>,
        sql: &str,
        params: T,
    ) -> Result<(), FbError>
    where
        T: IntoParams,
    {
        let status = &tr.conn.status;

        let params = Params::new_immediate(params.to_params());

        unsafe {
            if ibase::isc_dsql_execute_immediate(
                status.borrow_mut().as_mut_ptr(),
                tr.conn.handle.as_ptr(),
                tr.handle.as_ptr(),
                sql.len() as u16,
                sql.as_ptr() as *const i8,
                3, // Dialect
                &*params.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(())
    }
}

impl<'a> Drop for Statement<'a> {
    fn drop(&mut self) {
        let status = &self.tr.conn.status;

        // Close the statement
        unsafe {
            ibase::isc_dsql_free_statement(
                status.borrow_mut().as_mut_ptr(),
                &mut self.handle,
                ibase::DSQL_drop as u16,
            )
        };

        // Assert that the handle is invalid
        debug_assert_eq!(self.handle, 0);
    }
}
/// Cursor to fetch the results of a statement
pub struct StatementFetch<'a> {
    pub(crate) stmt: Statement<'a>,
    pub(crate) buffers: Vec<ColumnBuffer>,
}

impl<'a> StatementFetch<'a> {
    /// Fetch for the next row
    pub fn fetch(&mut self) -> Result<Option<Row>, FbError> {
        let status = &self.stmt.tr.conn.status;

        let result_fetch = unsafe {
            ibase::isc_dsql_fetch(
                status.borrow_mut().as_mut_ptr(),
                &mut self.stmt.handle,
                1,
                &*self.stmt.xsqlda,
            )
        };
        // 100 indicates that no more rows: http://docwiki.embarcadero.com/InterBase/2020/en/Isc_dsql_fetch()
        if result_fetch == 100 {
            return Ok(None);
        }

        if result_fetch != 0 {
            return Err(status.borrow().as_error());
        }

        let row = Row { stmt_ft: self };

        Ok(Some(row))
    }

    pub fn into_iter<T>(self) -> StatementIter<'a, T>
    where
        T: FromRow,
    {
        StatementIter {
            stmt_ft: self,
            _marker: Default::default(),
        }
    }
}

/// Iterator for the statement results
pub struct StatementIter<'a, T> {
    stmt_ft: StatementFetch<'a>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> Iterator for StatementIter<'a, T>
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

impl<'a> Drop for StatementFetch<'a> {
    fn drop(&mut self) {
        let status = &self.stmt.tr.conn.status;

        unsafe {
            // Close the cursor
            ibase::isc_dsql_free_statement(
                status.borrow_mut().as_mut_ptr(),
                &mut self.stmt.handle,
                ibase::DSQL_close as u16,
            )
        };
    }
}

#[cfg(test)]
mod test {
    use crate::connection::Connection;

    #[test]
    fn simple_select() {
        let conn = setup();

        let vals = vec![
            (Some(2), "coffee".to_string()),
            (Some(3), "milk".to_string()),
            (None, "fail coffee".to_string()),
        ];

        let tr = conn.transaction().expect("Error on start the transaction");
        let mut stmt = tr
            .prepare("insert into product (id, name) values (?, ?)")
            .expect("Error preparing the insert statement");

        for val in &vals {
            stmt.execute(val.clone()).expect("Error on insert");
        }

        drop(stmt);

        tr.commit().expect("Error on commit the transaction");

        let tr = conn.transaction().expect("Error on start the transaction");

        let stmt = tr
            .prepare("select id, name from product")
            .expect("Error on prepare the select");

        let rows: Vec<(Option<i32>, String)> = stmt
            .query(())
            .expect("Error on query")
            .into_iter()
            .collect::<Result<_, _>>()
            .expect("Error on fetch");

        // Asserts that all values are equal
        assert_eq!(vals, rows);

        let stmt = tr
            .prepare("select id, name from product")
            .expect("Error on prepare the select");

        let mut rows = stmt.query(()).expect("Error on query");

        let row = rows
            .fetch()
            .expect("Error on fetch the next row")
            .expect("No more rows");

        assert_eq!(
            2,
            row.get::<i32>(0)
                .expect("Error on get the first column value")
        );
        assert_eq!(
            "coffee".to_string(),
            row.get::<String>(1)
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

        drop(rows);

        tr.rollback().expect("Error on rollback the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn prepared_insert() {
        let conn = setup();

        let vals = vec![(Some(9), "apple"), (Some(12), "jack"), (None, "coffee")];

        let tr = conn.transaction().expect("Error on start the transaction");

        let mut stmt = tr
            .prepare("insert into product (id, name) values (?, ?)")
            .expect("Error preparing the insert statement");

        for val in vals.into_iter() {
            stmt.execute(val).expect("Error on insert");
        }

        drop(stmt);

        tr.commit().expect("Error on commit the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn normal_insert() {
        let conn = setup();

        let tr = conn.transaction().expect("Error on start the transaction");

        tr.execute_immediate("insert into product (id, name) values (?, ?)", (1, "apple"))
            .expect("Error on 1° insert");

        tr.execute_immediate("insert into product (id, name) values (?, ?)", (2, "coffe"))
            .expect("Error on 2° insert");

        tr.commit().expect("Error on commit the transaction");

        conn.close().expect("error on close the connection");
    }

    fn setup() -> Connection {
        Connection::recreate_local("test.fdb").expect("Error on recreate the test database");
        let conn = Connection::open_local("test.fdb").expect("Error on connect the test database");

        let tr = conn.transaction().expect("Error on start the transaction");

        tr.execute_immediate(
            "CREATE TABLE product (id int, name varchar(60), quantity int)",
            (),
        )
        .expect("Error on create the table product");

        tr.commit().expect("Error on commit the transaction");

        conn
    }
}
