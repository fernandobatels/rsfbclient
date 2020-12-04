//! Traits to abstract over firebird client implementations

use num_enum::TryFromPrimitive;
use std::str::FromStr;

use crate::*;

///A wrapper trait compatible with the niceties provided by the main rsfbclient crate
pub trait FirebirdClient
where
    Self: FirebirdClientDbOps,
    Self: FirebirdClientSqlOps<DbHandle = <Self as FirebirdClientDbOps>::DbHandle>,
{
}

impl<Hdl, A: FirebirdClientDbOps<DbHandle = Hdl> + FirebirdClientSqlOps<DbHandle = Hdl>>
    FirebirdClient for A
where
    Hdl: Send,
{
}

///Responsible for database administration and attachment/detachment
pub trait FirebirdClientDbOps: Send {
    /// A database handle
    type DbHandle: Send;

    /// Configuration details for attaching to the database.
    /// A user of an implementation of this trait can configure attachment details
    /// (database name, user name, etcetera) and then pass this configuration to the implementation
    /// via this type when a new attachment is requested
    type AttachmentConfig: Send + Clone;

    /// Create a new attachment to a database with the provided configuration
    /// Returns a database handle on success
    fn attach_database(
        &mut self,
        config: &Self::AttachmentConfig,
    ) -> Result<Self::DbHandle, FbError>;

    /// Disconnect from the database
    fn detach_database(&mut self, db_handle: &mut Self::DbHandle) -> Result<(), FbError>;

    /// Drop the database
    fn drop_database(&mut self, db_handle: &mut Self::DbHandle) -> Result<(), FbError>;
}

///Responsible for actual transaction and statement execution
pub trait FirebirdClientSqlOps {
    /// A database handle
    type DbHandle: Send;
    /// A transaction handle
    type TrHandle: Send;
    /// A statement handle
    type StmtHandle: Send;

    /// Start a new transaction, with the specified transaction parameter buffer
    fn begin_transaction(
        &mut self,
        db_handle: &mut Self::DbHandle,
        isolation_level: TrIsolationLevel,
    ) -> Result<Self::TrHandle, FbError>;

    /// Commit / Rollback a transaction
    fn transaction_operation(
        &mut self,
        tr_handle: &mut Self::TrHandle,
        op: TrOp,
    ) -> Result<(), FbError>;

    /// Execute a sql immediately, without returning rows
    fn exec_immediate(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError>;

    /// Allocate and prepare a statement
    /// Returns the statement type and handle
    fn prepare_statement(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, Self::StmtHandle), FbError>;

    /// Closes or drops a statement
    fn free_statement(
        &mut self,
        stmt_handle: &mut Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError>;

    /// Execute the prepared statement with parameters
    fn execute(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<(), FbError>;

    /// Execute the prepared statement
    /// with input and output parameters.
    ///
    /// The output parameters will be returned
    /// as in the Result
    fn execute2(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<Vec<Column>, FbError>;

    /// Fetch rows from the executed statement, coercing the types
    /// according to the provided blr
    fn fetch(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
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

impl FromStr for Dialect {
    type Err = FbError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1" => Ok(Dialect::D1),
            "2" => Ok(Dialect::D2),
            "3" => Ok(Dialect::D3),
            _ => Err(FbError::from(format!(
                "'{}' doesn't represent any dialect",
                s
            ))),
        }
    }
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
