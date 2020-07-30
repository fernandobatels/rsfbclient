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
    type Connection = Connection;
    type Error = FbError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        self.conn_builder.connect()
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        let mut tr = Transaction::new(&conn)?;
        let mut stmt = tr.prepare("SELECT 1 FROM RDB$DATABASE")?;
        stmt.query(&mut tr, ())?;

        Ok(())
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}
