use diesel::{QueryDsl, RunQueryDsl};
use r2d2::Pool;
use r2d2_firebird::DieselConnectionManager;
use std::{env, sync::Arc, thread, time::Duration};

use schema::{job::dsl::*, Job};

mod schema;

fn main() {

    let connecton_string = env::var("DIESELFDB_CONN").expect("DIESELFDB_CONN env not found");

    let manager = DieselConnectionManager::new(&connecton_string);
    let pool = Arc::new(
        Pool::builder()
            .max_size(4)
            .build(manager)
            .expect("Failed to build connection pooling."),
    );

    let mut tasks = vec![];

    for n in 0..3 {
        let pool = pool.clone();
        let th = thread::spawn(move || loop {

            match pool.get() {
                Ok(mut conn) => match job.offset(n).limit(1).first::<Job>(&mut *conn) {
                    Ok(model) => {
                        println!("Thread {}: {}", n, model.title)
                    }

                    Err(e) => println!("Execute query error in line:{} ! error: {:?}", line!(), e),
                },
                Err(e) => println!(
                    "Get connection from pool error in line:{} ! error: {:?}",
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
