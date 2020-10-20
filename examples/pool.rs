//!
//! Rust Firebird Client
//!
//! Example of the r2d2 connection pool
//!
//! You need create a database named test.fdb:
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::prelude::*;
use rsfbclient::FirebirdConnectionManager;
use std::{sync::Arc, thread, time::Duration};

fn main() {
    let builder = {
        #[cfg(feature = "linking")]
        let mut builder = rsfbclient::builder_native().with_dyn_link().with_remote();

        #[cfg(feature = "dynamic_loading")]
        let mut builder = rsfbclient::builder_native()
            .with_dyn_load("./fbclient.lib")
            .with_remote();

        #[cfg(feature = "pure_rust")]
        let mut builder = rsfbclient::builder_pure_rust();

        builder
            .host("localhost")
            .db_name("test.fdb")
            .user("SYSDBA")
            .pass("masterkey");

        builder
    };

    //FirebirdConnectionManager is designed to be used without needing a builder
    //however the default builders have all the necessary ingredients to use it.
    //We simply clone the ConnectionConfiguration<...> out from the builder.
    //A From<BuilderType> implementation is provided for both default builders.
    //the output type of into() is inferred automatically from subsequent use of connection_conf
    let connection_conf = (&builder).into();

    let manager = FirebirdConnectionManager::new(builder, connection_conf);
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
