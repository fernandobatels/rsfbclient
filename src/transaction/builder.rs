//! Transaction configuration builder
//!
//! More info about transactions in firebird:
//! <https://firebirdsql.org/file/documentation/html/en/refdocs/fblangref30/firebird-30-language-reference.html#fblangref30-transacs>

use rsfbclient_core::*;

/// Builder for transaction configuration
pub struct TransactionConfigurationBuilder {
    inner: TransactionConfiguration,
}

impl TransactionConfigurationBuilder {
    pub fn init() -> Self {
        Self {
            inner: TransactionConfiguration::default(),
        }
    }

    /// Disable the wait mode on lock resolution.
    ///
    /// In the NO WAIT mode, a transaction will immediately throw a database exception if a conflict occurs
    pub fn no_wait(&mut self) -> &mut Self {
        self.inner.lock_resolution = TrLockResolution::NoWait;

        self
    }

    /// Enable wait mode with a specific time on lock resolution.
    ///
    /// In the WAIT model, transaction will wait till the other transaction has finished.
    ///
    /// Waiting will continue only for the number of seconds specified
    pub fn wait(&mut self, until: u32) -> &mut Self {
        self.inner.lock_resolution = TrLockResolution::Wait(Some(until));

        self
    }

    /// Enable wait forever on lock resolution
    ///
    /// In the WAIT model, transaction will wait till the other transaction has finished.
    pub fn wait_infinitely(&mut self) -> &mut Self {
        self.inner.lock_resolution = TrLockResolution::Wait(None);

        self
    }

    /// Enable read only data access
    ///
    /// Only SELECT operations can be executed in the context of this transaction
    pub fn read_only(&mut self) -> &mut Self {
        self.inner.data_access = TrDataAccessMode::ReadOnly;

        self
    }

    /// Enable read write data access
    ///
    /// Operations in the context of this transaction can be both read operations and data update operations
    pub fn read_write(&mut self) -> &mut Self {
        self.inner.data_access = TrDataAccessMode::ReadWrite;

        self
    }

    /// Enable consistency(table lock) isolation level
    pub fn with_consistency(&mut self) -> &mut Self {
        self.inner.isolation = TrIsolationLevel::Consistency;

        self
    }

    /// Enable concurrency isolation level
    ///
    /// Transactions can't see alterations commited after they started
    pub fn with_concurrency(&mut self) -> &mut Self {
        self.inner.isolation = TrIsolationLevel::Concurrency;

        self
    }

    /// Enable read commited isolation level
    ///
    /// Transactions can see alterations commited after they started
    pub fn with_read_commited(&mut self, rec: TrRecordVersion) -> &mut Self {
        self.inner.isolation = TrIsolationLevel::ReadCommited(rec);

        self
    }

    pub fn build(&self) -> TransactionConfiguration {
        self.inner
    }
}

/// Get a new instance of TransactionConfigurationBuilder
pub fn transaction_builder() -> TransactionConfigurationBuilder {
    TransactionConfigurationBuilder::init()
}

#[cfg(test)]
mod tests {

    use crate::prelude::*;

    #[test]
    pub fn transaction_builder_no_wait() {
        let conf = transaction_builder().no_wait().build();
        assert_eq!(
            TransactionConfiguration {
                lock_resolution: TrLockResolution::NoWait,
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_wait_seconds() {
        let conf = transaction_builder().wait(32).build();
        assert_eq!(
            TransactionConfiguration {
                lock_resolution: TrLockResolution::Wait(Some(32)),
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_wait_infinity() {
        let conf = transaction_builder().wait_infinitely().build();
        assert_eq!(
            TransactionConfiguration {
                lock_resolution: TrLockResolution::Wait(None),
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_read_only() {
        let conf = transaction_builder().read_only().build();
        assert_eq!(
            TransactionConfiguration {
                data_access: TrDataAccessMode::ReadOnly,
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_read_write() {
        let conf = transaction_builder().read_write().build();
        assert_eq!(
            TransactionConfiguration {
                data_access: TrDataAccessMode::ReadWrite,
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_consistency() {
        let conf = transaction_builder().with_consistency().build();
        assert_eq!(
            TransactionConfiguration {
                isolation: TrIsolationLevel::Consistency,
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_concurrency() {
        let conf = transaction_builder().with_concurrency().build();
        assert_eq!(
            TransactionConfiguration {
                isolation: TrIsolationLevel::Concurrency,
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_read_commited() {
        let conf = transaction_builder()
            .with_read_commited(TrRecordVersion::NoRecordVersion)
            .build();
        assert_eq!(
            TransactionConfiguration {
                isolation: TrIsolationLevel::ReadCommited(TrRecordVersion::NoRecordVersion),
                ..TransactionConfiguration::default()
            },
            conf
        );
    }

    #[test]
    pub fn transaction_builder_full_custom() {
        let conf = transaction_builder()
            .with_read_commited(TrRecordVersion::NoRecordVersion)
            .read_only()
            .no_wait()
            .build();
        assert_eq!(
            TransactionConfiguration {
                isolation: TrIsolationLevel::ReadCommited(TrRecordVersion::NoRecordVersion),
                data_access: TrDataAccessMode::ReadOnly,
                lock_resolution: TrLockResolution::NoWait
            },
            conf
        );
    }
}
