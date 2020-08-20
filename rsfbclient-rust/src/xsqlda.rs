//! Structs and functions to parse and send data about the sql parameters and columns

#![allow(non_upper_case_globals)]

use bytes::{Buf, BufMut, Bytes, BytesMut};
use rsfbclient_core::{ibase, FbError, StmtType};
use std::{convert::TryFrom, mem};

use crate::consts;

/// Data to return about a statement
pub const XSQLDA_DESCRIBE_VARS: [u8; 17] = [
    ibase::isc_info_sql_stmt_type as u8, // Statement type: StmtType
    ibase::isc_info_sql_bind as u8,      // Select params
    ibase::isc_info_sql_describe_vars as u8, // Param count
    ibase::isc_info_sql_describe_end as u8, // End of param data
    ibase::isc_info_sql_select as u8,    // Select columns
    ibase::isc_info_sql_describe_vars as u8, // Column count
    ibase::isc_info_sql_sqlda_seq as u8, // Column index
    ibase::isc_info_sql_type as u8,      // Sql Type code
    ibase::isc_info_sql_sub_type as u8,  // Blob subtype
    ibase::isc_info_sql_scale as u8,     // Decimal / Numeric scale
    ibase::isc_info_sql_length as u8,    // Data length
    ibase::isc_info_sql_null_ind as u8,  // Null indicator (0 or -1)
    ibase::isc_info_sql_field as u8,     //
    ibase::isc_info_sql_relation as u8,  //
    ibase::isc_info_sql_owner as u8,     //
    ibase::isc_info_sql_alias as u8,     // Column alias
    ibase::isc_info_sql_describe_end as u8, // End of column data
];

#[derive(Debug, Default)]
/// Sql query column information
pub struct XSqlVar {
    /// Sql type code
    pub sqltype: i16,

    /// Scale: indicates that the real value is `data * 10.pow(scale)`
    pub scale: i16,

    /// Blob subtype code
    pub sqlsubtype: i16,

    /// Length of the column data
    pub data_length: i16,

    /// Null indicator
    pub null_ind: bool,

    pub field_name: String,

    pub relation_name: String,

    pub owner_name: String,

    /// Column alias
    pub alias_name: String,
}

impl XSqlVar {
    /// Coerces the data types of this XSqlVar as necessary
    pub fn coerce(&mut self) -> Result<(), FbError> {
        // Remove nullable type indicator
        let sqltype = self.sqltype & (!1);
        let sqlsubtype = self.sqlsubtype;

        // var.null_ind = 1;

        match sqltype as u32 {
            ibase::SQL_TEXT | ibase::SQL_VARYING => {
                self.sqltype = ibase::SQL_VARYING as i16 + 1;
            }

            ibase::SQL_SHORT | ibase::SQL_LONG | ibase::SQL_INT64 => {
                self.data_length = mem::size_of::<i64>() as i16;

                if self.scale == 0 {
                    self.sqltype = ibase::SQL_INT64 as i16 + 1;
                } else {
                    // Is actually a decimal or numeric value, so coerce as double
                    self.scale = 0;
                    self.sqltype = ibase::SQL_DOUBLE as i16 + 1;
                }
            }

            ibase::SQL_FLOAT | ibase::SQL_DOUBLE => {
                self.data_length = mem::size_of::<i64>() as i16;

                self.sqltype = ibase::SQL_DOUBLE as i16 + 1;
            }

            ibase::SQL_TIMESTAMP | ibase::SQL_TYPE_DATE | ibase::SQL_TYPE_TIME => {
                self.data_length = mem::size_of::<ibase::ISC_TIMESTAMP>() as i16;

                self.sqltype = ibase::SQL_TIMESTAMP as i16 + 1;
            }

            // TODO: proper blob support
            ibase::SQL_BLOB if (sqlsubtype == 0 || sqlsubtype == 1) => {
                self.sqltype = ibase::SQL_BLOB as i16 + 1;

                if sqlsubtype == 0 {
                    return Err("Blob type 0 not yet supported".into());
                } else {
                    // Coerce as varchar for now
                    self.sqltype = ibase::SQL_VARYING as i16 + 1;
                    self.data_length = crate::blr::MAX_DATA_LENGTH as i16;
                }
            }

            sqltype => {
                return Err(format!("Unsupported column type ({})", sqltype).into());
            }
        }

        Ok(())
    }
}

/// Convert the xsqlda to blr (binary representation)
pub fn xsqlda_to_blr(xsqlda: &[XSqlVar]) -> Result<Bytes, FbError> {
    let mut blr = BytesMut::with_capacity(256);
    blr.put_slice(&[
        consts::blr::VERSION5,
        consts::blr::BEGIN,
        consts::blr::MESSAGE,
        0, // Message index
    ]);
    // Message length, * 2 as there is 1 msg for the param type and another for the nullind
    blr.put_u16_le(xsqlda.len() as u16 * 2);

    for var in xsqlda {
        // Remove nullable type indicator
        let sqltype = var.sqltype as u32 & (!1);

        match sqltype as u32 {
            ibase::SQL_VARYING => {
                blr.put_u8(consts::blr::VARYING);
                blr.put_i16_le(var.data_length);
            }

            ibase::SQL_INT64 => blr.put_slice(&[
                consts::blr::INT64,
                0, // Scale
            ]),

            ibase::SQL_DOUBLE => blr.put_u8(consts::blr::DOUBLE),

            ibase::SQL_TIMESTAMP => blr.put_u8(consts::blr::TIMESTAMP),

            sqltype => {
                return Err(format!("Conversion from sql type {} not implemented", sqltype).into());
            }
        }
        // Nullind
        blr.put_slice(&[consts::blr::SHORT, 0]);
    }

    blr.put_slice(&[consts::blr::END, consts::blr::EOC]);

    Ok(blr.freeze())
}

