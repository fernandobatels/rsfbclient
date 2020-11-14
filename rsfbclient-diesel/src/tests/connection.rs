//! Connection and transaction tests

use super::schema;
use crate::prelude::*;
use diesel::prelude::*;
use diesel::result::Error;

#[test]
fn transaction() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://test.fdb").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.execute("insert into users (id, name) values (1, 'Teste 1')")
        .ok();

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(&conn)
        .map_err(|e| e.to_string())?;
    assert_eq!(vec!["Teste 1"], names);

    let _ = conn.transaction::<(), _, _>(|| {
        diesel::insert_into(schema::users::table)
            .values(schema::users::columns::name.eq("Teste 2"))
            .execute(&conn)?;

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;

        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Err(Error::RollbackTransaction)
    });

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(&conn)
        .map_err(|e| e.to_string())?;
    assert_eq!(vec!["Teste 1"], names);

    Ok(())
}

#[test]
fn transaction_depth() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://test.fdb").map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    let _ = conn.transaction::<(), _, _>(|| {
        conn.execute("insert into users (id, name) values (1, 'Teste 1')")
            .ok();
        conn.execute("insert into users (id, name) values (2, 'Teste 2')")
            .ok();

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        let _ = conn.transaction::<(), _, _>(|| {
            conn.execute("insert into users (id, name) values (2, 'Teste 3')")
                .ok();

            let names = schema::users::table
                .select(schema::users::columns::name)
                .load::<String>(&conn)?;
            assert_eq!(vec!["Teste 1", "Teste 2", "Teste 3"], names);

            Err(Error::RollbackTransaction)
        });

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Err(Error::RollbackTransaction)
    });

    Ok(())
}
