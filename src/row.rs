//!
//! Rust Firebird Client
//!
//! Representation of a fetched row
//!

use bytes::Bytes;
use num_enum::TryFromPrimitive;
use std::{convert::TryInto, mem, result::Result};

use super::{
    ibase,
    status::{err_buffer_len, err_column_null, err_idx_not_exist, err_type_conv, FbError},
};
use ColumnType::*;

/// A database row
pub struct Row {
    pub buffers: Vec<Option<ColumnBuffer>>,
}

impl Row {
    /// Get the column value by the index
    pub fn get<T>(&self, idx: usize) -> Result<T, FbError>
    where
        Option<ColumnBuffer>: ColumnToVal<T>,
    {
        if let Some(col) = self.buffers.get(idx) {
            col.to_val()
        } else {
            err_idx_not_exist()
        }
    }

    /// Get the values for all columns
    pub fn get_all<T>(&self) -> Result<T, FbError>
    where
        T: FromRow,
    {
        T::try_from(&self.buffers)
    }
}

#[derive(Debug, Clone, Copy, TryFromPrimitive)]
#[repr(u32)]
/// Types supported by the crate
pub enum ColumnType {
    /// Coerces to Varchar
    Text = ibase::SQL_VARYING,
    /// Coerces to Int64
    Integer = ibase::SQL_INT64,
    /// Coerces to Double
    Float = ibase::SQL_DOUBLE,
    /// Coerces to Timestamp
    Timestamp = ibase::SQL_TIMESTAMP,
}

#[derive(Debug)]
/// Data returned for a column
pub struct ColumnBuffer {
    /// Type of the data for conversion
    pub(crate) kind: ColumnType,

    /// Buffer for the column data
    pub(crate) buffer: Bytes,
}

/// Define the conversion from the buffer to a value
pub trait ColumnToVal<T> {
    fn to_val(&self) -> Result<T, FbError>
    where
        Self: std::marker::Sized;
}

impl ColumnToVal<String> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<String, FbError> {
        let col = self.as_ref().ok_or_else(|| err_column_null("String"))?;

        match col.kind {
            Text => varchar_to_string(&col.buffer),

            Integer => integer_from_buffer(&col.buffer).map(|i| i.to_string()),

            Float => float_from_buffer(&col.buffer).map(|f| f.to_string()),

            #[cfg(feature = "date_time")]
            Timestamp => {
                crate::date_time::timestamp_from_buffer(&col.buffer).map(|d| d.to_string())
            }

            #[cfg(not(feature = "date_time"))]
            Timestamp => Err(FbError::Other(
                "Enable the `date_time` feature to use Timestamp, Date and Time types".to_string(),
            )),
        }
    }
}

impl ColumnToVal<i64> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<i64, FbError> {
        let col = self.as_ref().ok_or_else(|| err_column_null("i64"))?;

        match col.kind {
            Integer => integer_from_buffer(&col.buffer),

            _ => err_type_conv(col.kind, "i64"),
        }
    }
}

impl ColumnToVal<i32> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<i32, FbError> {
        ColumnToVal::<i64>::to_val(self).map(|i| i as i32)
    }
}

impl ColumnToVal<i16> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<i16, FbError> {
        ColumnToVal::<i64>::to_val(self).map(|i| i as i16)
    }
}

impl ColumnToVal<f64> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<f64, FbError> {
        let col = self.as_ref().ok_or_else(|| err_column_null("f64"))?;

        match col.kind {
            Float => float_from_buffer(&col.buffer),

            _ => err_type_conv(col.kind, "f64"),
        }
    }
}

impl ColumnToVal<f32> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<f32, FbError> {
        ColumnToVal::<f64>::to_val(self).map(|i| i as f32)
    }
}

#[cfg(feature = "date_time")]
impl ColumnToVal<chrono::NaiveDate> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<chrono::NaiveDate, FbError> {
        let col = self.as_ref().ok_or_else(|| err_column_null("NaiveDate"))?;

        match col.kind {
            Timestamp => crate::date_time::timestamp_from_buffer(&col.buffer).map(|ts| ts.date()),

            _ => err_type_conv(col.kind, "NaiveDate"),
        }
    }
}

#[cfg(feature = "date_time")]
impl ColumnToVal<chrono::NaiveTime> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<chrono::NaiveTime, FbError> {
        let col = self.as_ref().ok_or_else(|| err_column_null("NaiveTime"))?;

