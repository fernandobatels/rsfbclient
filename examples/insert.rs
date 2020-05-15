///
/// Rust Firebird Client 
///
/// Example of insert 
///

use rsfbclient::connection;

fn main() {

    connection::open("localhost".to_string(), 3050, "employe2.fdb".to_string(), "SYSDBA".to_string(), "masterkey".to_string())
        .expect("Error on connect");

    println!("??");
}