/// Data returned for a prepare statement
pub struct PrepareInfo {
    pub stmt_type: StmtType,
    pub param_count: usize,
    pub truncated: bool,
}

/// Parses the data from the `PrepareStatement` response.
///
/// XSqlDa data format: u8 type + optional data preceded by a u16 length.
/// Returns the statement type, xsqlda and an indicator if the data was truncated (xsqlda not entirely filled)
pub fn parse_xsqlda(resp: &mut Bytes, xsqlda: &mut Vec<XSqlVar>) -> Result<PrepareInfo, FbError> {
    // Asserts that the first 7 bytes are the statement type information
    if resp.remaining() < 7 || resp[..3] != [ibase::isc_info_sql_stmt_type as u8, 0x04, 0x00] {
        return err_invalid_xsqlda();
    }
    resp.advance(3);

    let stmt_type =
        StmtType::try_from(resp.get_u32_le() as u8).map_err(|e| FbError::Other(e.to_string()))?;

    let param_count;

    // Asserts that the next 8 bytes are the start of the parameters data
    if resp.remaining() < 8
        || resp[..2]
            != [
                ibase::isc_info_sql_bind as u8,          // Start of param data
                ibase::isc_info_sql_describe_vars as u8, // Param count
            ]
    {
        return err_invalid_xsqlda();
    }
    resp.advance(2);
    // Parameter count

    // Assume 0x04 0x00
    resp.advance(2);

    param_count = resp.get_u32_le() as usize;

    while resp.remaining() > 0 && resp[0] == ibase::isc_info_sql_describe_end as u8 {
        // Indicates the end of param data, skip it as it appears only once. has one for each param
        resp.advance(1);
    }

    // Asserts that the next 8 bytes are the start of the columns data
    if resp.remaining() < 8
        || resp[..2]
            != [
                ibase::isc_info_sql_select as u8,        // Start of column data
                ibase::isc_info_sql_describe_vars as u8, // Column count
            ]
    {
        return err_invalid_xsqlda();
    }
    resp.advance(2);
    // Column count

    // Assume 0x04 0x00
    resp.advance(2);

    let col_len = resp.get_u32_le() as usize;
    if xsqlda.is_empty() {
        xsqlda.reserve(col_len);
    }

    let truncated = parse_select_items(resp, xsqlda)?;

    Ok(PrepareInfo {
        stmt_type,
        param_count,
        truncated,
    })
}

/// Fill the xsqlda with data from the cursor, return `true` if the data was truncated (needs more data to fill the xsqlda)
pub fn parse_select_items(resp: &mut Bytes, xsqlda: &mut Vec<XSqlVar>) -> Result<bool, FbError> {
    if resp.remaining() == 0 {
        return Ok(false);
    }

    let mut col_index = 0;

    let truncated = loop {
        if resp.remaining() == 0 {
            return err_invalid_xsqlda();
        }
        // Get item code
        match resp.get_u8() as u32 {
            // Column index
            ibase::isc_info_sql_sqlda_seq => {
                if resp.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                resp.advance(2);

                // Index received starts on 1
                col_index = resp.get_u32_le() as usize - 1;

                if col_index >= xsqlda.len() {
                    xsqlda.push(Default::default());
                    // Must be the same
                    debug_assert_eq!(xsqlda.len() - 1, col_index);
                }
            }

            ibase::isc_info_sql_type => {
                if resp.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                resp.advance(2);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.sqltype = resp.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_sub_type => {
                if resp.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                resp.advance(2);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.sqlsubtype = resp.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_scale => {
                if resp.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                resp.advance(2);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.scale = resp.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_length => {
                if resp.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                resp.advance(2);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.data_length = resp.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_null_ind => {
                if resp.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                resp.advance(2);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.null_ind = resp.get_i32_le() != 0;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_field => {
                if resp.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = resp.get_u16_le() as usize;

                if resp.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                resp.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.field_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_relation => {
                if resp.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = resp.get_u16_le() as usize;

                if resp.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                resp.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.relation_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_owner => {
                if resp.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = resp.get_u16_le() as usize;

                if resp.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                resp.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.owner_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            ibase::isc_info_sql_alias => {
                if resp.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = resp.get_u16_le() as usize;

                if resp.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                resp.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(col_index) {
                    var.alias_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            // End of this column data
            ibase::isc_info_sql_describe_end => {}

            // Data truncated
            ibase::isc_info_truncated => break true,

            // End of the data
            ibase::isc_info_end => break false,

            item => {
                return Err(FbError::Other(format!(
                    "Invalid item received in the xsqlda: {}",
                    item
                )));
            }
        }
    };

    Ok(truncated)
}

fn err_invalid_xsqlda<T>() -> Result<T, FbError> {
    Err(FbError::Other(
        "Invalid Xsqlda received from server".to_string(),
    ))
}
