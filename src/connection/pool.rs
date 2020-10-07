//!
//! Rust Firebird Client
//!
//! R2D2 Connection Pool
//!

use super::{ConnectionConfiguration, FirebirdClientFactory};
use crate::{Connection, FbError, Transaction};

pub struct FirebirdConnectionManager<F: FirebirdClientFactory> {
    client_factory: F,
    conn_conf: ConnectionConfiguration<F::C>,
}

impl<F: FirebirdClientFactory> FirebirdConnectionManager<F> {
    pub fn new(client_factory: F, conn_conf: ConnectionConfiguration<F::C>) -> Self {
        Self {
            client_factory,
            conn_conf,
        }
    }
}

impl<F: FirebirdClientFactory + 'static> r2d2::ManageConnection for FirebirdConnectionManager<F>
// TODO: Allow embedded database
{
    type Connection = Connection<F::C>;
    type Error = FbError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let cli = self.client_factory.new()?;
        Connection::open(cli, &self.conn_conf)
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
