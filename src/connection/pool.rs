//!
//! Rust Firebird Client
//!
//! R2D2 Connection Pool
//!

use crate::{Connection, ConnectionBuilder, FbError, Transaction};
use rsfbclient_core::{FirebirdClient, FirebirdClientRemoteAttach};

pub struct FirebirdConnectionManager<C: FirebirdClient> {
    conn_builder: ConnectionBuilder<C>,
}

impl<C> FirebirdConnectionManager<C>
where
    C: FirebirdClient,
{
    pub fn new(conn_builder: ConnectionBuilder<C>) -> Self {
        Self { conn_builder }
    }
}

impl<C> r2d2::ManageConnection for FirebirdConnectionManager<C>
where
    C: FirebirdClient + FirebirdClientRemoteAttach + 'static, // TODO: Allow embedded database
{
    type Connection = Connection<C>;
    type Error = FbError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        self.conn_builder.connect()
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        // If it can start a transaction, we are ok
        Transaction::new(conn)?;
        Ok(())
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}
