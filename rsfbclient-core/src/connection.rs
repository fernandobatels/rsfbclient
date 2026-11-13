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

/// Responsible for database administration and attachment/detachment
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
        dialect: Dialect,
        no_db_triggers: bool,
    ) -> Result<Self::DbHandle, FbError>;

    /// Disconnect from the database
    fn detach_database(&mut self, db_handle: &mut Self::DbHandle) -> Result<(), FbError>;

    /// Drop the database
    fn drop_database(&mut self, db_handle: &mut Self::DbHandle) -> Result<(), FbError>;

    /// Database information (`isc_database_info`): `items` is the list of
    /// item tags to ask for, ending in `isc_info_end`; the answer is the raw
    /// reply stream (see `crate::dbinfo`).
    fn database_info(
        &mut self,
        db_handle: &mut Self::DbHandle,
        items: &[u8],
    ) -> Result<Vec<u8>, FbError>;

    /// Bound every socket read and write of the attachment by `timeout`
    /// (`None`: wait as long as it takes, the default). A read or write that
    /// runs out fails with an io error and leaves the attachment unusable: it
    /// is for asking whether the server is still there, then dropping it if
    /// not. False when the client can't (the native one: fbclient owns its
    /// socket).
    fn set_io_timeout(&mut self, _timeout: Option<std::time::Duration>) -> Result<bool, FbError> {
        Ok(false)
    }

    /// Create the database and attach
    /// Returns a database handle on success
    fn create_database(
        &mut self,
        config: &Self::AttachmentConfig,
        page_size: Option<u32>,
        dialect: Dialect,
    ) -> Result<Self::DbHandle, FbError>;
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
        confs: TransactionConfiguration,
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
    /// and returns the affected rows count
    fn execute(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<usize, FbError>;

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

    /// Describe the output columns of a prepared statement: result-set name,
    /// source relation / context alias / field, and type.
    ///
    /// The pure-rust client answers from the prepare response it already
    /// parsed, so asking costs no round trip; the native client reads the
    /// XSQLDA and asks the library for the context aliases.
    fn describe_statement(
        &mut self,
        stmt_handle: &mut Self::StmtHandle,
    ) -> Result<Vec<ColumnDesc>, FbError>;

    /// The access plan the server chose for a prepared statement (see
    /// [`StmtPlan`]): the legacy one-line `PLAN` and, on Firebird 3+, the
    /// explained tree. One info round trip; the statement is not executed.
    fn statement_plan(&mut self, stmt_handle: &mut Self::StmtHandle) -> Result<StmtPlan, FbError>;
}

/// What the server knows about one output column of a prepared statement:
/// where it comes from and how it is typed. Obtained on request through
/// [`FirebirdClientSqlOps::describe_statement`].
///
/// For `SELECT c.cli_nom AS nom FROM clients c`, the column describes as
/// `name = "NOM"`, `field = Some("CLI_NOM")`, `relation = Some("CLIENTS")`,
/// `relation_alias = Some("C")`. An expression such as `1 + 1` has no
/// field, relation or alias.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnDesc {
    /// Name in the result set: the alias when one was given, else the field name.
    pub name: String,
    /// The underlying column (`RDB$FIELD_NAME`); `None` for an expression.
    pub field: Option<String>,
    /// The relation (table, view or procedure) the column comes from; `None`
    /// for an expression.
    pub relation: Option<String>,
    /// The context alias the relation has in the statement: `C` for
    /// `FROM clients c`, the relation name itself when none was written.
    /// `None` when the client could not find out.
    pub relation_alias: Option<String>,
    /// The relation's owner.
    pub owner: Option<String>,
    /// Firebird SQL type code (`SQL_TEXT`, `SQL_BLOB`, …) without the
    /// nullable bit.
    pub sql_type: u32,
    /// BLOB sub-type (0 binary, 1 text), or the character set id for text.
    pub sub_type: i16,
    /// Decimal scale (negative for fixed-point numbers).
    pub scale: i16,
    /// Length of the column data in bytes.
    pub length: i16,
    /// Whether the column can be NULL.
    pub nullable: bool,
}

impl ColumnDesc {
    /// The column is a BLOB (binary or text).
    pub fn is_blob(&self) -> bool {
        self.sql_type == ibase::SQL_BLOB
    }

    /// The column is computed, not read from a relation.
    pub fn is_expression(&self) -> bool {
        self.relation.is_none()
    }
}

/// Firebird hands back empty strings for the parts an expression does not have.
pub fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Firebird base event API
pub trait FirebirdClientDbEvents: FirebirdClientDbOps {
    /// Wait for an event to be posted on database
    fn wait_for_event(
        &mut self,
        db_handle: &mut Self::DbHandle,
        name: String,
    ) -> Result<(), FbError>;
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
#[repr(u8)]
/// Firebird sql dialect
pub enum Dialect {
    D1 = 1,
    D2 = 2,
    #[default]
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
    Select = 1,        // isc_info_sql_stmt_select
    Insert = 2,        // isc_info_sql_stmt_insert
    Update = 3,        // isc_info_sql_stmt_update
    Delete = 4,        // isc_info_sql_stmt_delete
    Ddl = 5,           // isc_info_sql_stmt_ddl
    GetSegment = 6,    // isc_info_sql_stmt_get_segment
    PutSegment = 7,    // isc_info_sql_stmt_put_segment
    ExecProcedure = 8, // isc_info_sql_stmt_exec_procedure
    StartTrans = 9,    // isc_info_sql_stmt_start_trans
    Commit = 10,       // isc_info_sql_stmt_commit
    Rollback = 11,     // isc_info_sql_stmt_rollback
    SelectForUpd = 12, // isc_info_sql_stmt_select_for_upd
    SetGenerator = 13, // isc_info_sql_stmt_set_generator
    Savepoint = 14,    // isc_info_sql_stmt_savepoint
}
