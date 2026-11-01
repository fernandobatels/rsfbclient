//!
//! Rust Firebird Client
//!
//! Native column buffers and complete column-name metadata
//!

use rsfbclient_core::{Charset, Column, FbError, SqlType};
use std::{mem, result::Result, sync::Arc};

use crate::{ibase, ibase::IBase, status::Status, varchar::Varchar};

use ColumnBufferData::*;

#[derive(Debug)]
/// Types supported by the crate
pub enum ColumnBufferData {
    /// Coerces to Varchar
    Text(Varchar),
    /// Coerces to Int64
    Integer(Box<i64>),
    /// Native Firebird INT128
    Int128(Box<i128>),
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
            Int128(i) => i.as_mut() as *mut i128 as _,
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

    /// Column name. `Arc<str>` (built once here) so cloning it into every fetched
    /// row's `Column` is a refcount bump rather than a per-row allocation.
    col_name: Arc<str>,

    raw_type: i16,
}

impl ColumnBuffer {
    /// Ask Firebird to format scalar columns as text without floating-point loss.
    /// Blobs are returned byte-for-byte, irrespective of their declared subtype.
    pub(crate) fn from_xsqlvar_text(
        var: &mut ibase::XSQLVAR,
        name: Option<String>,
        charset: &Charset,
    ) -> Result<Self, FbError> {
        let original_type = var.sqltype & !1;
        if original_type as u32 == ibase::SQL_BLOB {
            var.sqlsubtype = 0;
        } else {
            if ![ibase::SQL_TEXT as i16, ibase::SQL_VARYING as i16].contains(&original_type) {
                var.sqllen = 128;
            }
            var.sqltype = ibase::SQL_VARYING as i16 + 1;
            var.sqlscale = 0;
            var.sqlsubtype = 0;
        }
        let mut buffer = Self::from_xsqlvar_with_name(var, name, charset)?;
        buffer.raw_type = original_type;
        Ok(buffer)
    }

    /// Allocate a buffer from an output (column) XSQLVAR, coercing the data types as necessary
    pub fn from_xsqlvar(var: &mut ibase::XSQLVAR) -> Result<Self, FbError> {
        Self::from_xsqlvar_with_name(var, None, &rsfbclient_core::charset::UTF_8)
    }

