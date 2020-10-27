
use crate::prelude::*;
use diesel::prelude::*;

#[derive(Insertable)]
#[table_name = "users"]
pub struct User<'a> {
    pub id: i32,
    pub name: &'a str,
}

table! {
    users(id) {
        id -> Int4,
        name -> Varchar,
    }
}

pub fn setup(conn: &FbConnection) -> Result<(), String> {

    conn.execute("drop table users").ok();
    conn.execute("create table users(id int, name varchar(50))")
        .ok();

    Ok(())
}