        match col.kind {
            Timestamp => crate::date_time::timestamp_from_buffer(&col.buffer).map(|ts| ts.time()),

            _ => err_type_conv(col.kind, "NaiveTime"),
        }
    }
}

#[cfg(feature = "date_time")]
impl ColumnToVal<chrono::NaiveDateTime> for Option<ColumnBuffer> {
    fn to_val(&self) -> Result<chrono::NaiveDateTime, FbError> {
        let col = self
            .as_ref()
            .ok_or_else(|| err_column_null("NaiveDateTime"))?;

        match col.kind {
            Timestamp => crate::date_time::timestamp_from_buffer(&col.buffer),

            _ => err_type_conv(col.kind, "NaiveDateTime"),
        }
    }
}

/// Implements for all nullable variants
impl<T> ColumnToVal<Option<T>> for Option<ColumnBuffer>
where
    Option<ColumnBuffer>: ColumnToVal<T>,
{
    fn to_val(&self) -> Result<Option<T>, FbError> {
        if self.is_none() {
            return Ok(None);
        }

        Ok(Some(self.to_val()?))
    }
}

/// Converts a varchar in a buffer to a String
fn varchar_to_string(buffer: &[u8]) -> Result<String, FbError> {
    std::str::from_utf8(&buffer)
        .map(|str| str.to_string())
        .map_err(|_| FbError::Other("Found column with an invalid UTF-8 string".to_owned()))
}

/// Interprets an integer value from a buffer
fn integer_from_buffer(buffer: &[u8]) -> Result<i64, FbError> {
    let len = mem::size_of::<i64>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "i64");
    }

    Ok(i64::from_be_bytes(buffer.try_into().unwrap()))
}

/// Interprets a float value from a buffer
fn float_from_buffer(buffer: &[u8]) -> Result<f64, FbError> {
    let len = mem::size_of::<f64>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "f64");
    }

    Ok(f64::from_be_bytes(buffer.try_into().unwrap()))
}

/// Implemented for types that represents a list of values of columns
pub trait FromRow {
    fn try_from(row: &[Option<ColumnBuffer>]) -> Result<Self, FbError>
    where
        Self: std::marker::Sized;
}

/// For no columns
impl FromRow for () {
    fn try_from(_row: &[Option<ColumnBuffer>]) -> Result<Self, FbError>
    where
        Self: Sized,
    {
        Ok(())
    }
}

/// Generates FromRow implementations for a tuple
macro_rules! impl_from_row {
    ($($t: ident),+) => {
        impl<'a, $($t),+> FromRow for ($($t,)+)
        where
            $( Option<ColumnBuffer>: ColumnToVal<$t>, )+
        {
            fn try_from(row: &[Option<ColumnBuffer>]) -> Result<Self, FbError> {
                let mut iter = row.iter();

                Ok(( $(
                    ColumnToVal::<$t>::to_val(
                        iter
                            .next()
                            .ok_or_else(|| {
                                FbError::Other(
                                    format!("The sql returned less columns than the {} expected", row.len())
                                )
                            })?
                    )?,
                )+ ))
            }
        }
    };
}

/// Generates FromRow implementations for various tuples
macro_rules! impls_from_row {
    ($t: ident) => {
        impl_from_row!($t);
    };

    ($t: ident, $($ts: ident),+ ) => {
        impls_from_row!($($ts),+);

        impl_from_row!($t, $($ts),+);
    };
}

impls_from_row!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

#[cfg(test)]
mod test {
    use crate::{prelude::*, Connection, FbError};
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

