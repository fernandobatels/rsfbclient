//!
//! Some API utils
//!

use crate::{FbError, Queryable};

/// Infos about the server, database, engine...
pub trait SystemInfos {
    /// Return the current connected database name
    fn db_name(&mut self) -> Result<String, FbError>;

    /// Return the current server version
    fn server_engine(&mut self) -> Result<EngineVersion, FbError>;
}

#[derive(PartialOrd, PartialEq, Eq, Debug, Copy, Clone)]
#[repr(u8)]
pub enum EngineVersion {
    V1 = 1,
    V2 = 2,
    V3 = 3,
    V4 = 4,
}

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
            return match &version.get(0..2) {
                Some("4.") => Ok(EngineVersion::V4),
                Some("3.") => Ok(EngineVersion::V3),
                Some("2.") => Ok(EngineVersion::V2),
                _ => Err(FbError::from(format!("Version not detected: {}", version))),
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
}
