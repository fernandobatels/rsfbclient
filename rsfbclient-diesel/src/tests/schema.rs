use crate::FbConnection;
use chrono::*;
use diesel::connection::SimpleConnection;
use diesel::*;
use rsfbclient::{EngineVersion, SystemInfos};

#[derive(Insertable, Queryable, QueryableByName)]
#[diesel(table_name = users)]
pub struct User {
    pub id: i32,
    pub name: String,
}

table! {
    users(id) {
        id -> Int4,
        name -> Varchar,
    }
}

#[derive(Insertable, Queryable)]
#[diesel(table_name = types1)]
pub struct Types1 {
    pub id: i32,
    pub a: String,
    pub b: i32,
    pub c: f32,
    pub d: String,
}

table! {
    types1(id) {
        id -> Int4,
        a -> Text,
        b -> Integer,
        c -> Float,
        d -> Text,
    }
}

#[derive(Insertable, Queryable)]
#[diesel(table_name = types1null)]
pub struct Types1Null {
    pub id: i32,
    pub a: Option<String>,
    pub b: Option<i32>,
    pub c: Option<f32>,
    pub d: Option<String>,
}

table! {
    types1null(id) {
        id -> Int4,
        a -> Nullable<Text>,
        b -> Nullable<Integer>,
        c -> Nullable<Float>,
        d -> Nullable<Text>,
    }
}

#[derive(Insertable, Queryable)]
#[diesel(table_name = types2)]
pub struct Types2 {
    pub id: i32,
    pub a: NaiveDate,
    pub b: NaiveTime,
    pub c: NaiveDateTime,
}
table! {
    types2(id) {
        id -> Int4,
        a -> Date,
        b -> Time,
        c -> Timestamp,
    }
}

#[derive(Insertable, Queryable)]
#[diesel(table_name = bool_type)]
pub struct BoolType {
    pub id: i32,
    pub a: bool,
    pub b: bool,
    pub c: Option<bool>,
}

table! {
    bool_type(id) {
        id -> Int4,
        a -> Bool,
        b -> Bool,
        c -> Nullable<Bool>,
    }
}

#[derive(Insertable, Queryable)]
#[diesel(table_name = blob_type)]
pub struct BlobType {
    pub id: i32,
    pub a: Vec<u8>,
    pub b: Option<Vec<u8>>,
}

table! {
    blob_type(id) {
        id -> Int4,
        a -> Binary,
        b -> Nullable<Binary>,
    }
}

#[derive(Insertable, Queryable)]
#[diesel(table_name = types3)]
pub struct Types3 {
    pub id: i32,
    pub a: i16,
    pub b: i64,
    pub c: f32,
    pub d: f64,
}

table! {
    types3(id) {
        id -> Int4,
        a -> SmallInt,
        b -> BigInt,
        c -> Float,
        d -> Double,
    }
}

pub fn setup(conn: &mut FbConnection) -> Result<(), String> {
    conn.batch_execute("drop table users").ok();
    conn.batch_execute("create table users(id int, name varchar(50))")
        .ok();

    conn.batch_execute("drop table types1").ok();
    conn.batch_execute("create table types1(id int, a varchar(50), b int, c float, d char(2))")
        .ok();

    conn.batch_execute("drop table types1null").ok();
    conn.batch_execute("create table types1null(id int, a varchar(50), b int, c float, d char(2))")
        .ok();

    conn.batch_execute("drop table types2").ok();
    conn.batch_execute("create table types2(id int, a date, b time, c timestamp)")
        .ok();

    conn.batch_execute("drop table types3").ok();
    conn.batch_execute(
        "create table types3(id int, a smallint, b bigint, c float, d double precision)",
    )
    .ok();

    let se = conn.raw.server_engine().map_err(|e| e.to_string())?;
    if se >= EngineVersion::V3 {
        conn.batch_execute("drop table bool_type").ok();
        conn.batch_execute("create table bool_type(id int, a boolean, b boolean, c boolean)")
            .ok();
    }

    conn.batch_execute("drop table blob_type").ok();
    conn.batch_execute("create table blob_type(id int, a blob sub_type 0, b blob sub_type 0)")
        .ok();

    Ok(())
}
