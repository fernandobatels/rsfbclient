//!
//! Rust Firebird Client
//!
//! R2D2 Connection Pool
//!

use crate::{Connection, ConnectionBuilder, FbError, Transaction};

pub struct FirebirdConnectionManager {
    conn_builder: ConnectionBuilder,
}

impl FirebirdConnectionManager {
    pub fn new(conn_builder: ConnectionBuilder) -> Self {
        Self { conn_builder }
    }
}

impl r2d2::ManageConnection for FirebirdConnectionManager {
    type Connection = Connection<rsfbclient_native::NativeFbClient>; //TODO: Fix
    type Error = FbError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        self.conn_builder.connect()
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
