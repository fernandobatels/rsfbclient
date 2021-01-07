//! Types implementation of Firebird support

use super::backend::Fb;
use super::value::FbValue;
use bytes::Buf;
use bytes::Bytes;
#[cfg(feature = "date_time")]
use chrono::*;
use diesel::deserialize::{self, FromSql};
use diesel::result::Error::DatabaseError;
use diesel::result::*;
use diesel::serialize::{self, IsNull, ToSql};
use diesel::sql_types::{self, HasSqlType};
use rsfbclient::{ColumnToVal, IntoParam, SqlType};
use std::boxed::Box;
use std::error::Error;
use std::io::Write;

/// Supported types by the diesel
/// Firebird implementation
pub enum SupportedType {
    Text,
    SmallInt,
    Int,
    BigInt,
    Float,
    Double,
    Date,
    Time,
    DateTime,
    Bool,
    Blob,
}

impl SupportedType {
    pub fn to_param(self, source_val: Option<Vec<u8>>) -> SqlType {
        if let Some(val) = source_val {
            #[allow(unreachable_patterns)]
            match self {
                SupportedType::Text => String::from_utf8(val).expect("Invalid UTF-8").into_param(),
                SupportedType::SmallInt => Bytes::copy_from_slice(&val).get_i16().into_param(),
                SupportedType::Int => Bytes::copy_from_slice(&val).get_i32().into_param(),
                SupportedType::BigInt => Bytes::copy_from_slice(&val).get_i64().into_param(),
                SupportedType::Float => Bytes::copy_from_slice(&val).get_f32().into_param(),
                SupportedType::Double => Bytes::copy_from_slice(&val).get_f64().into_param(),
                #[cfg(feature = "date_time")]
                SupportedType::Date => {
                    let days = Bytes::copy_from_slice(&val).get_i32();
                    NaiveDate::from_num_days_from_ce(days).into_param()
                }
                #[cfg(feature = "date_time")]
                SupportedType::Time => {
                    let secs = Bytes::copy_from_slice(&val).get_u32();
                    NaiveTime::from_num_seconds_from_midnight(secs, 0).into_param()
                }
                #[cfg(feature = "date_time")]
                SupportedType::DateTime => {
                    let tms = Bytes::copy_from_slice(&val).get_i64();
                    NaiveDateTime::from_timestamp(tms, 0).into_param()
                }
                SupportedType::Bool => {
                    let bo = Bytes::copy_from_slice(&val).get_i8() == 1;
                    bo.into_param()
                }
                SupportedType::Blob => val.into_param(),
                _ => SqlType::Null,
            }
        } else {
            SqlType::Null
        }
    }
}

impl HasSqlType<sql_types::SmallInt> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::SmallInt
    }
}

impl HasSqlType<sql_types::Integer> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Int
    }
}

impl HasSqlType<sql_types::BigInt> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::BigInt
    }
}

impl HasSqlType<sql_types::Float> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Float
    }
}

impl HasSqlType<sql_types::Double> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Double
    }
}

impl HasSqlType<sql_types::VarChar> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Text
    }
}

impl HasSqlType<sql_types::Binary> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Blob
    }
}

impl HasSqlType<sql_types::Date> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Date
    }
}

impl HasSqlType<sql_types::Time> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Time
    }
}

impl HasSqlType<sql_types::Timestamp> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::DateTime
    }
}

impl HasSqlType<sql_types::Bool> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Bool
    }
}

impl FromSql<sql_types::Integer, Fb> for i32 {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

impl FromSql<sql_types::VarChar, Fb> for String {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

impl FromSql<sql_types::Float, Fb> for f32 {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

#[cfg(feature = "date_time")]
impl FromSql<sql_types::Date, Fb> for NaiveDate {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

#[cfg(feature = "date_time")]
impl ToSql<sql_types::Date, Fb> for NaiveDate {
    fn to_sql<W: Write>(&self, out: &mut serialize::Output<W, Fb>) -> serialize::Result {
        let days = self.num_days_from_ce().to_be_bytes();
        out.write_all(&days)
            .map(|_| IsNull::No)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

#[cfg(feature = "date_time")]
impl FromSql<sql_types::Timestamp, Fb> for NaiveDateTime {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

#[cfg(feature = "date_time")]
impl ToSql<sql_types::Timestamp, Fb> for NaiveDateTime {
    fn to_sql<W: Write>(&self, out: &mut serialize::Output<W, Fb>) -> serialize::Result {
        let tms = self.timestamp().to_be_bytes();
        out.write_all(&tms)
            .map(|_| IsNull::No)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

#[cfg(feature = "date_time")]
impl FromSql<sql_types::Time, Fb> for NaiveTime {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

#[cfg(feature = "date_time")]
impl ToSql<sql_types::Time, Fb> for NaiveTime {
    fn to_sql<W: Write>(&self, out: &mut serialize::Output<W, Fb>) -> serialize::Result {
        let secs = self.num_seconds_from_midnight().to_be_bytes();
        out.write_all(&secs)
            .map(|_| IsNull::No)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

impl FromSql<sql_types::Bool, Fb> for bool {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}

impl ToSql<sql_types::Bool, Fb> for bool {
    fn to_sql<W: Write>(&self, out: &mut serialize::Output<W, Fb>) -> serialize::Result {
        let bo = (*self as i8).to_be_bytes();
        out.write_all(&bo)
            .map(|_| IsNull::No)
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

impl FromSql<sql_types::Binary, Fb> for Vec<u8> {
    fn from_sql(value: FbValue) -> deserialize::Result<Self> {
        let rs = value
            .raw
            .to_val()
            .map_err(|e| DatabaseError(DatabaseErrorKind::__Unknown, Box::new(e.to_string())))?;

        Ok(rs)
    }
}
