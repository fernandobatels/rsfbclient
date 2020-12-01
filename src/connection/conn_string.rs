//! Connection string parser

use crate::*;
use regex::Regex;
use std::str::FromStr;

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
/// Basic string sintax: `firebird://{user}:{pass}@{host}:{port}/{db_name}?{options}`
pub fn parse(sconn: &str) -> Result<ConnStringSettings, FbError> {

    if !sconn.starts_with("firebird://") {
        return Err(FbError::from(
            "The string must start with the prefix 'firebird://'",
        ));
    }

    let user = regex_find(r#"(?:(/))([[:alnum:]]+)(?:.*)(?:@)"#, &sconn, 2, false)?;

    let pass = regex_find(r#"(?:(:))([[:alnum:]]+)(?:(@))"#, &sconn, 2, false)?;

    let host = regex_find(
        r#"((?:://)|(?:@))([^@/:]+)((?:\w:/)|(?::[[:digit:]])|(?:/))"#,
        &sconn,
        2,
        true,
    )?;

    let port = {
        let fport_op = regex_find(r#"(?:(:))([[:digit:]]+)(?:(/))"#, &sconn, 2, true)?;
        if let Some(fport) = fport_op {
            match fport.parse::<u16>() {
                Ok(v) => Some(v),
                _ => None,
            }
        } else {
            None
        }
    };

    let db_name = {
        // remote host
        let mut db_name = regex_find(r#"((?:@\w+/)|(?:[0-9]/))([^\?]+)"#, &sconn, 2, true)?;
        if db_name.is_none() {
            // embedded
            db_name = regex_find(r#"(?://)([^\?]+)"#, &sconn, 1, true)?;
        }

        db_name.ok_or_else(|| FbError::from("The database name/path is required"))?
    };

    let lib_path = regex_find(r#"(?:\?)(?:.*)(lib=)([^&]+)"#, &sconn, 2, false)?;

    let dialect = {
        let fdialect_op = regex_find(r#"(?:\?)(?:.*)(dialect=)([[:digit:]])"#, &sconn, 2, false)?;
        if let Some(fdialect) = fdialect_op {
            match Dialect::from_str(&fdialect) {
                Ok(d) => Some(d),
                _ => None,
            }
        } else {
            None
        }
    };

    let charset = {
        let fcharset_op = regex_find(r#"(?:\?)(?:.*)(charset=)([^&]+)"#, &sconn, 2, false)?;
        if let Some(fcharset) = fcharset_op {
            match Charset::from_str(&fcharset) {
                Ok(d) => Some(d),
                _ => None,
            }
        } else {
            None
        }
    };

    let stmt_cache_size = {
        let fstmt_op = regex_find(
            r#"(?:\?)(?:.*)(stmt_cache_size=)([[:digit:]]+)"#,
            &sconn,
            2,
            true,
        )?;
        if let Some(fstmt) = fstmt_op {
            match fstmt.parse::<usize>() {
                Ok(v) => Some(v),
                _ => None,
            }
        } else {
            None
        }
    };

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

/// A regex util tool. Handles the matches
/// and groups
fn regex_find(
    pattern: &str,
    sconn: &str,
    group_i: usize,
    last_match: bool,
) -> Result<Option<String>, FbError> {
    let regex = Regex::new(pattern)
        .map_err(|e| FbError::from(format!("Error on start the regex: {}", e)))?;

    let mut caps = regex.captures_iter(sconn);
    let cap_op = if last_match { caps.last() } else { caps.next() };

    match cap_op {
        Some(cap) => Ok(match cap.get(group_i) {
            Some(m) => Some(m.as_str().to_string()),
            None => None,
        }),
        None => Ok(None),
    }
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
