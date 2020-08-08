//! Connection trait to abstract over the client implementations

use crate::*;

pub trait FirebirdConnection {
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
        tpb: &[u8],
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
#[repr(u16)]
/// Firebird sql dialect
pub enum Dialect {
    D1 = 1,
    D2 = 2,
    D3 = 3,
}

#[derive(Debug, Clone, Copy)]
/// Commit / Rollback operations
pub enum TrOp {
    Commit,
    CommitRetaining,
    Rollback,
    RollbackRetaining,
}

/// Drop / Close statement
pub enum FreeStmtOp {
    Close,
    Drop,
}

/// Statement type
pub enum StmtType {
    Select,
    Insert,
    Update,
    Delete,
    DDL,
}
