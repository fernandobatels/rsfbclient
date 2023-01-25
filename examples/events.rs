//!
//! Rust Firebird Client
//!
//! Examples of events traits
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::{FbError, Queryable, RemoteEventsManager};

fn main() -> Result<(), FbError> {
    #[cfg(feature = "linking")]
    let mut conn = rsfbclient::builder_native()
        .with_dyn_link()
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?;

    #[cfg(feature = "dynamic_loading")]
    let mut conn = rsfbclient::builder_native()
        .with_dyn_load("./fbclient.lib")
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?;

    let wt = conn.listen_event("ping".to_string(), move |c| {
        println!("Pong! Some rows here:");

        let rows: Vec<(String, String)> = c.query(
            "select mon$attachment_name, mon$user from mon$attachments",
            (),
        )?;

        for row in rows {
            println!("Attachment {}, user {}", row.0, row.1);
        }

        return Ok(true);
    })?;

    println!("Try ping here with \"POST_EVENT 'ping'\"");

    wt.join().unwrap()?;

    Ok(())
}
