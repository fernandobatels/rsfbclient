//
// Rust Firebird Client
//
// Preparation and execution of statements
//

use std::cell::Cell;
use std::ffi::CString;
use std::mem;
use std::os::raw::c_char;
use std::os::raw::c_short;
use std::os::raw::c_void;
use std::ptr;
use std::result::Result;

use super::error::FbError;
use super::ibase;
use super::row::Row;
use super::transaction::Transaction;

pub struct Statement<'a> {
    handle: Cell<ibase::isc_stmt_handle>,
    xsqlda: Cell<*mut ibase::XSQLDA>,
    tr: &'a Transaction<'a>,
}

impl<'a> Statement<'a> {
    /// Prepare the statement that will be executed
    pub fn prepare(tr: &'a Transaction, sql: String) -> Result<Statement<'a>, FbError> {
        let handle = Cell::new(0 as u32);

        let xsqlda = Cell::new(unsafe { libc::malloc(xsqlda_length(1)) as *mut ibase::XSQLDA });

        unsafe {
            let conn_handle_ptr = tr.conn.handle.as_ptr();
            let handle_ptr = handle.as_ptr();

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_allocate_statement(status, conn_handle_ptr, handle_ptr) != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);

            let xsqlda_ptr = *xsqlda.as_ptr();
            (*xsqlda_ptr).version = 1;

            let c_sql = match CString::new(sql) {
                Ok(c) => c.into_raw(),
                Err(e) => {
                    return Err(FbError {
                        code: -1,
                        msg: e.to_string(),
                    })
                }
            };
            let tr_handle_ptr = tr.handle.as_ptr();
            let handle_ptr = handle.as_ptr();
            let xsqlda_ptr = *xsqlda.as_ptr();

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_prepare(status, tr_handle_ptr, handle_ptr, 0, c_sql, 1, xsqlda_ptr)
                != 0
            {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(Statement {
            handle: handle,
            xsqlda: xsqlda,
            tr: tr,
        })
    }

    /// Execute the current statement without parameters
    pub fn execute_simple(&self) -> Result<(), FbError> {
        unsafe {
            let handle_ptr = self.handle.as_ptr();
            let tr_handle_ptr = self.tr.handle.as_ptr();
            let xsqlda_ptr = *self.xsqlda.as_ptr();

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_execute(status, tr_handle_ptr, handle_ptr, 1, xsqlda_ptr) != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(())
    }

    /// Execute the current statement without parameters
    /// and returns the lines founds
    pub fn query_simple(self) -> Result<StatementFetch, FbError> {
        unsafe {
            let handle_ptr = self.handle.as_ptr();
            let mut xsqlda_ptr = *self.xsqlda.as_ptr();

            // Need more XSQLVARs
            if (*xsqlda_ptr).sqld > (*xsqlda_ptr).sqln {
                let num_cols = (*xsqlda_ptr).sqld;
                libc::free(xsqlda_ptr as *mut c_void);

                self.xsqlda
                    .replace(libc::malloc(xsqlda_length(num_cols)) as *mut ibase::XSQLDA);
                xsqlda_ptr = *self.xsqlda.as_ptr();

                (*xsqlda_ptr).version = 1;
                (*xsqlda_ptr).sqln = num_cols;

                let status: *mut ibase::ISC_STATUS_ARRAY =
                    libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                        as *mut ibase::ISC_STATUS_ARRAY;
                if ibase::isc_dsql_describe(status, handle_ptr, 1, xsqlda_ptr) != 0 {
                    return Err(FbError::from_status(status));
                }
                libc::free(status as *mut c_void);
            }

            let tr_handle_ptr = self.tr.handle.as_ptr();
            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_describe(status, handle_ptr, 1, xsqlda_ptr) != 0 {
                return Err(FbError::from_status(status));
            }
            libc::free(status as *mut c_void);

            for col in 0..(*xsqlda_ptr).sqld {
                let mut xcol = (*xsqlda_ptr).sqlvar[col as usize];

                xcol.sqldata = libc::malloc(xcol.sqllen as usize) as *mut c_char;
                xcol.sqlind = libc::malloc(1) as *mut c_short;

                (*xsqlda_ptr).sqlvar[col as usize] = xcol;
            }

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_execute(status, tr_handle_ptr, handle_ptr, 1, xsqlda_ptr) != 0 {
                return Err(FbError::from_status(status));
            }
            libc::free(status as *mut c_void);
        }

        Ok(StatementFetch {
            handle: self.handle,
            xsqlda: self.xsqlda,
        })
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(tr: &Transaction, sql: String) -> Result<(), FbError> {
        unsafe {
            let handle_ptr = tr.handle.as_ptr();
            let conn_handle_ptr = tr.conn.handle.as_ptr();

            let c_sql = match CString::new(sql) {
                Ok(c) => c.into_raw(),
                Err(e) => {
                    return Err(FbError {
                        code: -1,
                        msg: e.to_string(),
                    })
                }
            };

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_execute_immediate(
                status,
                conn_handle_ptr,
                handle_ptr,
                0,
                c_sql,
                1,
                ptr::null(),
            ) != 0
            {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);
        }

        Ok(())
    }
}

pub struct StatementFetch {
    handle: Cell<ibase::isc_stmt_handle>,
    pub xsqlda: Cell<*mut ibase::XSQLDA>,
}

impl StatementFetch {
    /// Fetch for the next row
    pub fn fetch(&mut self) -> Result<Option<Row>, FbError> {
        unsafe {
            let handle_ptr = self.handle.as_ptr();
            let xsqlda_ptr = *self.xsqlda.as_ptr();

            let status: *mut ibase::ISC_STATUS_ARRAY =
                libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>())
                    as *mut ibase::ISC_STATUS_ARRAY;
            let result_fetch = ibase::isc_dsql_fetch(status, handle_ptr, 1, xsqlda_ptr);

            // 100 indicates that no more rows: http://docwiki.embarcadero.com/InterBase/2020/en/Isc_dsql_fetch()
            if result_fetch == 100 {
                return Ok(None);
            }

            if result_fetch != 0 {
                return Err(FbError::from_status(status));
            }

            libc::free(status as *mut c_void);

            let row = Row { stmt_ft: self };

            Ok(Some(row))
        }
    }
}

/// Implementation of XSQLDA_LENGTH macro
fn xsqlda_length(size: i16) -> usize {
    let n = (size - 1).max(0) as usize;

    std::mem::size_of::<ibase::XSQLDA>() + (std::mem::size_of::<ibase::XSQLVAR>() * n)
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

        let stmt = tr
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
            "coffe".to_string(),
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

        let row = rows.fetch().expect("Error on fetch the next row");

        assert!(
            row.is_none(),
            "The 4° row dont exists, then should return a None"
        ); // null value

        tr.rollback().expect("Error on rollback the transaction");

        conn.close().expect("error on close the connection");
    }

    #[test]
    fn prepared_insert() {
        let conn = setup();

        let tr = conn
            .start_transaction()
            .expect("Error on start the transaction");

        let stmt = tr
            .prepare("insert into product (id, name) values (1, 'apple')".to_string())
            .expect("Error on prepare");

        stmt.execute_simple().expect("Error on execute");

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
