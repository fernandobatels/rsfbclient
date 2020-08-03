//!
//! Rust Firebird Client
//!
//! Wire protocol structs for
//!

#![allow(non_upper_case_globals)]

use bytes::Buf;
use std::{convert::TryFrom, io::Cursor};

use super::*;
use crate::FbError;

/// Data for the statement to return
pub const XSQLDA_DESCRIBE_VARS: [u8; 14] = [
    isc_info_sql_stmt_type as u8,     // Statement type: StmtType
    isc_info_sql_select as u8,        //
    isc_info_sql_describe_vars as u8, // Column count
    isc_info_sql_sqlda_seq as u8,     // Column index
    isc_info_sql_type as u8,          // Sql Type code
    isc_info_sql_sub_type as u8,      // Blob subtype
    isc_info_sql_scale as u8,         // Decimal / Numeric scale
    isc_info_sql_length as u8,        // Data length
    isc_info_sql_null_ind as u8,      // Null indicator (0 or -1)
    isc_info_sql_field as u8,         //
    isc_info_sql_relation as u8,      //
    isc_info_sql_owner as u8,         //
    isc_info_sql_alias as u8,         // Column alias
    isc_info_sql_describe_end as u8,
];

pub type XSqlDa = Vec<XSqlVar>;
#[derive(Debug, Default)]
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

/// XSqlDa data format: u8 type, optional data preceded by a u16 length.
/// Returns the statement type, xsqlda and an indicator if the data was truncated (xsqlda not entirely filled)
pub fn parse_xsqlda(data: &[u8]) -> Result<(StmtType, XSqlDa, bool), FbError> {
    // Asserts that the first
    if data.len() < 7 || data[..3] != [isc_info_sql_stmt_type as u8, 0x04, 0x00] {
        return err_invalid_xsqlda();
    }
    let mut c = Cursor::new(data);
    c.advance(3);

    let stmt_type = StmtType::try_from(c.get_u32_le())?;

    let mut xsqlda = Vec::new();

    let truncated = if c.remaining() >= 4
        && data[c.position() as usize..c.position() as usize + 2]
            == [isc_info_sql_select as u8, isc_info_sql_describe_vars as u8]
    {
        c.advance(2);

        let len = c.get_u16_le() as usize;
        if c.remaining() < len {
            return err_invalid_xsqlda();
        }

        let col_len = c.get_uint_le(len) as usize;
        xsqlda = Vec::with_capacity(col_len);

        parse_select_items(&mut c, &mut xsqlda)?
    } else {
        false
    };

    Ok((stmt_type, xsqlda, truncated))
}

/// Fill the xsqlda with data from the cursor, return `true` if the data was truncated (needs more data to fill the xsqlda)
fn parse_select_items(c: &mut impl Buf, xsqlda: &mut XSqlDa) -> Result<bool, FbError> {
    if c.remaining() == 0 {
        return Ok(false);
    }
    let mut i = 0;

    let truncated = loop {
        // Get item code
        match c.get_u8() as u32 {
            // Column index
            isc_info_sql_sqlda_seq => {
                if c.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                c.advance(2);
                // Index received starts on 1
                i = c.get_u32_le() as usize - 1;

                xsqlda.push(Default::default());
                // Must be the same
                debug_assert_eq!(xsqlda.len() - 1, i);
            }

            isc_info_sql_type => {
                if c.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                c.advance(2);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.sqltype = c.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_sub_type => {
                if c.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                c.advance(2);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.sqlsubtype = c.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_scale => {
                if c.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                c.advance(2);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.scale = c.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_length => {
                if c.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                c.advance(2);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.data_length = c.get_i32_le() as i16;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_null_ind => {
                if c.remaining() < 6 {
                    return err_invalid_xsqlda();
                }
                // Assume 0x04 0x00
                c.advance(2);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.null_ind = c.get_i32_le() != 0;
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_field => {
                if c.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = c.get_u16_le() as usize;

                if c.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                c.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.field_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_relation => {
                if c.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = c.get_u16_le() as usize;

                if c.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                c.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.relation_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_owner => {
                if c.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = c.get_u16_le() as usize;

                if c.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                c.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.owner_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            isc_info_sql_alias => {
                if c.remaining() < 2 {
                    return err_invalid_xsqlda();
                }
                let len = c.get_u16_le() as usize;

                if c.remaining() < len {
                    return err_invalid_xsqlda();
                }
                let mut buff = vec![0; len];
                c.copy_to_slice(&mut buff);

                if let Some(var) = xsqlda.get_mut(i) {
                    var.alias_name = String::from_utf8(buff).unwrap_or_default();
                } else {
                    return err_invalid_xsqlda();
                }
            }

            // Data truncated
            isc_info_truncated => break true,

            // End of this column
            isc_info_sql_describe_end => {}

            // End of the data
            isc_info_end => break false,

            item => {
                return Err(FbError {
                    code: -1,
                    msg: format!("Invalid item received in the xsqlda: {}", item),
                })
            }
        }
    };

    Ok(truncated)
}

fn err_invalid_xsqlda<T>() -> Result<T, FbError> {
    Err(FbError {
        code: -1,
        msg: "Invalid Xsqlda received from server".to_string(),
    })
}
