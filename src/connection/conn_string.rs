//! Connection string parser

use crate::*;
use regex::Regex;

pub struct ConnStringSettings {
    pub user: Option<String>,
    pub pass: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub db_name: String,
    pub charset: Option<Charset>,
    pub dialect: Option<Dialect>,
    pub others: Vec<(String, String)>,
}

/// Parse the connection string.
///
/// Basic string sintax: `firebird://{user}:{pass}@{host}:{port}/{db_name}?{options}`
pub fn parse<S: Into<String>>(conn_s: S) -> Result<ConnStringSettings, FbError> {
    let sconn = conn_s.into();

    if !sconn.starts_with("firebird://") {
        return Err(FbError::from(
            "The string must start with the prefix 'firebird://'",
        ));
    }

    let user = regex_find(r#"(?:(/))([[:alnum:]]+)(?:(:))"#, 2, &sconn)?;
    let pass = regex_find(r#"(?:(:))([[:alnum:]]+)(?:(@))"#, 2, &sconn)?;
    let host = regex_find(r#"(?:(@))([^/:]+)"#, 2, &sconn)?;
    let port = {
        let fport_op = regex_find(r#"(?:(:))([[:digit:]]+)(?:(/))"#, 2, &sconn)?;
        if let Some(fport) = fport_op {
            if let Ok(v) = fport.parse::<u16>() {
                Some(v)
            } else {
                None
            }
        } else {
            None
        }
    };
    let db_name = regex_find(r#"((?:@\w+/)|(?:[0-9]/))([^\?]+)"#, 2, &sconn)?
        .ok_or(FbError::from("The database name/path is required"))?;

    Ok(ConnStringSettings {
        user,
        pass,
        host,
        port,
        db_name,
        charset: None,
        dialect: None,
        others: vec![],
    })
}

fn regex_find(regex: &str, cap_pos: usize, sconn: &str) -> Result<Option<String>, FbError> {
    let caps = Regex::new(regex)
        .map_err(|e| FbError::from(format!("Error on start the regex: {}", e)))?
        .captures(sconn);

    if caps.is_none() {
        return Ok(None);
    }

    Ok(match caps.unwrap().get(cap_pos) {
        Some(m) => Some(m.as_str().to_string()),
        None => None,
    })
}

#[cfg(test)]
mod test {
    use super::parse;
    use crate::*;

    #[test]
    fn ipv4() -> Result<(), FbError> {
        let conn = parse("firebird://username:password@192.168.0.1//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://username:password@192.168.0.1:3050/c:/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("192.168.0.1".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("c:/db/database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }

    #[test]
    fn no_host_port() -> Result<(), FbError> {
        let conn = parse("firebird://username:password@localhost//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(None, conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://username:password@localhost/c:/db/database_name.fdb?dialect=3")?;

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
        let conn = parse("firebird://username:password@localhost:3050//srv/db/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("/srv/db/database_name.fdb".to_string(), conn.db_name);

        let conn = parse("firebird://username:password@localhost:3050/c:/db/database_name.fdb?dialect=3")?;

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

        let conn = parse("firebird://username:password@localhost:3050/database_name.fdb?dialect=3")?;

        assert_eq!(Some("username".to_string()), conn.user);
        assert_eq!(Some("password".to_string()), conn.pass);
        assert_eq!(Some("localhost".to_string()), conn.host);
        assert_eq!(Some(3050), conn.port);
        assert_eq!("database_name.fdb".to_string(), conn.db_name);

        Ok(())
    }
}
