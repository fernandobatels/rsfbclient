//! Basic crud tests

use super::schema;
use crate::connection::SimpleConnection;
use crate::fb::FbConnection;
use crate::prelude::*;

#[test]
fn insert() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let user = schema::User {
        id: 10,
        name: "Pedro".to_string(),
    };

    diesel::insert_into(schema::users::table)
        .values(&user)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn insert_alt() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    diesel::insert_into(schema::users::table)
        .values((
            schema::users::columns::id.eq(10),
            schema::users::columns::name.eq("Pedro alt"),
        ))
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn insert_returning() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let user = schema::User {
        id: 10,
        name: "Pedro".to_string(),
    };

    let id: i32 = diesel::insert_into(schema::users::table)
        .values(&user)
        .returning(schema::users::columns::id)
        .get_result(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(id, user.id);

    Ok(())
}

#[test]
fn update() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    conn.batch_execute("insert into users (id, name) values (1, 'Luis')")
        .ok();

    diesel::update(schema::users::table)
        .set(schema::users::columns::name.eq("Fernando"))
        .filter(schema::users::columns::id.eq(1))
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn delete() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    conn.batch_execute("insert into users (id, name) values (1, 'Luis 2')")
        .ok();
    conn.batch_execute("insert into users (id, name) values (2, 'Luis 3')")
        .ok();

    diesel::delete(schema::users::table)
        .filter(schema::users::columns::id.eq(1))
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn select() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    conn.batch_execute("insert into users (id, name) values (1, 'Luis A')")
        .ok();
    conn.batch_execute("insert into users (id, name) values (2, 'Luis B')")
        .ok();

    let users = schema::users::table
        .filter(schema::users::columns::id.eq(1))
        .load::<schema::User>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(1, users.len());

    let user = users.get(0).unwrap();
    assert_eq!(user.id, 1);
    assert_eq!(user.name, "Luis A");

    let user2 = schema::users::table
        .filter(schema::users::columns::id.eq(2))
        .first::<schema::User>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(user2.id, 2);
    assert_eq!(user2.name, "Luis B");

    Ok(())
}
