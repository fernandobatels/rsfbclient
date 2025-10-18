use diesel::{QueryDsl, RunQueryDsl};
use r2d2::Pool;
use std::env;
use std::{sync::Arc, thread, time::Duration};

use pool::FirebirdConnectionManager;
use schema::{job::dsl::*, Job};

mod pool;
mod schema;

fn main() {
    let connection_string = env::var("DIESELFDB_CONN").expect("DIESELFDB_CONN env not found.");

    let manager = FirebirdConnectionManager::new(&connection_string);
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
