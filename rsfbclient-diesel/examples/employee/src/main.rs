use diesel::prelude::*;
use rsfbclient_diesel::FbConnection;

mod schema;

fn main() {

    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/employee.fdb")
        .expect("Connection error");

    let jobs = schema::job::table
        .load::<schema::Job>(&mut conn)
        .expect("Job select error");

    for job in jobs {
        println!("{} {} {}", job.code, job.title, job.country);
    }
}
