//!
//! Rust Firebird Client
//!
//! Representation of a fetched row
//!

use rsfbclient_core::{Charset, Column, FbError, SqlType};
use std::{mem, result::Result};

use crate::{ibase, ibase::IBase, status::Status, varchar::Varchar};

use ColumnBufferData::*;

#[derive(Debug)]
/// Types supported by the crate
pub enum ColumnBufferData {
    /// Coerces to Varchar
    Text(Varchar),
    /// Coerces to Int64
    Integer(Box<i64>),
    /// Coerces to Double
    Float(Box<f64>),
    /// Coerces to Timestamp
    Timestamp(Box<ibase::ISC_TIMESTAMP>),
    /// Coerces to Blob sub_type 1
    BlobText(Box<ibase::GDS_QUAD_t>),
    /// Coerces to Blob sub_type 0
    BlobBinary(Box<ibase::GDS_QUAD_t>),
    /// Coerces to boolean. Fb >= 3
    Boolean(Box<i8>),
}

impl ColumnBufferData {
    fn as_mut_ptr(&mut self) -> *mut ibase::ISC_SCHAR {
        match self {
            Text(v) => v.as_ptr() as _,
            Integer(i) => &**i as *const _ as _,
            Float(f) => &**f as *const _ as _,
            Timestamp(ts) => &**ts as *const _ as _,
            BlobText(bid) => &**bid as *const _ as _,
            BlobBinary(bid) => &**bid as *const _ as _,
            Boolean(b) => &**b as *const _ as _,
        }
    }
}

#[derive(Debug)]
/// Allocates memory for a column
pub struct ColumnBuffer {
    /// Buffer for the column data
    buffer: ColumnBufferData,

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

        let mut buffer = match sqltype as u32 {
            ibase::SQL_BOOLEAN => {
                var.sqllen = mem::size_of::<i8>() as i16;

                var.sqltype = ibase::SQL_BOOLEAN as i16 + 1;

                Boolean(Box::new(0))
            }

            // BLOB sql_type text are considered a normal text on read
            ibase::SQL_BLOB if (sqlsubtype == 0 || sqlsubtype == 1) => {
                let blob_id = Box::new(ibase::GDS_QUAD_t {
                    gds_quad_high: 0,
                    gds_quad_low: 0,
                });

                var.sqltype = ibase::SQL_BLOB as i16 + 1;

                if sqlsubtype == 0 {
                    BlobBinary(blob_id)
                } else {
                    BlobText(blob_id)
                }
            }

            ibase::SQL_TEXT | ibase::SQL_VARYING => {
                var.sqltype = ibase::SQL_VARYING as i16 + 1;

                Text(Varchar::new(var.sqllen as u16))
            }

            ibase::SQL_SHORT | ibase::SQL_LONG | ibase::SQL_INT64 => {
                var.sqllen = mem::size_of::<i64>() as i16;

                if var.sqlscale == 0 {
                    var.sqltype = ibase::SQL_INT64 as i16 + 1;

                    Integer(Box::new(0))
                } else {
                    var.sqlscale = 0;
                    var.sqltype = ibase::SQL_DOUBLE as i16 + 1;

                    Float(Box::new(0.0))
                }
            }

            ibase::SQL_FLOAT | ibase::SQL_DOUBLE => {
                var.sqllen = mem::size_of::<i64>() as i16;

                var.sqltype = ibase::SQL_DOUBLE as i16 + 1;

                Float(Box::new(0.0))
            }

            ibase::SQL_TIMESTAMP | ibase::SQL_TYPE_DATE | ibase::SQL_TYPE_TIME => {
                var.sqllen = mem::size_of::<ibase::ISC_TIMESTAMP>() as i16;

                var.sqltype = ibase::SQL_TIMESTAMP as i16 + 1;

                Timestamp(Box::new(ibase::ISC_TIMESTAMP {
                    timestamp_date: 0,
                    timestamp_time: 0,
                }))
            }

            sqltype => {
                return Err(format!("Unsupported column type ({} {})", sqltype, sqlsubtype).into())
            }
        };

        var.sqldata = buffer.as_mut_ptr();

        let col_name = {
            let len = usize::min(var.aliasname_length as usize, var.aliasname.len());
            let bname = var.aliasname[..len]
                .iter()
                .map(|b| *b as u8)
                .collect::<Vec<u8>>();

            String::from_utf8(bname)
        }?;

        Ok(ColumnBuffer {
            buffer,
            nullind,
            col_name,
        })
    }

    /// Converts the buffer to a Column
    pub fn to_column<T: IBase>(
        &self,
        db: &mut ibase::isc_db_handle,
        tr: &mut ibase::isc_tr_handle,
        ibase: &T,
        charset: &Charset,
    ) -> Result<Column, FbError> {
        if *self.nullind != 0 {
            return Ok(Column::new(self.col_name.clone(), SqlType::Null));
        }

        let col_type = match &self.buffer {
            Text(varchar) => SqlType::Text(charset.decode(varchar.as_bytes())?),

            Integer(i) => SqlType::Integer(**i),

            Float(f) => SqlType::Floating(**f),

            #[cfg(feature = "date_time")]
            Timestamp(ts) => SqlType::Timestamp(rsfbclient_core::date_time::decode_timestamp(**ts)),

            BlobText(b) => SqlType::Text(blobtext_to_string(**b, db, tr, ibase, &charset)?),

            BlobBinary(b) => SqlType::Binary(blobbinary_to_vec(**b, db, tr, ibase)?),

            Boolean(b) => SqlType::Boolean(**b != 0),
        };

        Ok(Column::new(self.col_name.clone(), col_type))
    }
}

/// Converts a binary blob to a vec<u8>
fn blobbinary_to_vec<T: IBase>(
    blob_id: ibase::GDS_QUAD_t,
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &T,
) -> Result<Vec<u8>, FbError> {
    read_blob(blob_id, db, tr, ibase)
}

/// Converts a text blob to a string
fn blobtext_to_string<T: IBase>(
    blob_id: ibase::GDS_QUAD_t,
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &T,
    charset: &Charset,
) -> Result<String, FbError> {
    let blob_bytes = read_blob(blob_id, db, tr, ibase)?;

    charset.decode(blob_bytes)
}

/// Read the blob type
fn read_blob<T: IBase>(
    mut blob_id: ibase::GDS_QUAD_t,
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &T,
) -> Result<Vec<u8>, FbError> {
    let mut status = Status::default();
    let mut handle = 0;

    let mut blob_bytes = Vec::with_capacity(256);

    unsafe {
        if ibase.isc_open_blob()(&mut status[0], db, tr, &mut handle, &mut blob_id) != 0 {
            return Err(status.as_error(ibase));
        }
    }

    // Assert that the handle is valid
    debug_assert_ne!(handle, 0);

    let mut blob_stat = 0;

    while blob_stat == 0 || status[1] == (ibase::isc_segment as isize) {
        let mut blob_seg_loaded = 0 as u16;
        let mut blob_seg_slice = [0_u8; 255];

        blob_stat = unsafe {
            ibase.isc_get_segment()(
                &mut status[0],
                &mut handle,
                &mut blob_seg_loaded,
                blob_seg_slice.len() as u16,
                blob_seg_slice.as_mut_ptr() as *mut std::os::raw::c_char,
            )
        };

        blob_bytes.extend_from_slice(&blob_seg_slice[..blob_seg_loaded as usize]);
    }

    unsafe {
        if ibase.isc_close_blob()(&mut status[0], &mut handle) != 0 {
            return Err(status.as_error(ibase));
        }
    }

    Ok(blob_bytes)
}
