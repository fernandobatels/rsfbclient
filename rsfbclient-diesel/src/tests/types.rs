//! Params tests

use super::schema;
use crate::fb::FbConnection;
use crate::prelude::*;
use rsfbclient::{EngineVersion, SystemInfos};
use std::str;

#[test]
#[allow(clippy::float_cmp)]
fn types1() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let types1 = schema::Types1 {
        id: 1,
        a: "ab çç dd".to_string(),
        b: 88,
        c: 3.402E38,
        d: "aa".to_string(),
    };

    diesel::insert_into(schema::types1::table)
        .values(&types1)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let types1 = schema::types1::table
        .first::<schema::Types1>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(types1.a, "ab çç dd".to_string());
    assert_eq!(types1.b, 88);
    assert_eq!(types1.c, 3.402E38);
    assert_eq!(types1.d, "aa".to_string());

    Ok(())
}

#[test]
fn null() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let types1 = schema::Types1Null {
        id: 2,
        a: Some("ab çç dd".to_string()),
        b: Some(88),
        c: Some(3.402E38),
        d: Some("aa".to_string()),
    };

    diesel::insert_into(schema::types1null::table)
        .values(&types1)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let types1 = schema::types1null::table
        .first::<schema::Types1Null>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(types1.a, Some("ab çç dd".to_string()));
    assert_eq!(types1.b, Some(88));
    assert_eq!(types1.c, Some(3.402E38));
    assert_eq!(types1.d, Some("aa".to_string()));

    let types1 = schema::Types1Null {
        id: 3,
        a: None,
        b: None,
        c: None,
        d: None,
    };

    diesel::insert_into(schema::types1null::table)
        .values(&types1)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let types1 = schema::types1null::table
        .order(schema::types1null::columns::id.desc())
        .first::<schema::Types1Null>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(types1.a, None);
    assert_eq!(types1.b, None);
    assert_eq!(types1.c, None);
    assert_eq!(types1.d, None);

    Ok(())
}

#[test]
fn types2() -> Result<(), String> {
    use chrono::*;

    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let types2 = schema::Types2 {
        id: 2,
        a: NaiveDate::from_ymd(2020, 12, 15),
        b: NaiveTime::from_hms(10, 10, 10),
        c: NaiveDate::from_ymd(2020, 12, 15).and_hms(12, 12, 12),
    };

    diesel::insert_into(schema::types2::table)
        .values(&types2)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let types2 = schema::types2::table
        .first::<schema::Types2>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(types2.a, NaiveDate::from_ymd(2020, 12, 15));
    assert_eq!(types2.b, NaiveTime::from_hms(10, 10, 10));
    assert_eq!(
        types2.c,
        NaiveDate::from_ymd(2020, 12, 15).and_hms(12, 12, 12)
    );

    Ok(())
}

#[test]
#[allow(clippy::bool_assert_comparison)]
fn boolean() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    let se = conn.raw
        .server_engine()
        .map_err(|e| e.to_string())?;
    if se <= EngineVersion::V2 {
        return Ok(());
    }

    schema::setup(&mut conn)?;

    let bool_type = schema::BoolType {
        id: 2,
        a: true,
        b: false,
        c: None,
    };

    diesel::insert_into(schema::bool_type::table)
        .values(&bool_type)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let bool_type = schema::bool_type::table
        .first::<schema::BoolType>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(bool_type.a, true);
    assert_eq!(bool_type.b, false);
    assert_eq!(bool_type.c, None);

    Ok(())
}

#[test]
fn blob() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let text_test = "ab çç dd 123".to_string();
    let blob_test = text_test.as_bytes().to_vec();

    let types1 = schema::BlobType {
        id: 2,
        a: blob_test.clone(),
        b: Some(blob_test.clone()),
    };

    diesel::insert_into(schema::blob_type::table)
        .values(&types1)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let types1 = schema::blob_type::table
        .first::<schema::BlobType>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(types1.a, blob_test);
    assert_eq!(types1.b, Some(blob_test));
    assert_eq!(
        str::from_utf8(&types1.a).expect("Invalid UTF-8 sequence"),
        text_test
    );

    Ok(())
}

#[test]
#[allow(clippy::float_cmp)]
fn types3() -> Result<(), String> {
    let mut conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb")
        .map_err(|e| e.to_string())?;

    schema::setup(&mut conn)?;

    let types3 = schema::Types3 {
        id: 1,
        a: i16::MAX,
        b: i64::MAX,
        c: 3.402E38,
        d: f64::MAX,
    };

    diesel::insert_into(schema::types3::table)
        .values(&types3)
        .execute(&mut conn)
        .map_err(|e| e.to_string())?;

    let types3 = schema::types3::table
        .first::<schema::Types3>(&mut conn)
        .map_err(|e| e.to_string())?;

    assert_eq!(types3.a, i16::MAX);
    assert_eq!(types3.b, i64::MAX);
    assert_eq!(types3.c, 3.402E38);
    assert_eq!(types3.d, f64::MAX);

    Ok(())
}
