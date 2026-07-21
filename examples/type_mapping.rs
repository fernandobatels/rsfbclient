//!
//! Rust Firebird Client
//!
//! Example of how Firebird data types map to Rust types
//!
//! The driver's `SqlType` is deliberately coarse: every integer arrives
//! as i64, every float / NUMERIC / DECIMAL as f64, CHAR/VARCHAR as
//! String, BLOB as Vec<u8> or String, TIMESTAMP as chrono types and
//! BOOLEAN as bool. This example shows each mapping live, plus the two
//! sharp edges worth knowing:
//!
//!  - NUMERIC/DECIMAL are converted through f64, so scaled values whose
//!    integer form exceeds 2^53 silently lose precision (shown below
//!    with a value that comes back one cent off).
//!  - Firebird 4+ types (INT128, DECFLOAT, TIMESTAMP/TIME WITH TIME
//!    ZONE) and blobs with sub_type > 1 are not supported by the row
//!    reader: selecting such a column fails at describe time with
//!    "Unsupported column type". CAST them to VARCHAR (or BIGINT etc.)
//!    in SQL to move the conversion server-side.
//!
//! The table is created by the example itself (idempotent); you only
//! need an `examples.fdb` database. The FB4-specific columns are added
//! in a second DDL step that is allowed to fail, so the example also
//! runs against Firebird 3.
//!

#![allow(unused_variables, unused_mut)]

use rsfbclient::{prelude::*, FbError, SimpleConnection};

fn connect() -> Result<SimpleConnection, FbError> {
    #[cfg(feature = "linking")]
    let conn = rsfbclient::builder_native()
        .with_dyn_link()
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?
        .into();

    #[cfg(feature = "dynamic_loading")]
    let conn = rsfbclient::builder_native()
        .with_dyn_load("./fbclient.lib")
        .with_remote()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?
        .into();

    #[cfg(feature = "pure_rust")]
    let conn = rsfbclient::builder_pure_rust()
        .host("localhost")
        .db_name("examples.fdb")
        .user("SYSDBA")
        .pass("masterkey")
        .connect()?
        .into();

    Ok(conn)
}

fn main() -> Result<(), FbError> {
    let mut conn = connect()?;

    let _ = conn.execute("drop table type_probe", ());
    conn.execute(
        "create table type_probe (
            c_small  smallint,
            c_int    integer,
            c_big    bigint,
            c_num    numeric(18,2),
            c_double double precision,
            c_vc     varchar(20),
            c_bool   boolean,
            c_ts     timestamp
         )",
        (),
    )?;
    // Firebird 4+ types, added separately so the example runs on FB3 too
    let fb4 = conn
        .execute(
            "alter table type_probe add c_i128 int128, add c_dec decfloat(34)",
            (),
        )
        .is_ok();
    conn.commit()?;

    conn.execute(
        "insert into type_probe (c_small, c_int, c_big, c_num, c_double, c_vc, c_bool, c_ts)
         values (1, 2, 3, 90071992547409.93, 0.1, 'hello', true, current_timestamp)",
        (),
    )?;
    if fb4 {
        conn.execute(
            "update type_probe set c_i128 = 170141183460469231731687303715884105727,
                                   c_dec  = 1.234567890123456789012345678901234E+10",
            (),
        )?;
    }
    conn.commit()?;

    // 1. The supported mappings, fetched typed
    let (small, int, big, num, double, vc, boolean): (i64, i64, i64, f64, f64, String, bool) = conn
        .query_first(
            "select c_small, c_int, c_big, c_num, c_double, c_vc, c_bool from type_probe",
            (),
        )?
        .unwrap();
    println!("smallint  -> i64    : {}", small);
    println!("integer   -> i64    : {}", int);
    println!("bigint    -> i64    : {}", big);
    println!(
        "numeric   -> f64    : {}   <- one cent off: the scaled integer",
        num
    );
    println!("                              9007199254740993 exceeds 2^53");
    println!("double    -> f64    : {}", double);
    println!("varchar   -> String : {}", vc);
    println!("boolean   -> bool   : {}", boolean);

    // The server's own text rendering keeps the cent
    let (num_text,): (String,) = conn
        .query_first("select cast(c_num as varchar(30)) from type_probe", ())?
        .unwrap();
    println!("numeric via CAST    : {}   <- exact", num_text);

    // 2. The unsupported types fail at describe time...
    if fb4 {
        match conn.query_first::<(), (String,)>("select c_i128 from type_probe", ()) {
            Ok(_) => println!("unexpected: INT128 fetched"),
            Err(e) => println!("select c_i128 fails  : {}", e),
        }
        // ...and CAST moves the conversion server-side
        let (i128_text, dec_text): (String, String) = conn
            .query_first(
                "select cast(c_i128 as varchar(50)), cast(c_dec as varchar(50)) from type_probe",
                (),
            )?
            .unwrap();
        println!("int128 via CAST     : {}", i128_text);
        println!("decfloat via CAST   : {}", dec_text);
    } else {
        println!("(Firebird 3 server: INT128/DECFLOAT part skipped)");
    }

    conn.commit()?;
    Ok(())
}
