//! Query tests

use super::schema;
use crate::fb::FbConnection;
use crate::prelude::*;

#[test]
fn filter() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Luis A')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'Luis B')")
        .ok();
    conn.execute("insert into users (id, name) values (3, 'Luis C')")
        .ok();

    let users = schema::users::table
        .filter(schema::users::columns::id.eq(1))
        .or_filter(schema::users::columns::id.eq(3))
        .load::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(2, users.len());
    let mut users = users.iter();

    let user = users.next().unwrap();
    assert_eq!(user.id, 1);
    assert_eq!(user.name, "Luis A");

    let user = users.next().unwrap();
    assert_eq!(user.id, 3);
    assert_eq!(user.name, "Luis C");

    Ok(())
}

#[test]
fn order() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'aa')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'bb')")
        .ok();
    conn.execute("insert into users (id, name) values (3, 'cc')")
        .ok();

    let users = schema::users::table
        .filter(schema::users::columns::id.ge(2))
        .order(schema::users::columns::name.desc())
        .load::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(2, users.len());
    let mut users = users.iter();

    let user = users.next().unwrap();
    assert_eq!(user.id, 3);
    assert_eq!(user.name, "cc");

    let user = users.next().unwrap();
    assert_eq!(user.id, 2);
    assert_eq!(user.name, "bb");

    Ok(())
}

#[test]
fn limit_offset() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'aa')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'bb')")
        .ok();
    conn.execute("insert into users (id, name) values (3, 'cc')")
        .ok();

    let users = schema::users::table
        .limit(2)
        .load::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(2, users.len());
    let mut users = users.iter();

    let user = users.next().unwrap();
    assert_eq!(user.id, 1);
    assert_eq!(user.name, "aa");

    let user = users.next().unwrap();
    assert_eq!(user.id, 2);
    assert_eq!(user.name, "bb");

    let users = schema::users::table
        .limit(2)
        .offset(1)
        .load::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(2, users.len());
    let mut users = users.iter();

    let user = users.next().unwrap();
    assert_eq!(user.id, 2);
    assert_eq!(user.name, "bb");

    let user = users.next().unwrap();
    assert_eq!(user.id, 3);
    assert_eq!(user.name, "cc");

    Ok(())
}

#[test]
fn find() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'aa')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'bb')")
        .ok();
    conn.execute("insert into users (id, name) values (3, 'cc')")
        .ok();

    let user = schema::users::table
        .find(3)
        .get_result::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(user.id, 3);
    assert_eq!(user.name, "cc");

    let user = schema::users::table
        .find(4)
        .get_result::<schema::User>(&conn)
        .optional()
        .map_err(|e| e.to_string())?;

    assert!(user.is_none());

    Ok(())
}

#[test]
fn distinct() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'cc')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'bb')")
        .ok();
    conn.execute("insert into users (id, name) values (3, 'cc')")
        .ok();

    let names = schema::users::table
        .select(schema::users::columns::name)
        .distinct()
        .load::<String>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(2, names.len());
    assert_eq!(vec!["bb", "cc"], names);

    Ok(())
}
