use diesel::{QueryDsl, RunQueryDsl};
use r2d2::Pool;
use std::{sync::Arc, thread, time::Duration};

use pool::FirebirdConnectionManager;
use schema::{job::dsl::*, Job};

mod pool;
mod schema;

fn main() {
    let path = std::env::current_dir().unwrap();
    let connection_string = format!(
        "firebird://SYSDBA:masterkey@localhost:3050/{}", path.join("employee.fdb").display());

    let manager = FirebirdConnectionManager::new(&connection_string);
    let pool = Arc::new(Pool::builder().max_size(4).build(manager).unwrap());

    let mut tasks = vec![];

    for n in 0..3 {
        let pool = pool.clone();

        let th = thread::spawn(move || loop {
            match pool.get() {
                Ok(mut conn) => match job.offset(n).limit(1).first::<Job>(&mut *conn) {
                    Ok(model) => {
                        println!("Thread {}: {}", n, model.title)
                    }

                    Err(e) => println!("execute query error in line:{} ! error: {:?}", line!(), e),
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
