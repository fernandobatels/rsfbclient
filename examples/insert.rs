///
/// Rust Firebird Client 
///
/// Example of insert 
///

use rsfbclient::Connection;

fn main() {

    let mut conn = Connection::open("localhost".to_string(), 3050, "employe2.fdb".to_string(), "SYSDBA".to_string(), "masterkey".to_string())
        .expect("Error on connect");

    let tr = conn.start_transaction()
        .expect("Error on start the transaction");

    println!("??");

    tr.commit()
        .expect("Error on commit the transaction");

    conn.close()
        .expect("Error on close the connection");
}
