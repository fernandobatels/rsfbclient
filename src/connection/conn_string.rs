//! Connection string parser

use crate::*;
use std::str::FromStr;
use url::Url;

pub struct ConnStringSettings {
    pub user: Option<String>,
    pub pass: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub db_name: String,
    pub charset: Option<Charset>,
    pub dialect: Option<Dialect>,
    pub lib_path: Option<String>,
    pub stmt_cache_size: Option<usize>,
}

/// Parse the connection string.
///
/// Basic string syntax: `firebird://{user}:{pass}@{host}:{port}/{db_name}?{options}`
pub fn parse(sconn: &str) -> Result<ConnStringSettings, FbError> {
    let url = Url::parse(sconn)
        .map_err(|e| FbError::from(format!("Error on parse the string: {}", e)))?;

    if url.scheme().to_lowercase() != "firebird" {
        return Err(FbError::from(
            "The string must start with the prefix 'firebird://'",
        ));
    }

    let user = match url.username() {
        "" => None,
        u => Some(u.to_string()),
    };

    let pass = url.password().map(|p| p.to_string());

    let mut host = url.host().map(|h| h.to_string());

    let port = url.port();

    let mut db_name = match url.path() {
        "" => None,
        db => {
            if db.starts_with('/') && url.has_host() {
                Some(db.replacen("/", "", 1))
            } else {
                Some(db.to_string())
            }
        }
    };

    match (&host, &db_name) {
        // In the embedded case with a windows path,
        // the lib will return the drive in the host,
        // because of ':' char.
        //
        // Example: firebird://c:/a/b/c.fdb
        // We get:
        //  - host: c
        //  - port: None
        //  - db_name: a/b/c.fdb
        (Some(h), Some(db)) => {
            if h.len() == 1 && user.is_none() && pass.is_none() && port.is_none() {
                db_name = Some(format!("{}:/{}", h, db));
                host = None;
            }
        }
        // When we have an embedded path, but only
        // with the filename. In this cases, the lib
        // will return the db path in the host.
        //
        // Example: firebird://abc.fdb
        // We get:
        //  - host: abc.fdb
        //  - db_name: None
        (Some(h), None) => {
            if user.is_none() && pass.is_none() && port.is_none() {
                db_name = Some(h.to_string());
                host = None;
            }
        }
        _ => {}
    }

    let db_name = db_name.ok_or_else(|| FbError::from("The database name/path is required"))?;

    let mut lib_path = None;
    let mut dialect = None;
    let mut charset = None;
    let mut stmt_cache_size = None;

    for (param, val) in url.query_pairs() {
        match param.to_string().as_str() {
            "lib" => {
                lib_path = Some(val.to_string());
            }
            "dialect" => {
                dialect = match Dialect::from_str(&val) {
                    Ok(d) => Some(d),
                    _ => None,
                };
            }
            "charset" => {
                charset = match Charset::from_str(&val) {
                    Ok(d) => Some(d),
                    _ => None,
                };
            }
            "stmt_cache_size" => {
                stmt_cache_size = match val.parse::<usize>() {
                    Ok(v) => Some(v),
                    _ => None,
                };
            }
            _ => {}
        }
    }

    Ok(ConnStringSettings {
        user,
        pass,
        host,
        port,
        db_name,
        charset,
        dialect,
        lib_path,
        stmt_cache_size,
    })
}

#[cfg(test)]
mod test {
    use super::parse;
    use crate::*;

    #[test]
    fn params_combination() -> Result<(), FbError> {
        let conn = parse("firebird:///srv/db/database_name.fdb?lib=/tmp/fbclient.lib&stmt_cache_size=1&dialect=1&charset=utf8")?;

        assert_eq!(Some("/tmp/fbclient.lib".to_string()), conn.lib_path);
        assert_eq!(Some(1), conn.stmt_cache_size);
        assert_eq!(Some(Dialect::D1), conn.dialect);
        assert_eq!(Some(charset::UTF_8), conn.charset);

        Ok(())
    }

    #[test]
    fn stmt_cache_size() -> Result<(), FbError> {
        let conn = parse("firebird:///srv/db/database_name.fdb?lib=/tmp/fbclient.lib")?;
        assert_eq!(None, conn.stmt_cache_size);

        let conn = parse("firebird:///srv/db/database_name.fdb?stmt_cache_size=1")?;
        assert_eq!(Some(1), conn.stmt_cache_size);

        let conn = parse("firebird:///srv/db/database_name.fdb?stmt_cache_size=100")?;
        assert_eq!(Some(100), conn.stmt_cache_size);

        let conn = parse("firebird:///srv/db/database_name.fdb?stmt_cache_size=other")?;
        assert_eq!(None, conn.stmt_cache_size);

        Ok(())
    }

