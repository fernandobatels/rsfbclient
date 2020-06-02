///
/// Rust Firebird Client 
///
/// Preparation and execution of statements 
///

use std::result::Result;
use std::os::raw::c_void;
use std::mem;
use std::ptr;
use std::cell::Cell;
use std::ffi::CString;

use super::ibase;
use super::error::FbError;
use super::transaction::Transaction;

pub struct Statement<'a> {
    handle: Cell<ibase::isc_stmt_handle>, 
    xsqlda: Cell<*mut ibase::XSQLDA>,
    tr: &'a Transaction<'a>
}

impl<'a> Statement<'a> {

    /// Prepare the statement that will be executed
    pub fn prepare(tr: &'a Transaction, sql: String) -> Result<Statement<'a>, FbError> {
        let handle = Cell::new(0 as u32);
        
        let xsqlda = Cell::new(unsafe {
            libc::malloc(mem::size_of::<ibase::XSQLDA>() * mem::size_of::<ibase::XSQLDA>()) as *mut ibase::XSQLDA
        });

        unsafe {
            let conn_handle_ptr = tr.conn.handle.as_ptr();
            let handle_ptr = handle.as_ptr(); 

            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_alloc_statement2(status, conn_handle_ptr, handle_ptr) != 0 {
                return Err(FbError::from_status(status)); 
            }

            libc::free(status as *mut c_void);

            let xsqlda_ptr = *xsqlda.as_ptr(); 
            (*xsqlda_ptr).version = 1;

            let c_sql = match CString::new(sql) {
                Ok(c) => c.into_raw(),
                Err(e) => return Err(FbError { code: -1, msg: e.to_string() })
            };
            let tr_handle_ptr = tr.handle.as_ptr();
            let handle_ptr = handle.as_ptr(); 
            let xsqlda_ptr = *xsqlda.as_ptr(); 

            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_prepare(status, tr_handle_ptr, handle_ptr, 0, c_sql, 1, xsqlda_ptr) != 0 {
                return Err(FbError::from_status(status)); 
            }

            libc::free(status as *mut c_void);
        }

        Ok(Statement {
            handle: handle,
            xsqlda: xsqlda,
            tr: tr
        })
    }

    /// Execute the current statement without parameters
    pub fn execute_simple(&self) -> Result<(), FbError> {

        unsafe {
            
            let handle_ptr = self.handle.as_ptr(); 
            let tr_handle_ptr = self.tr.handle.as_ptr();
            let xsqlda_ptr = *self.xsqlda.as_ptr();

            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_execute(status, tr_handle_ptr, handle_ptr, 1, xsqlda_ptr) != 0 {
                return Err(FbError::from_status(status)); 
            }
            
            libc::free(status as *mut c_void);
        }
    
        Ok(())
    }

    /// Execute the current statement without parameters
    /// and returns the lines founds
    pub fn query_simple(&self) -> Result<(), FbError> {
        unimplemented!();
    }

    /// Execute the statement without returning any row
    pub fn execute_immediate(tr: &Transaction, sql: String) -> Result<(), FbError> {

        unsafe {
            
            let handle_ptr = tr.handle.as_ptr(); 
            let conn_handle_ptr = tr.conn.handle.as_ptr();

            let c_sql = match CString::new(sql) {
                Ok(c) => c.into_raw(),
                Err(e) => return Err(FbError { code: -1, msg: e.to_string() })
            };

            let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
            if ibase::isc_dsql_execute_immediate(status, conn_handle_ptr, handle_ptr, 0, c_sql, 1, ptr::null()) != 0 {
                return Err(FbError::from_status(status)); 
            }
            
            libc::free(status as *mut c_void);
        }
    
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::connection::Connection;

    #[test]
    fn prepared_insert() {
        let conn = setup();

        let tr = conn.start_transaction()
            .expect("Error on start the transaction");

        let stmt = tr.prepare("insert into product (id, name) values (1, 'apple')".to_string())
            .expect("Error on prepare");

        stmt.execute_simple()
            .expect("Error on execute");

        tr.commit()
            .expect("Error on commit the transaction");

        conn.close()
            .expect("error on close the connection");
    }

    fn setup() -> Connection {
    
        Connection::recreate_local("test.fdb".to_string())
            .expect("Error on recreate the test database");
        let conn = Connection::open_local("test.fdb".to_string())
            .expect("Error on connect the test database");

        let tr = conn.start_transaction()
            .expect("Error on start the transaction");

        tr.execute_immediate("CREATE TABLE product (id int, name varchar(60))".to_string())
            .expect("Error on create the table user");

        tr.commit()
            .expect("Error on commit the transaction");

        conn
    }
}
