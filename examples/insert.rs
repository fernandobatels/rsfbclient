///
/// Rust Firebird Client 
///
/// Example of insert 
///

use rsfbclient::connection;

fn main() {

    let mut conn = connection::open("localhost".to_string(), 3050, "employe2.fdb".to_string(), "SYSDBA".to_string(), "masterkey".to_string())
        .expect("Error on connect");

    println!("??");

    conn.close()
        .expect("Error on close the connection");
}
