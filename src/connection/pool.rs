//!
//! Rust Firebird Client
//!
//! R2D2 Connection Pool
//!

use super::{FirebirdClientDbOps, FirebirdClientFactory};
use crate::{Connection, FbError, Transaction};

/// A manager for connection pools. Requires the `pool` feature.
pub struct FirebirdConnectionManager<F: FirebirdClientFactory> {
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
        Transaction::new(&conn)?;
        Ok(())
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}
