//! Firebird transaction

use super::connection::FbConnection;
use diesel::connection::*;
use diesel::result::Error::DatabaseError;
use diesel::result::*;
use diesel::QueryResult;
use std::cell::Cell;

/// Firebird transaction manager
pub struct FbTransactionManager {
    depth: Cell<u32>,
}

impl FbTransactionManager {
    pub fn new() -> Self {
        FbTransactionManager {
            depth: Cell::new(0),
        }
    }
}

impl Default for FbTransactionManager {
    fn default() -> Self {
        FbTransactionManager::new()
    }
}

impl TransactionManager<FbConnection> for FbTransactionManager {
    fn begin_transaction(&self, conn: &FbConnection) -> QueryResult<()> {
        let depth = self.depth.get() + 1;
        if depth == 1 {
            conn.raw.borrow_mut().begin_transaction().map_err(|e| {
                DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string()))
            })?;
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
            conn.raw.borrow_mut().rollback().map_err(|e| {
                DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string()))
            })?;
        } else {
            conn.execute(&format!("rollback to savepoint sp_diesel_{}", depth))?;
        }

        self.depth.set(depth - 1);

        Ok(())
    }

    fn commit_transaction(&self, conn: &FbConnection) -> QueryResult<()> {
        conn.raw
            .borrow_mut()
            .commit()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        self.depth.set(0);

        Ok(())
    }

    fn get_transaction_depth(&self) -> u32 {
        self.depth.get()
    }
}
