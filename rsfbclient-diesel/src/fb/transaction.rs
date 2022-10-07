//! Firebird transaction

use super::connection::FbConnection;
use diesel::connection::TransactionManagerStatus;
use diesel::result::DatabaseErrorKind;
use diesel::result::Error::DatabaseError;
use diesel::QueryResult;
use diesel::{connection::*, RunQueryDsl};

/// Firebird transaction manager
pub struct FbTransactionManager {
    status: TransactionManagerStatus,
}

impl FbTransactionManager {
    pub fn new() -> Self {
        FbTransactionManager {
            status: Default::default(),
        }
    }

    fn get_transaction_state(
        conn: &mut FbConnection,
    ) -> QueryResult<&mut ValidTransactionManagerStatus> {
        match FbTransactionManager::transaction_manager_status_mut(conn) {
            TransactionManagerStatus::Valid(v) => Ok(v),
            TransactionManagerStatus::InError => {
                Err(diesel::result::Error::BrokenTransactionManager)
            }
        }
    }
}

impl Default for FbTransactionManager {
    fn default() -> Self {
        FbTransactionManager::new()
    }
}

impl TransactionManager<FbConnection> for FbTransactionManager {
    type TransactionStateData = Self;

    fn begin_transaction(conn: &mut FbConnection) -> QueryResult<()> {
        let state = Self::transaction_manager_status_mut(conn);
        let depth = state.transaction_depth()?;
        if let Some(depth) = depth {
            // Firebird does not support nested transactions, so
            // let's simulate this using save points
            diesel::sql_query(&format!("savepoint sp_diesel_{}", u32::from(depth) + 1))
                .execute(conn)?;
        } else {
            conn.raw
                .begin_transaction(None)
                .map_err(|e| DatabaseError(DatabaseErrorKind::Unknown, Box::new(e.to_string())))?;
        }
        if let TransactionManagerStatus::Valid(s) = Self::transaction_manager_status_mut(conn) {
            s.change_transaction_depth(TransactionDepthChange::IncreaseDepth)?;
        }
        Ok(())
    }

    fn rollback_transaction(conn: &mut FbConnection) -> QueryResult<()> {
        let transaction_state = Self::get_transaction_state(conn)?;

        let rollback_result = match transaction_state.transaction_depth().map(|d| d.get()) {
            Some(1) => conn
                .raw
                .rollback()
                .map_err(|e| DatabaseError(DatabaseErrorKind::Unknown, Box::new(e.to_string()))),
            Some(depth_gt1) => {
                diesel::sql_query(&format!("rollback to savepoint sp_diesel_{}", depth_gt1))
                    .execute(conn)
                    .map(|_| ())
            }
            None => return Err(diesel::result::Error::NotInTransaction),
        };

        match rollback_result {
            Ok(()) => {
                Self::get_transaction_state(conn)?
                    .change_transaction_depth(TransactionDepthChange::DecreaseDepth)?;
                Ok(())
            }
            Err(rollback_error) => {
                let tm_status = Self::transaction_manager_status_mut(conn);
                tm_status.set_in_error();
                Err(rollback_error)
            }
        }
    }

    fn commit_transaction(conn: &mut FbConnection) -> QueryResult<()> {
        let transaction_state = Self::get_transaction_state(conn)?;
        let transaction_depth = transaction_state.transaction_depth();
        let commit_result = match transaction_depth {
            None => return Err(diesel::result::Error::NotInTransaction),
            Some(transaction_depth) if transaction_depth.get() == 1 => conn
                .raw
                .commit()
                .map_err(|e| DatabaseError(DatabaseErrorKind::Unknown, Box::new(e.to_string()))),
            Some(_transaction_depth) => Ok(()),
        };
        match commit_result {
            Ok(()) => {
                Self::get_transaction_state(conn)?
                    .change_transaction_depth(TransactionDepthChange::DecreaseDepth)?;
                Ok(())
            }
            Err(commit_error) => {
                if let TransactionManagerStatus::Valid(ref mut s) = conn.transaction_state().status
                {
                    match s.transaction_depth().map(|p| p.get()) {
                        Some(1) => match Self::rollback_transaction(conn) {
                            Ok(()) => {}
                            Err(rollback_error) => {
                                conn.transaction_state().status.set_in_error();
                                return Err(diesel::result::Error::RollbackErrorOnCommit {
                                    rollback_error: Box::new(rollback_error),
                                    commit_error: Box::new(commit_error),
                                });
                            }
                        },
                        Some(_depth_gt1) => {
                            // There's no point in *actually* rolling back this one
                            // because we won't be able to do anything until top-level
                            // is rolled back.

                            // To make it easier on the user (that they don't have to really look
                            // at actual transaction depth and can just rely on the number of
                            // times they have called begin/commit/rollback) we don't mark the
                            // transaction manager as out of the savepoints as soon as we
                            // realize there is that issue, but instead we still decrement here:
                            s.change_transaction_depth(TransactionDepthChange::DecreaseDepth)?;
                        }
                        None => {}
                    }
                }
                Err(commit_error)
            }
        }
    }

    fn transaction_manager_status_mut(conn: &mut FbConnection) -> &mut TransactionManagerStatus {
        &mut conn.transaction_state().status
    }
}
