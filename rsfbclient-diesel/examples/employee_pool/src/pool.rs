use diesel::prelude::*;
use diesel::{sql_query, Connection, ConnectionError};
use r2d2::{ManageConnection};
use rsfbclient_diesel::FbConnection;

#[derive(Debug, Clone)]
pub struct FirebirdConnectionManager {
    database_url: String,
}

impl FirebirdConnectionManager {
    pub fn new(database_url: &str) -> Self {
        Self {
            database_url: database_url.to_string(),
        }
    }
}

impl ManageConnection for FirebirdConnectionManager {
    type Connection = FbConnection;
    type Error = ConnectionError;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        FbConnection::establish(&self.database_url)
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        sql_query("SELECT 1 FROM RDB$DATABASE")
            .execute(conn)
            .map(|_| ())
            .map_err(|_| ConnectionError::BadConnection("Connection check failed.".into()))
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}
