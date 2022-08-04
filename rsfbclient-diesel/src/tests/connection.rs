//! Connection and transaction tests

use super::schema;
use crate::FbConnection;
use diesel::connection::SimpleConnection;
use diesel::result::Error;
use diesel::*;

#[test]
fn transaction() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    conn.batch_execute("insert into users (id, name) values (1, 'Teste 1')")
        .ok();

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(&mut conn)
        .map_err(|e| e.to_string())?;
    assert_eq!(vec!["Teste 1"], names);

    let _ = conn.transaction::<(), _, _>(|conn| {
        diesel::insert_into(schema::users::table)
            .values(schema::users::columns::name.eq("Teste 2"))
            .execute(conn)?;

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(conn)?;

        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Err(Error::RollbackTransaction)
    });

    conn.test_transaction::<(), Error, _>(|conn| {
        diesel::insert_into(schema::users::table)
            .values(schema::users::columns::name.eq("Teste 2"))
            .execute(conn)?;

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(conn)?;

        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Ok(())
    });

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(&mut conn)
        .map_err(|e| e.to_string())?;
    assert_eq!(vec!["Teste 1"], names);

    Ok(())
}

#[test]
fn transaction_depth() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    conn.test_transaction::<(), Error, _>(|conn| {
        conn.batch_execute("insert into users (id, name) values (1, 'Teste 1')")
            .unwrap();
        conn.batch_execute("insert into users (id, name) values (2, 'Teste 2')")
            .unwrap();

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        conn.transaction::<(), Error, _>(|conn| {
            conn.batch_execute("insert into users (id, name) values (3, 'Teste 3')")
                .unwrap();

            let names = schema::users::table
                .select(schema::users::columns::name)
                .load::<String>(conn)?;
            assert_eq!(vec!["Teste 1", "Teste 2", "Teste 3"], names);

            Err(Error::RollbackTransaction)
        })
        .ok();

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Ok(())
    });

    Ok(())
}
