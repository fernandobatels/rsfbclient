//!
//! Rust Firebird Client
//!
//! Representation of a fetched row
//!

use rsfbclient_core::{Column, ColumnType, FbError};
use std::{convert::TryInto, mem, result::Result};

use crate::ibase;

use SqlType::*;

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

    /// Converts the buffer to a Column
    pub fn to_column(&self) -> Result<Column, FbError> {
        if *self.nullind != 0 {
            return Ok(Column(None));
        }

        let col_type = match self.kind {
            Text => ColumnType::Text(varchar_to_string(&self.buffer)?),

            Integer => ColumnType::Integer(integer_from_buffer(&self.buffer)?),

            Float => ColumnType::Float(float_from_buffer(&self.buffer)?),

            Timestamp => ColumnType::Timestamp(timestamp_from_buffer(&self.buffer)?),
        };

        Ok(Column(Some(col_type)))
    }
}

/// Converts a varchar in a buffer to a String
fn varchar_to_string(buffer: &[u8]) -> Result<String, FbError> {
    if buffer.len() < 2 {
        return err_buffer_len(2, buffer.len(), "String");
    }

    let len = i16::from_ne_bytes(buffer[0..2].try_into().unwrap()) as usize;

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

    Ok(i64::from_ne_bytes(buffer.try_into().unwrap()))
}

/// Interprets a float value from a buffer
fn float_from_buffer(buffer: &[u8]) -> Result<f64, FbError> {
    let len = mem::size_of::<f64>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "f64");
    }

    Ok(f64::from_ne_bytes(buffer.try_into().unwrap()))
}

/// Interprets a timestamp value from a buffer
pub fn timestamp_from_buffer(buffer: &[u8]) -> Result<ibase::ISC_TIMESTAMP, FbError> {
    let len = mem::size_of::<ibase::ISC_TIMESTAMP>();
    assert_eq!(len, 8);
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "NaiveDateTime");
    }

    let date = ibase::ISC_TIMESTAMP {
        timestamp_date: ibase::ISC_DATE::from_ne_bytes(buffer[0..4].try_into().unwrap()),
        timestamp_time: ibase::ISC_TIME::from_ne_bytes(buffer[4..8].try_into().unwrap()),
    };

    Ok(date)
}

pub fn err_buffer_len<T>(expected: usize, found: usize, type_name: &str) -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: format!(
            "Invalid buffer size for type {:?} (expected: {}, found: {})",
            type_name, expected, found
        ),
    })
}
