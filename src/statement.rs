//
// Rust Firebird Client
//
// Preparation and execution of statements
//

use std::mem;
use std::os::raw::c_char;
use std::os::raw::c_short;
use std::ptr;
use std::result::Result;

use super::error::FbError;
use super::ibase;
use super::row::Row;
use super::transaction::Transaction;
use super::xsqlda::XSqlDa;

pub struct Statement<'c, 't> {
    pub(crate) handle: ibase::isc_stmt_handle,
    pub(crate) xsqlda: XSqlDa,
    pub(crate) tr: &'t Transaction<'c>,
}

impl<'c, 't> Statement<'c, 't> {
    /// Prepare the statement that will be executed
    pub fn prepare(tr: &'t Transaction<'c>, sql: String) -> Result<Statement<'c, 't>, FbError> {
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
                1, // TODO: Add a way to select the dialect (1, 2 or 3)
                &mut *xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(Statement { handle, xsqlda, tr })
    }

    /// Execute the current statement without parameters
    pub fn execute_simple(&mut self) -> Result<(), FbError> {
        let status = &self.tr.conn.status;

        unsafe {
            if ibase::isc_dsql_execute(
                status.borrow_mut().as_mut_ptr(),
                self.tr.handle.as_ptr(),
                &mut self.handle,
                1,
                &*self.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(())
    }

    /// Execute the current statement without parameters
    /// and returns the lines founds
    pub fn query_simple<'s>(&'s mut self) -> Result<StatementFetch<'c, 't, 's>, FbError> {
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

            for col in 0..self.xsqlda.sqln {
                let mut xcol = self.xsqlda.get_xsqlvar_mut(col as usize).unwrap();

                // + 2 because varchars need two more bytes to store the size
                // TODO: Data never deallocated
                xcol.sqldata = libc::malloc(xcol.sqllen as usize + 2) as *mut c_char;
                xcol.sqldata.write_bytes(0, xcol.sqllen as usize + 2); // Initializes with 0
                xcol.sqlind = libc::malloc(mem::size_of::<c_short>()) as *mut c_short;
                xcol.sqldata.write_bytes(0, mem::size_of::<c_short>()); // Initializes with 0
            }

            if ibase::isc_dsql_execute(
                status.borrow_mut().as_mut_ptr(),
                self.tr.handle.as_ptr(),
                &mut self.handle,
                1,
                &*self.xsqlda,
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(StatementFetch { stmt: self })
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(tr: &Transaction, sql: String) -> Result<(), FbError> {
        let status = &tr.conn.status;

        unsafe {
            if ibase::isc_dsql_execute_immediate(
                status.borrow_mut().as_mut_ptr(),
                tr.conn.handle.as_ptr(),
                tr.handle.as_ptr(),
                sql.len() as u16,
                sql.as_ptr() as *const i8,
                1,
                ptr::null(),
            ) != 0
            {
                return Err(status.borrow().as_error());
            }
        }

        Ok(())
    }
}

impl<'c, 't> Drop for Statement<'c, 't> {
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
pub struct StatementFetch<'c, 't, 's> {
    pub(crate) stmt: &'s mut Statement<'c, 't>,
}

impl<'c, 't, 's> StatementFetch<'c, 't, 's> {
    /// Fetch for the next row
    pub fn fetch<'sf>(&'sf mut self) -> Result<Option<Row<'c, 't, 's, 'sf>>, FbError> {
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
}

impl<'c, 't, 's> Drop for StatementFetch<'c, 't, 's> {
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

        let tr = conn
            .start_transaction()
            .expect("Error on start the transaction");
        tr.execute_immediate("insert into product (id, name) values (2, 'coffee')".to_string())
            .expect("Error on insert");
        tr.execute_immediate("insert into product (id, name) values (3, 'milk')".to_string())
            .expect("Error on insert");
        tr.execute_immediate(
            "insert into product (id, name) values (null, 'fail coffee')".to_string(),
        )
        .expect("Error on insert");
        tr.commit().expect("Error on commit the transaction");

        let tr = conn
            .start_transaction()
            .expect("Error on start the transaction");

        let mut stmt = tr
            .prepare("select id, name from product".to_string())
            .expect("Error on prepare the select");

        let mut rows = stmt.query_simple().expect("Error on query");

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

        drop(stmt);

        tr.rollback().expect("Error on rollback the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn prepared_insert() {
        let conn = setup();

        let tr = conn
            .start_transaction()
            .expect("Error on start the transaction");

        let mut stmt = tr
            .prepare("insert into product (id, name) values (1, 'apple')".to_string())
            .expect("Error on prepare");

        stmt.execute_simple().expect("Error on execute");

        drop(stmt);

        tr.commit().expect("Error on commit the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn normal_insert() {
        let conn = setup();

        let tr = conn
            .start_transaction()
            .expect("Error on start the transaction");

        tr.execute_immediate("insert into product (id, name) values (1, 'apple')".to_string())
            .expect("Error on 1° insert");

        tr.execute_immediate("insert into product (id, name) values (2, 'coffee')".to_string())
            .expect("Error on 2° insert");

        tr.commit().expect("Error on commit the transaction");

        conn.close().expect("error on close the connection");
    }

    fn setup() -> Connection {
        Connection::recreate_local("test.fdb".to_string())
            .expect("Error on recreate the test database");
        let conn = Connection::open_local("test.fdb".to_string())
            .expect("Error on connect the test database");

        let tr = conn
            .start_transaction()
            .expect("Error on start the transaction");

        tr.execute_immediate(
            "CREATE TABLE product (id int, name varchar(60), quantity int)".to_string(),
        )
        .expect("Error on create the table product");

        tr.commit().expect("Error on commit the transaction");

        conn
    }
}
