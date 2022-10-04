//! Firebird transaction types
//!
//! More info about transactions in firebird:
//! https://firebirdsql.org/file/documentation/html/en/refdocs/fblangref30/firebird-30-language-reference.html#fblangref30-transacs

use crate::*;

/// Transaction isolation level
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum TrIsolationLevel {
    /// Transactions can't see alterations commited after they started
    Concurrency,
    /// Table locking
    Consistency,
    /// Transactions can see alterations commited after they started
    ReadCommited(TrRecordVersion),
}

impl Default for TrIsolationLevel {
    fn default() -> Self {
        Self::ReadCommited(TrRecordVersion::default())
    }
}

impl From<TrIsolationLevel> for u8 {
    fn from(tp: TrIsolationLevel) -> Self {
        match tp {
            TrIsolationLevel::Concurrency => ibase::isc_tpb_concurrency as u8,
            TrIsolationLevel::Consistency => ibase::isc_tpb_consistency as u8,
            TrIsolationLevel::ReadCommited(_) => ibase::isc_tpb_read_committed as u8,
        }
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

/// Lock resolution modes
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum TrLockResolution {
    /// In the NO WAIT mode, a transaction will immediately throw a database exception if a conflict occurs
    NoWait,
    /// In the WAIT model, transaction will wait till the other transaction has finished.
    ///
    /// If a TIMEOUT is specified for the WAIT transaction, waiting will continue only for the number of seconds specified
    Wait(Option<u32>)
}

impl Default for TrLockResolution {
    fn default() -> Self {
        Self::Wait(None)
    }
}

impl From<TrLockResolution> for u8 {
    fn from(tp: TrLockResolution) -> Self {
        match tp {
            TrLockResolution::NoWait => ibase::isc_tpb_nowait as u8,
            TrLockResolution::Wait(_) => ibase::isc_tpb_wait as u8,
        }
    }
}

/// Data access mode
#[repr(u8)]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum TrDataAccessMode {
    /// Operations in the context of this transaction can be both read operations and data update operations
    ReadWrite = ibase::isc_tpb_write as u8,
    /// Only SELECT operations can be executed in the context of this transaction
    ReadOnly = ibase::isc_tpb_read as u8,
}

impl Default for TrDataAccessMode {
    fn default() -> Self {
        Self::ReadWrite
    }
}

/// Record version isolation
#[repr(u8)]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum TrRecordVersion {
    /// Is a kind of two-phase locking mechanism: it will make the transaction unable to write to any row that has an update pending from another transaction
    RecordVersion = ibase::isc_tpb_rec_version as u8,
    /// The transaction reads the latest committed version of the row, regardless of other pending versions of the row.
    NoRecordVersion = ibase::isc_tpb_no_rec_version as u8,
}

impl Default for TrRecordVersion {
    fn default() -> Self {
        Self::NoRecordVersion
    }
}

/// Parameters of a new transaction
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct TransactionConfiguration {
    pub data_access: TrDataAccessMode,
    pub isolation: TrIsolationLevel,
    pub lock_resolution: TrLockResolution,
}

impl Default for TransactionConfiguration {
    fn default() -> Self {
        Self {
            data_access: TrDataAccessMode::default(),
            isolation: TrIsolationLevel::default(),
            lock_resolution: TrLockResolution::default(),
        }
    }
}