    #[test]
    fn charset() -> Result<(), FbError> {
        let conn = parse("firebird:///srv/db/database_name.fdb?lib=/tmp/fbclient.lib")?;
        assert_eq!(None, conn.charset);

        let conn = parse("firebird:///srv/db/database_name.fdb?charset=utf8")?;
        assert_eq!(Some(charset::UTF_8), conn.charset);

        let conn = parse("firebird:///srv/db/database_name.fdb?charset=utf-8")?;
        assert_eq!(Some(charset::UTF_8), conn.charset);

        let conn = parse("firebird:///srv/db/database_name.fdb?charset=utf_8")?;
        assert_eq!(Some(charset::UTF_8), conn.charset);

        let conn = parse("firebird:///srv/db/database_name.fdb?charset=UTF_8")?;
        assert_eq!(Some(charset::UTF_8), conn.charset);

        Ok(())
    }

    #[test]
    fn dialect() -> Result<(), FbError> {
        let conn = parse("firebird:///srv/db/database_name.fdb?lib=/tmp/fbclient.lib")?;
        assert_eq!(None, conn.dialect);

        let conn = parse("firebird:///srv/db/database_name.fdb?dialect=1")?;
        assert_eq!(Some(Dialect::D1), conn.dialect);

        let conn = parse("firebird:///srv/db/database_name.fdb?dialect=2")?;
        assert_eq!(Some(Dialect::D2), conn.dialect);

        let conn = parse("firebird:///srv/db/database_name.fdb?dialect=3")?;
        assert_eq!(Some(Dialect::D3), conn.dialect);

        let conn = parse("firebird:///srv/db/database_name.fdb?dialect=4")?;
        assert_eq!(None, conn.dialect);

        let conn = parse("firebird:///srv/db/database_name.fdb?dialect=other")?;
        assert_eq!(None, conn.dialect);

        Ok(())
    }

    #[test]
    fn dynload() -> Result<(), FbError> {
        let conn = parse("firebird:///srv/db/database_name.fdb?lib=/tmp/fbclient.lib")?;

        assert_eq!(Some("/tmp/fbclient.lib".to_string()), conn.lib_path);

        let conn = parse("firebird://c:/db/database_name.fdb?lib=/tmp/fbclient.lib&other=234")?;

        assert_eq!(Some("/tmp/fbclient.lib".to_string()), conn.lib_path);

        let conn = parse("firebird://c:/db/database_name.fdb?lib=fbclient.lib")?;

        assert_eq!(Some("fbclient.lib".to_string()), conn.lib_path);

        Ok(())
    }

    #[test]
    fn embedded() -> Result<(), FbError> {
        let conn = parse("firebird:///srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(None, conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://c:/db/database_name.fdb?dialect=3")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(None, conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://database_name.fdb")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(None, conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://database_name")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(None, conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("database_name".to_string(), conn.db_name);

        let conn = parse("firebird://database_name.fdb?dialect=3")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(None, conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }

    #[test]
    fn user() -> Result<(), FbError> {
        // no user or pass
        let conn = parse("firebird://192.168.0.1//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        // no user or pass
        let conn = parse("firebird://192.168.0.1:3050//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        // only user provided
        let conn = parse("firebird://username@192.168.0.1:3050/c:/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        // no user or pass, and a simple db path
        let conn = parse("firebird://localhost:3050/database_name.fdb")?;

        assert_eq!(None, conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        // only user provided, but with a blank ':' char
        let conn = parse("firebird://username:@192.168.0.1:3050/c:/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(None, conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }

    #[test]
    fn ipv4() -> Result<(), FbError> {
        let conn =
            parse("firebird://username:password@192.168.0.1//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse(
            "firebird://username:password@192.168.0.1:3050/c:/db/database_name.fdb?dialect=3",
        )?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }

    #[test]
    fn no_host_port() -> Result<(), FbError> {
        let conn =
            parse("firebird://username:password@localhost//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn =
            parse("firebird://username:password@localhost/c:/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://username:password@localhost/database_name?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("database_name".to_string(), conn.db_name);

        let conn = parse("firebird://username:password@localhost/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        // host provided, but with a blank ':' char in the port section
        let conn = parse("firebird://username:password@localhost:/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }

    #[test]
    fn database_fullpath() -> Result<(), FbError> {
        let conn = parse(
            "firebird://username:password@localhost:3050//srv/db/database_name.fdb?dialect=3",
        )?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn =
            parse("firebird://username:password@localhost:3050/c:/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://username:password@localhost:3050/c:/db/database_name.fdb")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }

    #[test]
    fn basic() -> Result<(), FbError> {
        let conn = parse("firebird://username:password@localhost:3050/database_name?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("database_name".to_string(), conn.db_name);

        let conn =
            parse("firebird://username:password@localhost:3050/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }
}
