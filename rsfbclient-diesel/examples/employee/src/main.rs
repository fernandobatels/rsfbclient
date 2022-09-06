use diesel::prelude::*;
use rsfbclient_diesel::FbConnection;
use argopt::{subcmd, cmd_group};
use tabled::Table;
use std::env;

mod schema;

#[cmd_group(commands = [list, add, update, remove])]
fn main() {
}

/// List all avaliable jobs
#[subcmd]
fn list() {
    let str_conn = env::var("DIESELFDB_CONN")
        .expect("DIESELFDB_CONN env not found");
    let mut conn = FbConnection::establish(&str_conn)
        .expect("Connection error");

    let jobs = schema::job::table
        .load::<schema::Job>(&mut conn)
        .expect("Job select error");

    println!("{}", Table::new(jobs));
}

/// Create a new job
#[subcmd]
fn add(
    /// Job identification
    #[opt(long)]
    code: String,
    /// Job title
    #[opt(long)]
    title: String,
    /// Job grade
    #[opt(long)]
    grade: i16,
    /// Job country destination
    #[opt(long)]
    country: String,
    /// Min salary
    #[opt(long)]
    min_salary: f32,
    /// Max salary
    #[opt(long)]
    max_salary: f32
) {
    let str_conn = env::var("DIESELFDB_CONN")
        .expect("DIESELFDB_CONN env not found");
    let mut conn = FbConnection::establish(&str_conn)
        .expect("Connection error");

    let new_job = schema::Job {
        code,
        title,
        grade,
        country,
        min_salary,
        max_salary
    };

    diesel::insert_into(schema::job::table)
        .values(new_job)
        .execute(&mut conn)
        .expect("Job insert error");
}

/// Update a job by code
#[subcmd]
fn update(
    /// Job code
    code: String,
    /// New title
    #[opt(long)]
    title: Option<String>,
    /// New min salary
    #[opt(long)]
    min_salary: Option<f32>,
    /// New max salary
    #[opt(long)]
    max_salary: Option<f32>
) {
    let str_conn = env::var("DIESELFDB_CONN")
        .expect("DIESELFDB_CONN env not found");
    let mut conn = FbConnection::establish(&str_conn)
        .expect("Connection error");

    let update_job = schema::JobUpdate {
        title,
        min_salary,
        max_salary
    };

    diesel::update(schema::job::table)
        .filter(schema::job::columns::job_code.eq(code))
        .set(&update_job)
        .execute(&mut conn)
        .expect("Job update error");
}

/// Remove a job by code
#[subcmd]
fn remove(
    /// Job code
    code: String,
) {
    let str_conn = env::var("DIESELFDB_CONN")
        .expect("DIESELFDB_CONN env not found");
    let mut conn = FbConnection::establish(&str_conn)
        .expect("Connection error");

    diesel::delete(schema::job::table)
        .filter(schema::job::columns::job_code.eq(code))
        .execute(&mut conn)
        .expect("Job delete error");
}
