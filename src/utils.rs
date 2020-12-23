//!
//! Some API utils
//!

use rsfbclient_core::{FirebirdClient};
use crate::{FbError, Connection, SimpleConnection, Queryable};
use std::cmp::*;
use std::convert::From;

/// Infos about the server, database, engine...
pub trait SystemInfos {

    /// Return the current connected database name
    fn db_name(&mut self) -> Result<String, FbError>;

    /// Return the current server version
    fn server_engine(&mut self) -> Result<EngineVersion, FbError>;
}

#[derive(Debug, Copy, Clone)]
pub enum EngineVersion {
    V1,
    V2,
    V3,
    V4,
}

impl From<EngineVersion> for u8 {
    fn from(eg: EngineVersion) -> Self {
        match eg {
            EngineVersion::V1 => 1,
            EngineVersion::V2 => 2,
            EngineVersion::V3 => 3,
            EngineVersion::V4 => 4
        }
    }
}

impl PartialEq for EngineVersion {
    fn eq(&self, other: &Self) -> bool {
        (*self as u8) == (*other as u8)
    }
}

impl Eq for EngineVersion {}

impl<C: FirebirdClient> SystemInfos for Connection<C> {

    fn db_name(&mut self) -> Result<String, FbError> {
        let (name,): (String,) = self.query_first(
            "SELECT rdb$get_context('SYSTEM', 'DB_NAME') from rdb$database;",
            (),
        )?.unwrap();

        Ok(name)
    }

    fn server_engine(&mut self) -> Result<EngineVersion, FbError> {
        todo!()
    }
}


#[cfg(test)]
pub(crate) mod test {
    use crate::*;

    #[test]
    fn eng_version_cmp() {
        assert_eq!(EngineVersion::V1, EngineVersion::V1);
    }
}
