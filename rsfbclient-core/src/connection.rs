//! Connection trait to abstract over the client implementations

use num_enum::TryFromPrimitive;

use crate::*;

pub trait FirebirdClientEmbeddedAttach: FirebirdClient {
    /// Connect to a database, returning a database handle
    fn attach_database(&mut self, db_name: &str, user: &str) -> Result<Self::DbHandle, FbError>;
}

pub trait FirebirdClientRemoteAttach: FirebirdClient {
    /// Connect to a database, returning a database handle
    fn attach_database(
        &mut self,
        host: &str,
        port: u16,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<Self::DbHandle, FbError>;
}

pub trait FirebirdClient: Send {
    /// A database handle
    type DbHandle: Send + Clone + Copy;
    /// A transaction handle
    type TrHandle: Send + Clone + Copy;
    /// A statement handle
    type StmtHandle: Send + Clone + Copy;

    /// Arguments to instantiate the client
    type Args: Send + Sync + Clone;

    fn new(charset: Charset, args: Self::Args) -> Result<Self, FbError>
    where
        Self: Sized;

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
        db_handle: Self::DbHandle,
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
        db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        stmt_handle: Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<(), FbError>;

    /// Execute the prepared statement
    /// with input and output parameters.
    ///
    /// The output parameters will be returned
    /// as in the Result
    fn execute2(
        &mut self,
        db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        stmt_handle: Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<Vec<Column>, FbError>;

    /// Fetch rows from the executed statement, coercing the types
    /// according to the provided blr
    fn fetch(
        &mut self,
        db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        stmt_handle: Self::StmtHandle,
    ) -> Result<Option<Vec<Column>>, FbError>;
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
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

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
/// Commit / Rollback operations
pub enum TrOp {
    Commit,
    CommitRetaining,
    Rollback,
    RollbackRetaining,
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
/// Drop / Close statement
pub enum FreeStmtOp {
    Close = ibase::DSQL_close as u8,
    Drop = ibase::DSQL_drop as u8,
}

#[repr(u8)]
#[derive(Debug, Eq, PartialEq, Copy, Clone, TryFromPrimitive)]
/// Statement type
pub enum StmtType {
    Select = ibase::isc_info_sql_stmt_select as u8,
    Insert = ibase::isc_info_sql_stmt_insert as u8,
    Update = ibase::isc_info_sql_stmt_update as u8,
    Delete = ibase::isc_info_sql_stmt_delete as u8,
    DDL = ibase::isc_info_sql_stmt_ddl as u8,
    GetSegment = ibase::isc_info_sql_stmt_get_segment as u8,
    PutSegment = ibase::isc_info_sql_stmt_put_segment as u8,
    ExecProcedure = ibase::isc_info_sql_stmt_exec_procedure as u8,
    StartTrans = ibase::isc_info_sql_stmt_start_trans as u8,
    Commit = ibase::isc_info_sql_stmt_commit as u8,
    Rollback = ibase::isc_info_sql_stmt_rollback as u8,
    SelectForUpd = ibase::isc_info_sql_stmt_select_for_upd as u8,
    SetGenerator = ibase::isc_info_sql_stmt_set_generator as u8,
    Savepoint = ibase::isc_info_sql_stmt_savepoint as u8,
}
