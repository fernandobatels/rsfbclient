use diesel::prelude::*;
use rsfbclient_diesel::FbConnection;

fn main() {

    let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/employee.fdb")
        .expect("Connection error");
}
