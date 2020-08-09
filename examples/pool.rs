//!
//! Rust Firebird Client
//!
//! Example of the r2d2 connection pool
//!
//! You need create a database named test.fdb:
//!

use rsfbclient::{prelude::*, ConnectionBuilder, FirebirdConnectionManager};
use std::{sync::Arc, thread, time::Duration};

fn main() {
    let builder = {
        #[cfg(not(feature = "dynamic_loading"))]
        let mut builder = ConnectionBuilder::default();

        #[cfg(feature = "dynamic_loading")]
        let mut builder = ConnectionBuilder::with_client("./fbclient.lib");

        builder
            .host("localhost")
            .db_name("test.fdb")
            .user("SYSDBA")
            .pass("masterkey");

        builder
    };

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