    pub(crate) fn from_xsqlvar_with_name(
        var: &mut ibase::XSQLVAR,
        name: Option<String>,
        charset: &Charset,
    ) -> Result<Self, FbError> {
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

            ibase::SQL_BLOB if (sqlsubtype <= 1) => {
                let blob_id = Box::new(ibase::GDS_QUAD_t {
                    gds_quad_high: 0,
                    gds_quad_low: 0,
                });

                var.sqltype = ibase::SQL_BLOB as i16 + 1;

                // subtype 0: binary
                // subtype <= -1: custom
                if sqlsubtype <= 0 {
                    BlobBinary(blob_id)
                } else {
                    BlobText(blob_id)
                }
            }

            ibase::SQL_TEXT | ibase::SQL_VARYING => {
                var.sqltype = ibase::SQL_VARYING as i16 + 1;

                Text(Varchar::new(var.sqllen as u16))
            }

            ibase::SQL_INT128 if sqlsubtype == 0 && var.sqlscale == 0 => {
                var.sqllen = mem::size_of::<i128>() as i16;
                var.sqltype = ibase::SQL_INT128 as i16 + 1;

                Int128(Box::new(0))
            }

            ibase::SQL_INT128 => {
                // NUMERIC/DECIMAL backed by INT128 follows the crate's existing
                // fixed-point policy and is converted to double by fbclient.
                var.sqllen = mem::size_of::<f64>() as i16;
                var.sqlscale = 0;
                var.sqltype = ibase::SQL_DOUBLE as i16 + 1;

                Float(Box::new(0.0))
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

        let col_name: Arc<str> = if let Some(name) = name {
            name.into()
        } else {
            let len = usize::min(var.aliasname_length as usize, var.aliasname.len());
            let bname = var.aliasname[..len]
                .iter()
                .map(|b| *b as u8)
                .collect::<Vec<u8>>();

            charset.decode(bname)?.into()
        };

        Ok(ColumnBuffer {
            buffer,
            nullind,
            col_name,
            raw_type: sqltype,
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
            return Ok(Column::new(
                self.col_name.clone(),
                self.raw_type as u32,
                SqlType::Null,
            ));
        }

        let col_type = match &self.buffer {
            Text(varchar) => SqlType::Text(charset.decode(varchar.as_bytes())?),

            Integer(i) => SqlType::Integer(**i),

            Int128(i) => SqlType::Int128(**i),

            Float(f) => SqlType::Floating(**f),

            Timestamp(ts) => SqlType::Timestamp(rsfbclient_core::date_time::decode_timestamp(**ts)),

            BlobText(b) => SqlType::Text(blobtext_to_string(**b, db, tr, ibase, charset)?),

            BlobBinary(b) => SqlType::Binary(blobbinary_to_vec(**b, db, tr, ibase)?),

            Boolean(b) => SqlType::Boolean(**b != 0),
        };

        Ok(Column::new(
            self.col_name.clone(),
            self.raw_type as u32,
            col_type,
        ))
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
        let mut blob_seg_loaded = 0;
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

// SQL-info aliases are length-delimited and preserve complete identifiers.

pub(crate) fn parse_aliases_with_charset(
    data: &[u8],
    names: &mut [Option<String>],
    charset: &Charset,
) -> Result<bool, FbError> {
    let mut offset = 0;
    let mut column = None;
    while let Some(&item) = data.get(offset) {
        offset += 1;
        match item as u32 {
            ibase::isc_info_end => return Ok(false),
            ibase::isc_info_truncated => return Ok(true),
            ibase::isc_info_sql_select | ibase::isc_info_sql_describe_end => continue,
            _ => {}
        }
        let length = data
            .get(offset..offset + 2)
            .ok_or("Incomplete native column-name metadata length")?;
        offset += 2;
        let length = u16::from_le_bytes([length[0], length[1]]) as usize;
        let value = data
            .get(offset..offset + length)
            .ok_or("Incomplete native column-name metadata value")?;
        offset += length;
        match item as u32 {
            ibase::isc_info_sql_describe_vars | ibase::isc_info_sql_sqlda_seq => {
                if value.is_empty() || value.len() > 4 {
                    return Err("Invalid native column-name metadata number".into());
                }
                let number = value
                    .iter()
                    .enumerate()
                    .fold(0u32, |n, (i, b)| n | ((*b as u32) << (i * 8)))
                    as usize;
                if item as u32 == ibase::isc_info_sql_describe_vars {
                    if number != names.len() {
                        return Err("Native column-name metadata count changed".into());
                    }
                } else {
                    if number == 0 || number > names.len() {
                        return Err("Invalid native column-name metadata index".into());
                    }
                    column = Some(number - 1);
                }
            }
            ibase::isc_info_sql_alias => {
                let column = column.ok_or("Native column alias has no column index")?;
                names[column] = Some(charset.decode(value)?);
            }
            _ => return Err(format!("Unexpected native column-name metadata item {item}").into()),
        }
    }
    Err("Unterminated native column-name metadata".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_aliases(data: &[u8], names: &mut [Option<String>]) -> Result<bool, FbError> {
        parse_aliases_with_charset(data, names, &rsfbclient_core::charset::UTF_8)
    }
    #[test]
    fn attachment_character_set_decodes_non_utf8_aliases_without_replacement() {
        let mut bytes = header(1);
        value(&mut bytes, 9, &1u32.to_le_bytes());
        value(&mut bytes, 19, &[0xc6, 0xc6]);
        bytes.push(1);
        let mut names = vec![None];
        assert!(!parse_aliases_with_charset(
            &bytes,
            &mut names,
            &rsfbclient_core::charset::WIN_1251
        )
        .unwrap());
        assert_eq!(names, vec![Some("ЖЖ".into())]);
        assert!(parse_aliases(&bytes, &mut [None]).is_err());
    }
    fn value(output: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
        output.push(tag);
        output.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        output.extend_from_slice(bytes);
    }
    fn header(count: u32) -> Vec<u8> {
        let mut result = vec![4];
        value(&mut result, 7, &count.to_le_bytes());
        result
    }
    #[test]
    fn full_utf8_aliases_and_duplicate_aliases_keep_their_column_positions() {
        let name = "Ж".repeat(63);
        let mut bytes = header(2);
        for column in 1..=2u32 {
            value(&mut bytes, 9, &column.to_le_bytes());
            value(&mut bytes, 19, name.as_bytes());
            bytes.push(8);
        }
        bytes.push(1);
        let mut names = vec![None, None];
        assert!(!parse_aliases(&bytes, &mut names).unwrap());
        assert_eq!(names, vec![Some(name.clone()), Some(name)]);
    }
    #[test]
    fn a_truncated_page_can_be_completed_from_the_next_column() {
        let mut first = header(2);
        value(&mut first, 9, &1u32.to_le_bytes());
        value(&mut first, 19, b"first");
        first.extend([8, 2]);
        let mut names = vec![None, None];
        assert!(parse_aliases(&first, &mut names).unwrap());
        assert!(names[1].is_none());
        let mut next = header(2);
        value(&mut next, 9, &2u32.to_le_bytes());
        value(&mut next, 19, b"second");
        next.extend([8, 1]);
        assert!(!parse_aliases(&next, &mut names).unwrap());
        assert_eq!(names, vec![Some("first".into()), Some("second".into())]);
    }
    #[test]
    fn malformed_metadata_never_becomes_a_truncated_or_lossy_identifier() {
        for bytes in [
            vec![4, 7, 4],
            vec![4, 7, 4, 0, 2, 0],
            vec![4, 19, 1, 0, b'x'],
            vec![4, 9, 4, 0, 0, 0, 0, 0],
            vec![4, 9, 4, 0, 3, 0, 0, 0],
        ] {
            assert!(parse_aliases(&bytes, &mut [None, None]).is_err());
        }
        let mut bytes = header(1);
        value(&mut bytes, 9, &1u32.to_le_bytes());
        value(&mut bytes, 19, &[0xd0]);
        bytes.push(1);
        assert!(parse_aliases(&bytes, &mut [None]).is_err());
    }
}
