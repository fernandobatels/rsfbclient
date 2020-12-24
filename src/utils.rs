//!
//! Some API utils
//!

use crate::{FbError, Queryable};
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
            EngineVersion::V4 => 4,
        }
    }
}

impl PartialEq for EngineVersion {
    fn eq(&self, other: &Self) -> bool {
        (*self as u8) == (*other as u8)
    }
}

impl PartialOrd for EngineVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some((*self as u8).cmp(&(*other as u8)))
    }
}

impl Eq for EngineVersion {}

impl<T> SystemInfos for T
where
    T: Queryable,
{
    fn db_name(&mut self) -> Result<String, FbError> {
        let (name,): (String,) = self
            .query_first(
                "SELECT rdb$get_context('SYSTEM', 'DB_NAME') from rdb$database;",
                (),
            )?
            .unwrap();

        Ok(name)
    }

    fn server_engine(&mut self) -> Result<EngineVersion, FbError> {
        let row: Option<(String,)> = self.query_first(
            "SELECT rdb$get_context('SYSTEM', 'ENGINE_VERSION') from rdb$database;",
            (),
        )?;

        if let Some((version,)) = row {
            return match version {
                ver if ver.starts_with("4.") => Ok(EngineVersion::V4),
                ver if ver.starts_with("3.") => Ok(EngineVersion::V3),
                ver if ver.starts_with("2.") => Ok(EngineVersion::V2),
                ver => Err(FbError::from(format!("Version not detected: {}", ver))),
            };
        }

        // ENGINE_VERSION is only avaliable after fb 2.1
        Ok(EngineVersion::V1)
    }
}

#[cfg(test)]
mk_tests_default! {
    use crate::*;

    #[test]
    fn server_engine() -> Result<(), FbError> {

        let mut conn = cbuilder().connect()?;

        let version = conn.server_engine()?;

        // Our current CI versions..
        assert!([EngineVersion::V2, EngineVersion::V3, EngineVersion::V4].contains(&version));

        Ok(())
    }

    #[test]
    fn eng_version() {
        assert_eq!(EngineVersion::V1, EngineVersion::V1);
        assert!(EngineVersion::V1 == EngineVersion::V1);

        assert!(EngineVersion::V1 >= EngineVersion::V1);
        assert!(EngineVersion::V3 > EngineVersion::V2);
    }
}
