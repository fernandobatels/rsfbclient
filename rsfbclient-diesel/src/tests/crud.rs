//! Basic crud tests

use crate::prelude::*;
use diesel::prelude::*;
use super::schema;

#[test]
fn insert() -> Result<(), String> {
    let conn = FbConnection::establish("teste").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    let user = schema::User {
        id: 10,
        name: "Pedro",
    };

    diesel::insert_into(schema::users::table)
        .values(&user)
        .execute(&conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn update() -> Result<(), String> {
    let conn = FbConnection::establish("teste").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Luis')").ok();

    diesel::update(schema::users::table)
        .set(schema::users::columns::name.eq("Fernando"))
        .filter(schema::users::columns::id.eq(1))
        .execute(&conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[test]
fn delete() -> Result<(), String> {
    let conn = FbConnection::establish("teste").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Luis 2')").ok();
    conn.execute("insert into users (id, name) values (2, 'Luis 3')").ok();

    diesel::delete(schema::users::table)
        .filter(schema::users::columns::id.eq(1))
        .execute(&conn)
        .map_err(|e| e.to_string())?;

    Ok(())
}
