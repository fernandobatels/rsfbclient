//! Basic crud tests

use super::schema;
use crate::prelude::*;
use diesel::prelude::*;

#[test]
fn insert() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://test.fdb").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    let user = schema::User {
        id: 10,
        name: "Pedro".to_string(),
    };

    diesel::insert_into(schema::users::table)
        .values(&user)
        .execute(&conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn update() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://test.fdb").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Luis')")
        .ok();

    diesel::update(schema::users::table)
        .set(schema::users::columns::name.eq("Fernando"))
        .filter(schema::users::columns::id.eq(1))
        .execute(&conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn delete() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://test.fdb").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Luis 2')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'Luis 3')")
        .ok();

    diesel::delete(schema::users::table)
        .filter(schema::users::columns::id.eq(1))
        .execute(&conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn select() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://test.fdb").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Luis A')")
        .ok();
    conn.execute("insert into users (id, name) values (2, 'Luis B')")
        .ok();

    let users = schema::users::table
        .filter(schema::users::columns::id.eq(1))
        .load::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(1, users.len());

    let user = users.iter().next().unwrap();
    assert_eq!(user.id, 1);
    assert_eq!(user.name, "Luis A");

    let user2 = schema::users::table
        .filter(schema::users::columns::id.eq(2))
        .first::<schema::User>(&conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(user2.id, 2);
    assert_eq!(user2.name, "Luis B");

    Ok(())
}
