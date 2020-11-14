//! Firebird transaction

use super::connection::FbConnection;
use diesel::connection::*;
use diesel::result::Error::DatabaseError;
use diesel::result::*;
use diesel::QueryResult;
use rsfbclient::SimpleTransaction as FbRawTransaction;
use std::cell::{Cell, RefCell};

/// Firebird transaction manager
pub struct FbTransactionManager<'c> {
    pub(crate) raw: RefCell<Option<FbRawTransaction<'c>>>,
    depth: Cell<u32>,
}

impl<'c> FbTransactionManager<'c> {
    pub fn new() -> Self {
        FbTransactionManager {
            raw: RefCell::new(None),
            depth: Cell::new(0),
        }
    }
}

impl<'c> TransactionManager<FbConnection<'c>> for FbTransactionManager<'c> {
    fn begin_transaction(&self, conn: &FbConnection) -> QueryResult<()> {
        let depth = self.depth.get() + 1;
        if depth == 1 {
            let conn = unsafe { conn.raw.as_ptr().as_mut().unwrap() };

            let tr = FbRawTransaction::new(conn).map_err(|e| {
                DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string()))
            })?;

            self.raw.replace(Some(tr));
        } else {
            // Firebird does not support nested transactions, so
            // let's simulate this using save points
            conn.execute(&format!("savepoint sp_diesel_{}", depth))?;
        }

        self.depth.set(depth);

        Ok(())
    }

    fn rollback_transaction(&self, conn: &FbConnection) -> QueryResult<()> {
        let depth = self.depth.get();
        if depth <= 1 {
            self.raw
                .borrow_mut()
                .as_mut()
                .unwrap()
                .rollback_retaining()
                .map_err(|e| {
                    DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string()))
                })?;

            self.raw.replace(None);
        } else {
            conn.execute(&format!("rollback to savepoint sp_diesel_{}", depth))?;
        }

        self.depth.set(depth - 1);

        Ok(())
    }

    fn commit_transaction(&self, _conn: &FbConnection) -> QueryResult<()> {
        self.raw
            .borrow_mut()
            .as_mut()
            .unwrap()
            .commit_retaining()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        self.raw.replace(None);

        self.depth.set(0);

        Ok(())
    }

    fn get_transaction_depth(&self) -> u32 {
        self.depth.get()
    }
}
