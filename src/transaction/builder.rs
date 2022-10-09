//! Transaction configuration builder

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

    /// Disable the wait mode on lock resolution
    pub fn no_wait(&mut self) -> &mut Self {
        self.inner.lock_resolution = TrLockResolution::NoWait;

        self
    }

    /// Enable wait mode with a specific time on lock resolution
    pub fn wait(&mut self, until: u32) -> &mut Self {
        self.inner.lock_resolution = TrLockResolution::Wait(Some(until));

        self
    }

    /// Enable wait forever on lock resolution
    pub fn wait_infinitely(&mut self) -> &mut Self {
        self.inner.lock_resolution = TrLockResolution::Wait(None);

        self
    }

    /// Enable read only data access
    pub fn read_only(&mut self) -> &mut Self {
        self.inner.data_access = TrDataAccessMode::ReadOnly;

        self
    }

    /// Enable read write data access
    pub fn read_write(&mut self) -> &mut Self {
        self.inner.data_access = TrDataAccessMode::ReadWrite;

        self
    }

    /// Enable consistency(table lock) isolation level
    pub fn with_consistency(&mut self) -> &mut Self {
        self.inner.isolation = TrIsolationLevel::Consistency;

        self
    }

    /// Enable concurrency(Transactions can't see alterations
    /// commited after they started) isolation level
    pub fn with_concurrency(&mut self) -> &mut Self {
        self.inner.isolation = TrIsolationLevel::Concurrency;

        self
    }

    /// Enable read commited(Transactions can see alterations
    /// commited after they started) isolation level
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
