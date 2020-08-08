//! Connection trait to abstract over the client implementations

use crate::*;

pub trait FirebirdClient {
    /// A database handle
    type DbHandle;
    /// A transaction handle
    type TrHandle;
    /// A statement handle
    type StmtHandle;

    /// Connect to a database, returning a database handle
    fn attach_database(
        &mut self,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<Self::DbHandle, FbError>;

    /// Disconnect from the database
    fn detach_database(&mut self, db_handle: Self::DbHandle) -> Result<(), FbError>;

    /// Drop the database
    fn drop_database(&mut self, db_handle: Self::DbHandle) -> Result<(), FbError>;

    /// Start a new transaction, with the specified transaction parameter buffer
    fn begin_transaction(
        &mut self,
        db_handle: Self::DbHandle,
        isolation_level: TrIsolationLevel,
    ) -> Result<Self::TrHandle, FbError>;

    /// Commit / Rollback a transaction
    fn transaction_operation(&mut self, tr_handle: Self::TrHandle, op: TrOp)
        -> Result<(), FbError>;

    /// Execute a sql immediately, without returning rows
    fn exec_immediate(
        &mut self,
        tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError>;

    /// Alloc and prepare a statement
    ///
    /// Returns the statement type and handle
    fn prepare_statement(
        &mut self,
        db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, Self::StmtHandle), FbError>;

    /// Closes or drops a statement
    fn free_statement(
        &mut self,
        stmt_handle: Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError>;

    /// Execute the prepared statement with parameters
    fn execute(
        &mut self,
        tr_handle: Self::TrHandle,
        stmt_handle: Self::StmtHandle,
        params: &[Param],
    ) -> Result<(), FbError>;

    /// Fetch rows from the executed statement, coercing the types
    /// according to the provided blr
    fn fetch(
        &mut self,
        stmt_handle: Self::StmtHandle,
    ) -> Result<Option<Vec<Option<Column>>>, FbError>;
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
/// Firebird sql dialect
pub enum Dialect {
    D1 = 1,
    D2 = 2,
    D3 = 3,
}

#[repr(u8)]
/// Transaction isolation level
pub enum TrIsolationLevel {
    /// Transactions can't see alterations commited after they started
    Concurrency = ibase::isc_tpb_concurrency as u8,
    /// Table locking
    Concistency = ibase::isc_tpb_consistency as u8,
    /// Transactions can see alterations commited after they started
    ReadCommited = ibase::isc_tpb_read_committed as u8,
}

impl Default for TrIsolationLevel {
    fn default() -> Self {
        Self::ReadCommited
    }
}

#[derive(Debug, Clone, Copy)]
/// Commit / Rollback operations
pub enum TrOp {
    Commit,
    CommitRetaining,
    Rollback,
    RollbackRetaining,
}

#[repr(u8)]
/// Drop / Close statement
pub enum FreeStmtOp {
    Close = ibase::DSQL_close as u8,
    Drop = ibase::DSQL_drop as u8,
}

#[repr(u8)]
/// Statement type
pub enum StmtType {
    Select = ibase::isc_info_sql_stmt_select as u8,
    Insert = ibase::isc_info_sql_stmt_insert as u8,
    Update = ibase::isc_info_sql_stmt_update as u8,
    Delete = ibase::isc_info_sql_stmt_delete as u8,
    DDL = ibase::isc_info_sql_stmt_ddl as u8,
}
