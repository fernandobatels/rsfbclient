//!
//! Rust Firebird Client
//!
//! Representation of a fetched row
//!

use rsfbclient_core::{Column, ColumnType, FbError};
use std::{convert::TryInto, ffi, mem, result::Result, str};

use crate::{ibase, ibase::IBase, status::Status};

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
    /// Coerces to Blob sub_type 1
    BlobText,
    /// Coerces to Blob sub_type 0
    BlobBinary,
    /// Coerces to boolean. Fb >= 3
    Boolean,
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

    /// Column name
    col_name: String,
}

impl ColumnBuffer {
    /// Allocate a buffer from an output (column) XSQLVAR, coercing the data types as necessary
    pub fn from_xsqlvar(var: &mut ibase::XSQLVAR) -> Result<Self, FbError> {
        // Remove nullable type indicator
        let sqltype = var.sqltype & (!1);
        let sqlsubtype = var.sqlsubtype;

        let mut nullind = Box::new(0);
        var.sqlind = &mut *nullind;

        let (kind, mut buffer) = match sqltype as u32 {
            ibase::SQL_BOOLEAN => {
                var.sqllen = mem::size_of::<i8>() as i16;

                let buffer = vec![0; var.sqllen as usize].into_boxed_slice();

                var.sqltype = ibase::SQL_BOOLEAN as i16 + 1;

                (Boolean, buffer)
            }

            // BLOB sql_type text are considered a normal text on read
            ibase::SQL_BLOB if (sqlsubtype == 0 || sqlsubtype == 1) => {
                let buffer = vec![0; var.sqllen as usize].into_boxed_slice();

                var.sqltype = ibase::SQL_BLOB as i16 + 1;

                if sqlsubtype == 0 {
                    (BlobBinary, buffer)
                } else {
                    (BlobText, buffer)
                }
            }

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
                return Err(format!("Unsupported column type ({} {})", sqltype, sqlsubtype).into())
            }
        };

        var.sqldata = buffer.as_mut_ptr() as _;

        let col_name = {
            let len = usize::min(var.aliasname_length as usize, var.aliasname.len());
            let bname = var.aliasname[..len]
                .iter()
                .map(|b| *b as u8)
                .collect::<Vec<u8>>();

            String::from_utf8(bname)
                .map_err(|_| FbError::from("Found a column name with an invalid utf-8 string"))
        }?;

        Ok(ColumnBuffer {
            kind,
            buffer,
            nullind,
            col_name,
        })
    }

    /// Converts the buffer to a Column
    pub fn to_column(
        &self,
        db: &mut ibase::isc_db_handle,
        tr: &mut ibase::isc_tr_handle,
        ibase: &IBase,
    ) -> Result<Column, FbError> {
        if *self.nullind != 0 {
            return Ok(Column::new(self.col_name.clone(), None));
        }

        let col_type = match self.kind {
            Text => ColumnType::Text(varchar_to_string(&self.buffer)?),

            Integer => ColumnType::Integer(integer_from_buffer(&self.buffer)?),

            Float => ColumnType::Float(float_from_buffer(&self.buffer)?),

            Timestamp => ColumnType::Timestamp(timestamp_from_buffer(&self.buffer)?),

            BlobText => ColumnType::Text(blobtext_to_string(&self.buffer, db, tr, ibase)?),

            BlobBinary => ColumnType::Binary(blobbinary_to_vec(&self.buffer, db, tr, ibase)?),

            Boolean => ColumnType::Boolean(boolean_from_buffer(&self.buffer)?),
        };

        Ok(Column::new(self.col_name.clone(), Some(col_type)))
    }
}

/// Interprets a boolean value from a buffer
fn boolean_from_buffer(buffer: &[u8]) -> Result<bool, FbError> {
    let len = mem::size_of::<i8>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "bool");
    }

    Ok(i8::from_ne_bytes(buffer.try_into().unwrap()) != 0)
}

/// Converts a binary blob to a vec<u8>
fn blobbinary_to_vec(
    buffer: &[u8],
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &IBase,
) -> Result<Vec<u8>, FbError> {
    let mut final_bin = vec![];

    read_blob(
        buffer,
        db,
        tr,
        ibase,
        |blob_seg_loaded, blob_seg_vec, _blob_seg| {
            for byte in &blob_seg_vec[0..(blob_seg_loaded as usize)] {
                final_bin.push(*byte as u8);
            }

            Ok(())
        },
    )?;

    Ok(final_bin)
}

/// Converts a text blob to a string
fn blobtext_to_string(
    buffer: &[u8],
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &IBase,
) -> Result<String, FbError> {
    let mut final_string = String::new();

    read_blob(
        buffer,
        db,
        tr,
        ibase,
        |_blob_seg_loaded, _blob_seg_vec, blob_seg| {
            let blob_seg_cstr = unsafe { ffi::CStr::from_ptr(blob_seg) };

            let blob_seg_str = blob_seg_cstr
                .to_str()
                .map_err(|_| FbError::from("Found column with an invalid utf-8 string"))?;

            final_string.push_str(blob_seg_str);

            Ok(())
        },
    )?;

    Ok(final_string)
}

/// Read the blob type
fn read_blob<F>(
    buffer: &[u8],
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &IBase,
    mut on_read_segment: F,
) -> Result<(), FbError>
where
    F: FnMut(u16, Vec<i8>, *mut i8) -> Result<(), FbError>,
{
    let mut status = Status::default();
    let mut handle = 0;

    let len = mem::size_of::<ibase::GDS_QUAD_t>();
    assert_eq!(len, 8);
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "Blob");
    }

    let mut blob_id = ibase::GDS_QUAD_t {
        gds_quad_high: ibase::ISC_LONG::from_ne_bytes(buffer[0..4].try_into().unwrap()),
        gds_quad_low: ibase::ISC_ULONG::from_ne_bytes(buffer[4..8].try_into().unwrap()),
    };

    unsafe {
        if ibase.isc_open_blob()(&mut status[0], db, tr, &mut handle, &mut blob_id) != 0 {
            return Err(status.as_error(&ibase));
        }
    }

    // Assert that the handle is valid
    debug_assert_ne!(handle, 0);

    let mut blob_stat = 0;

    while blob_stat == 0 || status[1] == (ibase::isc_segment as isize) {
        let mut blob_seg_loaded = 0 as u16;
        let mut blob_seg_slice = [0; 255];
        let blob_seg = blob_seg_slice.as_mut_ptr();

        blob_stat = unsafe {
            ibase.isc_get_segment()(
                &mut status[0],
                &mut handle,
                &mut blob_seg_loaded,
                blob_seg_slice.len() as u16,
                blob_seg,
            )
        };

        on_read_segment(blob_seg_loaded, blob_seg_slice.to_vec(), blob_seg)?;
    }

    unsafe {
        if ibase.isc_close_blob()(&mut status[0], &mut handle) != 0 {
            return Err(status.as_error(&ibase));
        }
    }

    Ok(())
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

    str::from_utf8(&buffer[2..(len + 2)])
        .map(|str| str.to_string())
        .map_err(|_| "Found column with an invalid utf-8 string".into())
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
    Err(format!(
        "Invalid buffer size for type {:?} (expected: {}, found: {})",
        type_name, expected, found
    )
    .into())
}
