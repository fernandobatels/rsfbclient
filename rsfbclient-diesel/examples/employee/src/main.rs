use diesel::prelude::*;
use rsfbclient_diesel::FbConnection;
use argopt::{subcmd, cmd_group};
use tabled::Table;

mod schema;

#[cmd_group(commands = [list])]
fn main() {
}

/// List all avaliable jobs
#[subcmd]
fn list() {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/employee.fdb")
        .expect("Connection error");

    let jobs = schema::job::table
        .load::<schema::Job>(&mut conn)
        .expect("Job select error");

    println!("{}", Table::new(jobs));
}
