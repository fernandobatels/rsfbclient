//!
//! Rust Firebird Client
//!
//! Native column buffers and complete column-name metadata
//!

use rsfbclient_core::{charset::OCTETS_CHARSET_ID, Charset, Column, FbError, SqlType};
use std::{mem, result::Result, sync::Arc};

use crate::{ibase, ibase::IBase, status::Status, varchar::Varchar};
use rsfbclient_core::FixedDecimal;

use ColumnBufferData::*;

#[derive(Debug)]
/// Types supported by the crate
pub enum ColumnBufferData {
    /// Coerces to Varchar
    Text(Varchar),
    /// Binary string, with no attachment character-set conversion.
    Octets(Varchar),
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
            Octets(v) => v.as_ptr() as _,
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

    raw_scale: i16,
    raw_subtype: i16,
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
        let original_scale = var.sqlscale;
        let original_type = var.sqltype & !1;
        if original_type as u32 == ibase::SQL_BLOB {
            var.sqlsubtype = 0;
        } else {
            let is_string = matches!(original_type as u32, ibase::SQL_TEXT | ibase::SQL_VARYING);
            if is_string && var.sqlsubtype as u16 == OCTETS_CHARSET_ID {
                return Self::from_xsqlvar_with_name(var, name, charset);
            }
            if !is_string {
                var.sqllen = 128;
            }
            var.sqltype = ibase::SQL_VARYING as i16 + 1;
            var.sqlscale = 0;
            var.sqlsubtype = 0;
        }
        let mut buffer = Self::from_xsqlvar_with_name(var, name, charset)?;
        buffer.raw_type = original_type;
        buffer.raw_scale = original_scale;
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
        let raw_scale = var.sqlscale;

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

