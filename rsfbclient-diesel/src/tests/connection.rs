//! Connection and transaction tests

use super::schema;
use crate::fb::FbConnection;
use crate::prelude::*;
use crate::result::Error;

#[test]
fn transaction() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

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

    conn.test_transaction::<(), Error, _>(|| {
        diesel::insert_into(schema::users::table)
            .values(schema::users::columns::name.eq("Teste 2"))
            .execute(&conn)?;

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;

        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Ok(())
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
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    conn.test_transaction::<(), Error, _>(|| {
        conn.execute("insert into users (id, name) values (1, 'Teste 1')")
            .unwrap();
        conn.execute("insert into users (id, name) values (2, 'Teste 2')")
            .unwrap();

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        conn.transaction::<(), Error, _>(|| {
            conn.execute("insert into users (id, name) values (3, 'Teste 3')")
                .unwrap();

            let names = schema::users::table
                .select(schema::users::columns::name)
                .load::<String>(&conn)?;
            assert_eq!(vec!["Teste 1", "Teste 2", "Teste 3"], names);

            Err(Error::RollbackTransaction)
        })
        .ok();

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2"], names);

        Ok(())
    });

    Ok(())
}

#[test]
fn transaction_depth_moved_connection() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&conn)?;

    let mut opt_conn = Some(conn);
    let mut opt_conn2 = None;

    opt_conn.as_ref().unwrap().begin_test_transaction().unwrap();

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(opt_conn.as_ref().unwrap())
        .unwrap();
    assert_eq!(Vec::<&str>::new(), names);

    std::mem::swap(&mut opt_conn, &mut opt_conn2);
    drop(opt_conn);

    let conn = opt_conn2.take().unwrap();

    conn.execute("insert into users (id, name) values (1, 'Teste 1')")
        .unwrap();
    conn.execute("insert into users (id, name) values (2, 'Teste 2')")
        .unwrap();

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(&conn)
        .unwrap();
    assert_eq!(vec!["Teste 1", "Teste 2"], names);

    conn.transaction::<(), Error, _>(|| {
        conn.execute("insert into users (id, name) values (3, 'Teste 3')")
            .unwrap();

        let names = schema::users::table
            .select(schema::users::columns::name)
            .load::<String>(&conn)?;
        assert_eq!(vec!["Teste 1", "Teste 2", "Teste 3"], names);

        Err(Error::RollbackTransaction)
    })
    .ok();

    let names = schema::users::table
        .select(schema::users::columns::name)
        .load::<String>(&conn)
        .unwrap();
    assert_eq!(vec!["Teste 1", "Teste 2"], names);

    Ok(())
}
