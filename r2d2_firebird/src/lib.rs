//!
//! Rust Firebird Client
//!
//! R2D2 Connection Pool
//!

use rsfbclient::{Connection, FbError, FirebirdClientFactory, Transaction};
use rsfbclient_core::{FirebirdClientDbOps, TransactionConfiguration};

/// A manager for connection pools. Requires the `pool` feature.
pub struct FirebirdConnectionManager<F>
where
    F: FirebirdClientFactory,
{
    client_factory: F,
}

impl<F> FirebirdConnectionManager<F>
where
    F: FirebirdClientFactory,
{
    pub fn new(client_factory: F) -> Self {
        Self { client_factory }
    }
}

impl<F: FirebirdClientFactory + 'static> r2d2::ManageConnection for FirebirdConnectionManager<F>
where
    F: Send + Sync,
    <F::C as FirebirdClientDbOps>::AttachmentConfig: Send + Sync + Clone, // TODO: Allow embedded database
{
    type Connection = Connection<F::C>;
    type Error = FbError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let cli = self.client_factory.new_instance()?;
        Connection::open(cli, self.client_factory.get_conn_conf())
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        // If it can start a transaction, we are ok
        Transaction::new(conn, TransactionConfiguration::default())?;
        Ok(())
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

// Implementation for Diesel
#[cfg(feature = "diesel_pool")]
mod diesel_manager {
    use diesel::prelude::*;
    use diesel::{sql_query, Connection, ConnectionError};
    use rsfbclient_diesel::FbConnection;

    pub struct DieselConnectionManager {
        connection_string: String,
    }

    impl DieselConnectionManager {
        pub fn new(database_url: &str) -> Self {
            Self {
                connection_string: database_url.to_string(),
            }
        }
    }

    impl r2d2::ManageConnection for DieselConnectionManager {
        type Connection = FbConnection;
        type Error = ConnectionError;

        fn connect(&self) -> Result<Self::Connection, Self::Error> {
            FbConnection::establish(&self.connection_string)
        }

        fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
            sql_query("SELECT 1 FROM RDB$DATABASE")
                .execute(conn)
                .map(|_| ())
                .map_err(|_| {
                    ConnectionError::BadConnection("Diesel pooled connection check failed.".into())
                })
        }

        fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
            false
        }
    }
}

#[cfg(feature = "diesel_pool")]
pub use diesel_manager::DieselConnectionManager;
