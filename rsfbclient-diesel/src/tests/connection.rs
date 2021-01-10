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

#[test]
fn transaction_depth_multithreaded() -> Result<(), String> {
    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    for i in 0_u8..10 {
        conn.execute(&format!("drop table users{}", i)).ok();
        conn.execute(&format!(
            "create table users{}(id int, name varchar(50))",
            i
        ))
        .ok();
    }

    let jobs: Vec<_> = (0_u8..10)
        .map(|i| {
            std::thread::spawn(move || {
                let conn =
                    FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
                        .map_err(|e| e.to_string())
                        .unwrap();

                let _ = conn
                    .transaction::<(), _, _>(|| {
                        conn.execute(&format!(
                            "insert into users{} (id, name) values (1, 'Teste 1')",
                            i
                        ))
                        .ok();
                        conn.execute(&format!(
                            "insert into users{} (id, name) values (2, 'Teste 2')",
                            i
                        ))
                        .ok();

                        let names = schema::users::table
                            .select(schema::users::columns::name)
                            .load::<String>(&conn)?;
                        assert_eq!(vec!["Teste 1", "Teste 2"], names);

                        let _ = conn.transaction::<(), _, _>(|| {
                            conn.execute(&format!(
                                "insert into users{} (id, name) values (2, 'Teste 3')",
                                i
                            ))
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
                    })
                    .unwrap();
            })
        })
        .collect();

    for j in jobs {
        j.join().unwrap();
    }

    Ok(())
}
