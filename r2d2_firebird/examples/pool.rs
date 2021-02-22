//!
//! Rust Firebird Client
//!
//! Example of the r2d2 connection pool
//!
//! You need create a database named test.fdb:
//!

#![allow(unused_variables, unused_mut)]

use r2d2_firebird::FirebirdConnectionManager;
use rsfbclient::prelude::*;
use std::{sync::Arc, thread, time::Duration};

fn main() {
    let builder = {
        let mut builder = rsfbclient::builder_pure_rust();

        builder
            .host("localhost")
            .db_name("test.fdb")
            .user("SYSDBA")
            .pass("masterkey");

        builder
    };

    //FirebirdConnectionManager makes use of FirebirdClientFactory, which is implemented
    //by builders

    let manager = FirebirdConnectionManager::new(builder);
    let pool = Arc::new(r2d2::Pool::builder().max_size(4).build(manager).unwrap());

    let mut tasks = vec![];

    for n in 0..3 {
        let pool = pool.clone();

        let th = thread::spawn(move || loop {
            match pool.get() {
                Ok(mut conn) => match conn.query_first("SELECT rand() FROM RDB$DATABASE", ()) {
                    Ok(Some(row)) => {
                        let (res,): (f64,) = row;
                        println!("Thread {}: {}", n, res)
                    }

                    Err(e) => println!("execute query error in line:{} ! error: {:?}", line!(), e),

                    _ => panic!("Select returned nothing!"),
                },
                Err(e) => println!(
                    "get connection from pool error in line:{} ! error: {:?}",
                    line!(),
                    e
                ),
            }

            thread::sleep(Duration::from_secs(1));
        });
        tasks.push(th);
    }

    for th in tasks {
        let _ = th.join();
    }
}