                let buffer = Varchar::new(var.sqllen as u16);
                if sqlsubtype as u16 == OCTETS_CHARSET_ID {
                    Octets(buffer)
                } else {
                    Text(buffer)
                }
            }

            ibase::SQL_TIMESTAMP_TZ | ibase::SQL_TIME_TZ | ibase::SQL_DEC16 | ibase::SQL_DEC34 => {
                // Let Firebird format these values in the attachment character set.
                var.sqllen = 128;
                var.sqlscale = 0;
                var.sqlsubtype = 0;
                var.sqltype = ibase::SQL_VARYING as i16 + 1;
                Text(Varchar::new(128))
            }

            ibase::SQL_INT128 => {
                var.sqllen = mem::size_of::<i128>() as i16;
                var.sqltype = ibase::SQL_INT128 as i16 + 1;

                Int128(Box::new(0))
            }

            ibase::SQL_SHORT | ibase::SQL_LONG | ibase::SQL_INT64 => {
                var.sqllen = mem::size_of::<i64>() as i16;
                var.sqltype = ibase::SQL_INT64 as i16 + 1;

                Integer(Box::new(0))
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
            raw_scale,
            raw_subtype: sqlsubtype,
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
            return Ok(Column::with_scale(
                self.col_name.clone(),
                self.raw_type as u32,
                self.raw_scale,
                SqlType::Null,
            ));
        }

        let col_type = match &self.buffer {
            Text(varchar) => SqlType::Text(charset.decode(varchar.as_bytes())?),
            Octets(varchar) => SqlType::Octets(varchar.as_bytes().to_vec()),

            Integer(i) if self.raw_scale != 0 => {
                SqlType::Decimal(FixedDecimal::new(i128::from(**i), self.raw_scale as i8))
            }

            Integer(i) => SqlType::Integer(**i),

            Int128(i) if self.raw_scale != 0 || self.raw_subtype != 0 => {
                SqlType::Decimal(FixedDecimal::new(**i, self.raw_scale as i8))
            }

            Int128(i) => SqlType::Int128(**i),

            Float(f) => SqlType::Floating(**f),

            Timestamp(ts) => SqlType::Timestamp(rsfbclient_core::date_time::decode_timestamp(**ts)),

            BlobText(b) => SqlType::Text(blobtext_to_string(**b, db, tr, ibase, charset)?),

            BlobBinary(b) => SqlType::Binary(blobbinary_to_vec(**b, db, tr, ibase)?),

            Boolean(b) => SqlType::Boolean(**b != 0),
        };

        Ok(Column::with_scale(
            self.col_name.clone(),
            self.raw_type as u32,
            self.raw_scale,
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

/// What `isc_blob_info` reports about an open blob. `read_blob` uses it to bound the
/// `isc_get_segment` loop (fernandobatels/rsfbclient#197).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct BlobInfo {
    /// `isc_info_blob_total_length`: bytes in the blob. Exact for a stored blob; an
    /// estimate for a blob the server transliterates while it is read (see
    /// [`LENGTH_ESTIMATE_FACTOR`]).
    total_length: Option<u64>,
    /// `isc_info_blob_num_segments`: segments of a segmented blob. A transliterated read
    /// never splits a stored segment, though it may join two.
    num_segments: Option<u64>,
    /// `isc_info_blob_type`: `true` for a stream blob, `false` for a segmented one
    stream: Option<bool>,
}

const BLOB_INFO_ITEMS: [u8; 4] = [
    ibase::isc_info_blob_total_length as u8,
    ibase::isc_info_blob_num_segments as u8,
    ibase::isc_info_blob_type as u8,
    ibase::isc_info_end as u8,
];

/// Initial capacity of the buffer of a blob read, however long the blob says it is
const MAX_INITIAL_CAPACITY: u64 = 16 * 1024 * 1024;

/// How many times longer than its `total_length` a healthy blob can be.
///
/// For a blob the server transliterates while it is read (a `CAST` of a text blob to
/// another charset or to binary), `isc_blob_info` answers an estimate: the stored length
/// scaled by the growth of the longest stored segment, which the server caps at 65532
/// bytes (`filter_transliterate_text` in Firebird's `src/jrd/filters.cpp`). Because of
/// the cap, stored segments longer than 16 KB make the estimate smaller than the data:
/// two 65535-byte segments of `é` in ISO8859_1 read as UTF8 are 262140 bytes, reported
/// as 130046. The estimate never undercounts the stored bytes, and no Firebird charset
/// has characters longer than 4 bytes, so the data is at most four times the estimate.
const LENGTH_ESTIMATE_FACTOR: u64 = 4;

/// Consecutive `isc_get_segment` calls allowed to bring nothing from a stream blob.
///
/// A healthy stream blob yields data on every call until `isc_segstr_eof`. A client
/// library walking memory past the end of an inline blob buffer
/// (FirebirdSQL/firebird#9234) hands out whatever it finds there, often zero-length
/// frames, so the byte bound alone would not stop it. Segmented blobs can hold empty
/// segments legitimately and are bounded by their segment count instead.
const MAX_EMPTY_STREAM_CALLS: u32 = 64;

/// Asks the client library what it knows about an open blob
fn blob_info<T: IBase>(
    handle: &mut ibase::isc_blob_handle,
    status: &mut Status,
    ibase: &T,
) -> Result<BlobInfo, FbError> {
    // Three items of at most 1 + 2 + 4 bytes each, plus the end marker
    let mut buffer = [0_u8; 64];

    unsafe {
        if ibase.isc_blob_info()(
            &mut status[0],
            handle,
            BLOB_INFO_ITEMS.len() as i16,
            BLOB_INFO_ITEMS.as_ptr() as *const ibase::ISC_SCHAR,
            buffer.len() as i16,
            buffer.as_mut_ptr() as *mut ibase::ISC_SCHAR,
        ) != 0
        {
            return Err(status.as_error(ibase));
        }
    }

    Ok(parse_blob_info(&buffer))
}

/// Decodes an `isc_blob_info` answer: each item is followed by a little-endian 2-byte
/// length and a little-endian integer, up to `isc_info_end` or `isc_info_truncated`.
/// Items that are missing, unknown or oversized are left as `None`.
fn parse_blob_info(buffer: &[u8]) -> BlobInfo {
    let mut info = BlobInfo::default();
    let mut pos = 0;

    while let Some(&item) = buffer.get(pos) {
        pos += 1;
        let item = item as u32;
        if item == ibase::isc_info_end || item == ibase::isc_info_truncated {
            break;
        }

        let len = match buffer.get(pos..pos + 2) {
            Some(len) => u16::from_le_bytes([len[0], len[1]]) as usize,
            None => break,
        };
        pos += 2;

        let value = match buffer.get(pos..pos + len) {
            Some(bytes) if len <= 8 => Some(
                bytes
                    .iter()
                    .rev()
                    .fold(0_u64, |acc, b| (acc << 8) | u64::from(*b)),
            ),
            Some(_) => None,
            None => break,
        };
        pos += len;

        match (item, value) {
            (ibase::isc_info_blob_total_length, Some(v)) => info.total_length = Some(v),
            (ibase::isc_info_blob_num_segments, Some(v)) => info.num_segments = Some(v),
            (ibase::isc_info_blob_type, Some(v)) => info.stream = Some(v != 0),
            _ => {}
        }
    }

    info
}

/// Reads a whole blob.
///
/// The loop ends at `isc_segstr_eof`. What `isc_blob_info` reported bounds it, so that a
/// client library that never signals the end of the blob (FirebirdSQL/firebird#9234,
/// fernandobatels/rsfbclient#197) produces an error instead of appending memory past the
/// data until the process dies: the bytes may not exceed [`LENGTH_ESTIMATE_FACTOR`] times
/// the reported length, a segmented blob may not yield more segments than reported, and a
/// stream blob may not yield nothing [`MAX_EMPTY_STREAM_CALLS`] times in a row.
fn read_blob<T: IBase>(
    mut blob_id: ibase::GDS_QUAD_t,
    db: &mut ibase::isc_db_handle,
    tr: &mut ibase::isc_tr_handle,
    ibase: &T,
) -> Result<Vec<u8>, FbError> {
    let mut status = Status::default();
    let mut handle = 0;

    unsafe {
        if ibase.isc_open_blob()(&mut status[0], db, tr, &mut handle, &mut blob_id) != 0 {
            return Err(status.as_error(ibase));
        }
    }

    // Assert that the handle is valid
    debug_assert_ne!(handle, 0);

    let blob_bytes = read_segments(&mut handle, &mut status, ibase);

    // Close the blob also after a failed read, so the handle is not leaked
    let closed = unsafe { ibase.isc_close_blob()(&mut status[0], &mut handle) };

    let blob_bytes = blob_bytes?;

    if closed != 0 {
        return Err(status.as_error(ibase));
    }

    Ok(blob_bytes)
}

/// Reads the segments of an open blob
fn read_segments<T: IBase>(
    handle: &mut ibase::isc_blob_handle,
    status: &mut Status,
    ibase: &T,
) -> Result<Vec<u8>, FbError> {
    let info = blob_info(handle, status, ibase)?;

    let capacity = info
        .total_length
        .map_or(256, |len| len.min(MAX_INITIAL_CAPACITY) as usize);
    let mut blob_bytes = Vec::with_capacity(capacity);

    // Completed segments of a segmented blob (`isc_segment` means the segment did not fit
    // the buffer and continues in the next call), and calls in a row that brought nothing
    // from a stream blob
    let mut segments: u64 = 0;
    let mut empty_calls: u32 = 0;

    loop {
        let mut blob_seg_loaded: u16 = 0;
        let mut blob_seg_slice = [0_u8; 255];

        let blob_stat = unsafe {
            ibase.isc_get_segment()(
                &mut status[0],
                handle,
                &mut blob_seg_loaded,
                blob_seg_slice.len() as u16,
                blob_seg_slice.as_mut_ptr() as *mut std::os::raw::c_char,
            )
        };

        let segment_complete = if blob_stat == 0 {
            true
        } else if blob_stat == ibase::isc_segment as ibase::ISC_STATUS {
            false
        } else if blob_stat == ibase::isc_segstr_eof as ibase::ISC_STATUS {
            break;
        } else {
            return Err(status.as_error(ibase));
        };

        let loaded = usize::from(blob_seg_loaded).min(blob_seg_slice.len());
        blob_bytes.extend_from_slice(&blob_seg_slice[..loaded]);

        if let Some(total) = info.total_length {
            if blob_bytes.len() as u64 > total.saturating_mul(LENGTH_ESTIMATE_FACTOR) {
                return Err(runaway(format!(
                    "{} bytes were read from a blob that reports {} bytes",
                    blob_bytes.len(),
                    total
                )));
            }
        }

        match info.stream {
            Some(false) if segment_complete => {
                segments += 1;

                if let Some(count) = info.num_segments {
                    if segments > count {
                        return Err(runaway(format!(
                            "{} segments were read from a blob that reports {} segments",
                            segments, count
                        )));
                    }
                }
            }
            Some(true) if loaded == 0 => {
                empty_calls += 1;

                if empty_calls > MAX_EMPTY_STREAM_CALLS {
                    return Err(runaway(format!(
                        "{} calls in a row brought nothing from a stream blob",
                        empty_calls
                    )));
                }
            }
            Some(true) => empty_calls = 0,
            _ => {}
        }
    }

    Ok(blob_bytes)
}

/// The client library handed back more than the blob has and did not signal its end
fn runaway(what: String) -> FbError {
    FbError::Other(format!(
        "{} and the client library did not signal the end of the blob: the read was stopped \
         (see https://github.com/fernandobatels/rsfbclient/issues/197)",
        what
    ))
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

    fn item(tag: u32, value: u32) -> Vec<u8> {
        let mut v = vec![tag as u8, 4, 0];
        v.extend_from_slice(&value.to_le_bytes());
        v
    }

    #[test]
    fn parses_every_item() {
        let mut buffer = Vec::new();
        buffer.extend(item(ibase::isc_info_blob_total_length, 131070));
        buffer.extend(item(ibase::isc_info_blob_num_segments, 4));
        buffer.extend(item(ibase::isc_info_blob_max_segment, 65532)); // not asked for: skipped
        buffer.extend([ibase::isc_info_blob_type as u8, 1, 0, 0]);
        buffer.push(ibase::isc_info_end as u8);
        buffer.extend([0xAA; 8]); // garbage after the end marker is ignored

        assert_eq!(
            BlobInfo {
                total_length: Some(131070),
                num_segments: Some(4),
                stream: Some(false),
            },
            parse_blob_info(&buffer)
        );
    }

    #[test]
    fn stream_blob_type() {
        let mut buffer = vec![ibase::isc_info_blob_type as u8, 1, 0, 1];
        buffer.push(ibase::isc_info_end as u8);

        assert_eq!(Some(true), parse_blob_info(&buffer).stream);
    }

    #[test]
    fn truncated_answer_keeps_what_came_before() {
        let mut buffer = item(ibase::isc_info_blob_total_length, 10);
        buffer.push(ibase::isc_info_truncated as u8);
        buffer.extend(item(ibase::isc_info_blob_num_segments, 1));

        assert_eq!(
            BlobInfo {
                total_length: Some(10),
                ..BlobInfo::default()
            },
            parse_blob_info(&buffer)
        );
    }

    #[test]
    fn unknown_error_and_oversized_items_are_skipped() {
        let mut buffer = item(ibase::isc_info_error, 335544321);
        buffer.extend([0x7F, 2, 0, 1, 2]); // unknown item
        buffer.extend([ibase::isc_info_blob_max_segment as u8, 9, 0]); // 9-byte value
        buffer.extend([1; 9]);
        buffer.extend(item(ibase::isc_info_blob_total_length, 10));
        buffer.push(ibase::isc_info_end as u8);

        assert_eq!(
            BlobInfo {
                total_length: Some(10),
                ..BlobInfo::default()
            },
            parse_blob_info(&buffer)
        );
    }

    #[test]
    fn answer_without_end_marker_or_cut_short() {
        assert_eq!(BlobInfo::default(), parse_blob_info(&[]));
        assert_eq!(BlobInfo::default(), parse_blob_info(&[0; 64]));
        // Length announces more bytes than the buffer has
        assert_eq!(
            BlobInfo::default(),
            parse_blob_info(&[ibase::isc_info_blob_total_length as u8, 4, 0, 1])
        );
    }

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
