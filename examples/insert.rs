///
/// Rust Firebird Client 
///
/// Example of insert 
///

use rsfbclient::Connection;

const SQL_TABLE: &'static str = "create table test (tcolumn int);";
const SQL_INSERT: &'static str = "insert into test (tcolumn) values (10)";

fn main() {
    
    if let Ok(conn) = Connection::open_local("test.fdb".to_string()) {
        conn.drop()
            .expect("Error on drop the existing database");
    }

    Connection::create_local("test.fdb".to_string())
        .expect("Error on create the new database");

    let conn = Connection::open_local("test.fdb".to_string())
        .expect("Error on connect");

    let tr = conn.start_transaction()
        .expect("Error on start the transaction");

    tr.execute_immediate(SQL_TABLE.to_string())
        .expect("Error on create the table");

    tr.commit()
        .expect("Error on commit the transaction");

    let tr = conn.start_transaction()
        .expect("Error on start the transaction");

    tr.execute_immediate(SQL_INSERT.to_string())
        .expect("Error on insert");

    tr.commit()
        .expect("Error on commit the transaction");

    conn.close()
        .expect("Error on close the connection");
}
