//!
//! Rust Firebird Client
//!
//! Representation of a fetched row
//!

use std::{convert::TryInto, mem, result::Result};

use super::status::{err_buffer_len, err_column_null, err_idx_not_exist, err_type_conv};
use rsfbclient_core::FbError;
use SqlType::*;

/// A database row
pub struct Row<'a> {
    pub buffers: &'a Vec<ColumnBuffer>,
}

impl<'a> Row<'a> {
    /// Get the column value by the index
    pub fn get<T>(&self, idx: usize) -> Result<T, FbError>
    where
        ColumnBuffer: ColumnToVal<T>,
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

#[derive(Debug, Clone, Copy)]
/// Types supported by the crate
pub enum SqlType {
    /// Coerces to Varchar
    Text,
    /// Coerces to Int64
    Integer,
    /// Coerces to Double
    Float,
    /// Coerces to Timestamp
    Timestamp,
}

#[derive(Debug)]
/// Allocates memory for a column
pub struct ColumnBuffer {
    /// Type of the data for conversion
    kind: SqlType,

    /// Buffer for the column data
    buffer: Box<[u8]>,

    /// Null indicator
    nullind: Box<i16>,
}

impl ColumnBuffer {
    /// Allocate a buffer from an output (column) XSQLVAR, coercing the data types as necessary
    pub fn from_xsqlvar(var: &mut ibase::XSQLVAR) -> Result<Self, FbError> {
        // Remove nullable type indicator
        let sqltype = var.sqltype & (!1);

        let mut nullind = Box::new(0);
        var.sqlind = &mut *nullind;

        let (kind, mut buffer) = match sqltype as u32 {
            ibase::SQL_TEXT | ibase::SQL_VARYING => {
                // sqllen + 2 because the two bytes from the varchar length
                let buffer = vec![0; var.sqllen as usize + 2].into_boxed_slice();

                var.sqltype = ibase::SQL_VARYING as i16 + 1;

                (Text, buffer)
            }

            ibase::SQL_SHORT | ibase::SQL_LONG | ibase::SQL_INT64 => {
                var.sqllen = mem::size_of::<i64>() as i16;

                let buffer = vec![0; var.sqllen as usize].into_boxed_slice();

                if var.sqlscale == 0 {
                    var.sqltype = ibase::SQL_INT64 as i16 + 1;

                    (Integer, buffer)
                } else {
                    var.sqlscale = 0;
                    var.sqltype = ibase::SQL_DOUBLE as i16 + 1;

                    (Float, buffer)
                }
            }

            ibase::SQL_FLOAT | ibase::SQL_DOUBLE => {
                var.sqllen = mem::size_of::<i64>() as i16;

                let buffer = vec![0; var.sqllen as usize].into_boxed_slice();

                var.sqltype = ibase::SQL_DOUBLE as i16 + 1;

                (Float, buffer)
            }

            ibase::SQL_TIMESTAMP | ibase::SQL_TYPE_DATE | ibase::SQL_TYPE_TIME => {
                var.sqllen = mem::size_of::<ibase::ISC_TIMESTAMP>() as i16;

                let buffer = vec![0; var.sqllen as usize].into_boxed_slice();

                var.sqltype = ibase::SQL_TIMESTAMP as i16 + 1;

                (Timestamp, buffer)
            }

            sqltype => {
                return Err(FbError {
                    code: -1,
                    msg: format!("Unsupported column type ({})", sqltype),
                })
            }
        };

        var.sqldata = buffer.as_mut_ptr() as _;

        Ok(ColumnBuffer {
            kind,
            buffer,
            nullind,
        })
    }
}

/// Define the conversion from the buffer to a value
pub trait ColumnToVal<T> {
    fn to_val(&self) -> Result<T, FbError>
    where
        Self: std::marker::Sized;
}

impl ColumnToVal<String> for ColumnBuffer {
    fn to_val(&self) -> Result<String, FbError> {
        if *self.nullind < 0 {
            return err_column_null("String");
        }

        match self.kind {
            Text => varchar_to_string(&self.buffer),

            Integer => integer_from_buffer(&self.buffer).map(|i| i.to_string()),

            Float => float_from_buffer(&self.buffer).map(|f| f.to_string()),

            #[cfg(feature = "date_time")]
            Timestamp => {
                crate::date_time::timestamp_from_buffer(&self.buffer).map(|d| d.to_string())
            }

            #[cfg(not(feature = "date_time"))]
            Timestamp => Err(FbError {
                code: -1,
                msg: "Enable the `date_time` feature to use Timestamp, Date and Time types"
                    .to_string(),
            }),
        }
    }
}

impl ColumnToVal<i64> for ColumnBuffer {
    fn to_val(&self) -> Result<i64, FbError> {
        if *self.nullind < 0 {
            return err_column_null("i64");
        }

        match self.kind {
            Integer => integer_from_buffer(&self.buffer),

            _ => err_type_conv(self.kind, "i64"),
        }
    }
}

impl ColumnToVal<i32> for ColumnBuffer {
    fn to_val(&self) -> Result<i32, FbError> {
        ColumnToVal::<i64>::to_val(self).map(|i| i as i32)
    }
}

impl ColumnToVal<i16> for ColumnBuffer {
    fn to_val(&self) -> Result<i16, FbError> {
        ColumnToVal::<i64>::to_val(self).map(|i| i as i16)
    }
}

impl ColumnToVal<f64> for ColumnBuffer {
    fn to_val(&self) -> Result<f64, FbError> {
        if *self.nullind < 0 {
            return err_column_null("f64");
        }

        match self.kind {
            Float => float_from_buffer(&self.buffer),

            _ => err_type_conv(self.kind, "f64"),
        }
    }
}

impl ColumnToVal<f32> for ColumnBuffer {
    fn to_val(&self) -> Result<f32, FbError> {
        ColumnToVal::<f64>::to_val(self).map(|i| i as f32)
    }
}

#[cfg(feature = "date_time")]
impl ColumnToVal<chrono::NaiveDate> for ColumnBuffer {
    fn to_val(&self) -> Result<chrono::NaiveDate, FbError> {
        if *self.nullind < 0 {
            return err_column_null("NaiveDate");
        }

        match self.kind {
            Timestamp => crate::date_time::timestamp_from_buffer(&self.buffer).map(|ts| ts.date()),

            _ => err_type_conv(self.kind, "NaiveDate"),
        }
    }
}

#[cfg(feature = "date_time")]
impl ColumnToVal<chrono::NaiveTime> for ColumnBuffer {
    fn to_val(&self) -> Result<chrono::NaiveTime, FbError> {
        if *self.nullind < 0 {
            return err_column_null("NaiveTime");
        }

        match self.kind {
            Timestamp => crate::date_time::timestamp_from_buffer(&self.buffer).map(|ts| ts.time()),

            _ => err_type_conv(self.kind, "NaiveTime"),
        }
    }
}

#[cfg(feature = "date_time")]
impl ColumnToVal<chrono::NaiveDateTime> for ColumnBuffer {
    fn to_val(&self) -> Result<chrono::NaiveDateTime, FbError> {
        if *self.nullind < 0 {
            return err_column_null("NaiveDateTime");
        }

        match self.kind {
            Timestamp => crate::date_time::timestamp_from_buffer(&self.buffer),

            _ => err_type_conv(self.kind, "NaiveDateTime"),
        }
    }
}

/// Implements for all nullable variants
impl<T> ColumnToVal<Option<T>> for ColumnBuffer
where
    ColumnBuffer: ColumnToVal<T>,
{
    fn to_val(&self) -> Result<Option<T>, FbError> {
        if *self.nullind < 0 {
            return Ok(None);
        }

        Ok(Some(self.to_val()?))
    }
}

/// Converts a varchar in a buffer to a String
fn varchar_to_string(buffer: &[u8]) -> Result<String, FbError> {
    if buffer.len() < 2 {
        return err_buffer_len(2, buffer.len(), "String");
    }

    let len = i16::from_le_bytes(buffer[0..2].try_into().unwrap()) as usize;

    if len > buffer.len() - 2 {
        return err_buffer_len(len + 2, buffer.len(), "String");
    }

    std::str::from_utf8(&buffer[2..(len + 2)])
        .map(|str| str.to_string())
        .map_err(|_| FbError {
            code: -1,
            msg: "Found column with an invalid utf-8 string".to_owned(),
        })
}

/// Interprets an integer value from a buffer
fn integer_from_buffer(buffer: &[u8]) -> Result<i64, FbError> {
    let len = mem::size_of::<i64>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "i64");
    }

    Ok(i64::from_le_bytes(buffer.try_into().unwrap()))
}

/// Interprets a float value from a buffer
fn float_from_buffer(buffer: &[u8]) -> Result<f64, FbError> {
    let len = mem::size_of::<f64>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "f64");
    }

    Ok(f64::from_le_bytes(buffer.try_into().unwrap()))
}

/// Implemented for types that represents a list of values of columns
pub trait FromRow {
    fn try_from(row: &[ColumnBuffer]) -> Result<Self, FbError>
    where
        Self: std::marker::Sized;
}

/// For no columns
impl FromRow for () {
    fn try_from(_row: &[ColumnBuffer]) -> Result<Self, FbError>
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
            $( ColumnBuffer: ColumnToVal<$t>, )+
        {
            fn try_from(row: &[ColumnBuffer]) -> Result<Self, FbError> {
                let mut iter = row.iter();

                Ok(( $(
                    ColumnToVal::<$t>::to_val(
                        iter
                            .next()
                            .ok_or_else(|| {
                                FbError {
                                    code: -1,
                                    msg: format!("The sql returned less columns than the {} expected", row.len())
                                }
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
    #[allow(clippy::float_cmp)]
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
