use crate::ibase::consts;
use crate::{
    params::{ParamInfo, ParamType},
    FbError,
};
use bytes::{BufMut, Bytes, BytesMut};

/// Maximum parameter data length
const MAX_DATA_LENGTH: u16 = 32767;

/// Data for the parameters to send in the wire
pub struct ParamsBlr {
    /// Definitions of the data types
    pub(crate) blr: Bytes,
    /// Actual values of the data
    pub(crate) values: Bytes,
}

/// Convert the parameters to a blr (binary representation)
pub fn params_to_blr(
    params: &[ParamInfo],
    version: consts::ProtocolVersion,
) -> Result<ParamsBlr, FbError> {
    let mut blr = BytesMut::with_capacity(256);
    let mut values = BytesMut::with_capacity(256);

    blr.put_slice(&[
        consts::blr::VERSION5,
        consts::blr::BEGIN,
        consts::blr::MESSAGE,
        0, // Message index
    ]);
    // Message length, * 2 as there is 1 msg for the param type and another for the nullind
    blr.put_u16_le(params.len() as u16 * 2);

    // Insert a null indicator bitmap
    if version >= consts::ProtocolVersion::V13 {
        null_bitmap(&mut values, params);
    }

    for p in params {
        let len = p.buffer.len() as u16;
        if len > MAX_DATA_LENGTH {
            return Err("Parameter too big! Not supported yet".into());
        }

        values.put_slice(&p.buffer);
        if len % 4 != 0 {
            // 4 byte align
            values.put_slice(&[0; 4][..4 - (len as usize % 4)])
        }
        if version < consts::ProtocolVersion::V13 {
            // Null indicator
            values.put_i32_le(if p.null { -1 } else { 0 });
        }

        match p.sqltype {
            ParamType::Text => {
                blr.put_u8(consts::blr::TEXT);
                blr.put_u16_le(len);
            }

            ParamType::Integer => blr.put_slice(&[
                consts::blr::INT64,
                0, // Scale
            ]),

            ParamType::Floating => blr.put_u8(consts::blr::DOUBLE),

            ParamType::Timestamp => blr.put_u8(consts::blr::TIMESTAMP),

            ParamType::Null => {
                // Represent as empty text
                blr.put_u8(consts::blr::TEXT);
                blr.put_u16_le(0);
            }
        }

        // Null indicator type
        blr.put_slice(&[consts::blr::SHORT, 0]);
    }

    blr.put_slice(&[consts::blr::END, consts::blr::EOC]);

    Ok(ParamsBlr {
        blr: blr.freeze(),
        values: values.freeze(),
    })
}

/// Create a null indicator bitmap and insert into the `values`
///
/// The bitmap is a list of bytes,
/// the first bit of the first byte will be 0 if the first value is not null
/// or 1 if it is, and so forth.
///
/// Needs to be aligned to 4 bytes, so processing in chunks of 32 parameters (4 bytes = 32 bits)
fn null_bitmap(values: &mut BytesMut, params: &[ParamInfo]) {
    for bitmap in params.chunks(32).map(|params| {
        params
            .iter()
            .fold((0, 0), |(bitmap, i), p| {
                if p.null {
                    (bitmap | (1 << i), i + 1)
                } else {
                    (bitmap, i + 1)
                }
            })
            .0
    }) {
        values.put_u32_le(bitmap);
    }
}