    #[test]
    fn dates() -> Result<(), FbError> {
        let mut conn = conn();

        let (a, b, c): (NaiveDate, NaiveDateTime, NaiveTime) = conn
            .query_first(
                "select cast('2010-10-10' as date), cast('2010-10-10 10:10:10' as TIMESTAMP), cast('10:10:10' as TIME) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(NaiveDate::from_ymd(2010, 10, 10), a);
        assert_eq!(NaiveDate::from_ymd(2010, 10, 10).and_hms(10, 10, 10), b);
        assert_eq!(NaiveTime::from_hms(10, 10, 10), c);

        Ok(())
    }

    #[test]
    fn strings() -> Result<(), FbError> {
        let mut conn = conn();

        let (a, b): (String, String) = conn
            .query_first(
                "select cast('firebird' as varchar(8)), cast('firebird' as char(8)) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!("firebird".to_string(), a);
        assert_eq!("firebird".to_string(), b);

        let (a, b): (String, String) = conn
            .query_first(
                "select cast('firebird' as varchar(10)), cast('firebird' as char(10)) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!("firebird".to_string(), a);
        assert_eq!("firebird  ".to_string(), b);

        Ok(())
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::excessive_precision)]
    fn fixed_points() -> Result<(), FbError> {
        let mut conn = conn();

        let (a, b): (f32, f32) = conn
            .query_first(
                "select cast(100 as numeric(3, 2)), cast(100 as decimal(3, 2)) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(100.0, a);
        assert_eq!(100.0, b);

        let (a, b): (f32, f32) = conn
            .query_first(
                "select cast(2358.35321 as numeric(5, 5)), cast(2358.35321 as decimal(5, 5)) from rdb$database",
                ()
            )?
            .unwrap();
        assert_eq!(2358.35321, a);
        assert_eq!(2358.35321, b);

        let (a, b): (f64, f64) = conn
            .query_first(
                "select cast(2358.78353211234 as numeric(11, 11)), cast(2358.78353211234 as decimal(11, 11)) from rdb$database",
                ()
            )?
            .unwrap();
        assert_eq!(2358.78353211234, a);
        assert_eq!(2358.78353211234, b);

        Ok(())
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::excessive_precision)]
    fn float_points() -> Result<(), FbError> {
        let mut conn = conn();

        let (a, b): (f32, f64) = conn
            .query_first(
                "select cast(100 as float), cast(100 as double precision) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(100.0, a);
        assert_eq!(100.0, b);

        let (a, b): (f32, f64) = conn
            .query_first(
                "select cast(2358.35 as float), cast(2358.35 as double precision) from rdb$database",
                ()
            )?
            .unwrap();
        assert_eq!(2358.35, a);
        assert_eq!(2358.35, b);

        // We use fixed values instead of f64::MAX/MIN, because the supported ranges in rust and firebird aren't the same.
        let (min, max): (f64, f64) = conn.query_first("select cast(2.225E-300 as double precision), cast(1.797e300 as double precision) from RDB$DATABASE", ())?
            .unwrap();
        assert_eq!(2.225e-300, min);
        assert_eq!(1.797e300, max);

        // We use fixed values instead of f32::MAX/MIN, because the supported ranges in rust and firebird aren't the same.
        let (min, max): (f32, f32) = conn
            .query_first(
                "select cast(1.175E-38 as float), cast(3.402E38 as float) from RDB$DATABASE",
                (),
            )?
            .unwrap();
        assert_eq!(1.175E-38, min);
        assert_eq!(3.402E38, max);

        Ok(())
    }

    #[test]
    fn ints() -> Result<(), FbError> {
        let mut conn = conn();

        let (a, b, c): (i32, i16, i64) = conn
            .query_first(
                "select cast(100 as int), cast(100 as smallint), cast(100 as bigint) from rdb$database",
                ()
            )?
            .unwrap();
        assert_eq!(100, a);
        assert_eq!(100, b);
        assert_eq!(100, c);

        let (a, b, c): (i32, i16, i64) = conn
            .query_first(
                "select cast(2358 as int), cast(2358 as smallint), cast(2358 as bigint) from rdb$database",
                ()
            )?
            .unwrap();
        assert_eq!(2358, a);
        assert_eq!(2358, b);
        assert_eq!(2358, c);

        let (min, max): (i64, i64) = conn.query_first("select cast(-9223372036854775808 as bigint), cast(9223372036854775807 as bigint) from RDB$DATABASE", ())?
            .unwrap();
        assert_eq!(i64::MIN, min);
        assert_eq!(i64::MAX, max);

        let (min, max): (i32, i32) = conn
            .query_first(
                "select cast(-2147483648 as int), cast(2147483647 as int) from RDB$DATABASE",
                (),
            )?
            .unwrap();
        assert_eq!(i32::MIN, min);
        assert_eq!(i32::MAX, max);

        let (min, max): (i16, i16) = conn
            .query_first(
                "select cast(-32768 as bigint), cast(32767 as bigint) from RDB$DATABASE",
                (),
            )?
            .unwrap();
        assert_eq!(i16::MIN, min);
        assert_eq!(i16::MAX, max);

        Ok(())
    }

    fn conn() -> Connection {
        #[cfg(not(feature = "dynamic_loading"))]
        let conn = crate::ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn = crate::ConnectionBuilder::with_client("./fbclient.lib")
            .expect("Error finding fbclient lib")
            .connect()
            .expect("Error on connect the test database");

        conn
    }
}
