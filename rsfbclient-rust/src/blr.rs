use crate::consts;
use bytes::{BufMut, Bytes, BytesMut};
use rsfbclient_core::{FbError, Param};

/// Maximum parameter data length
const MAX_DATA_LENGTH: usize = 32767;

/// Data for the parameters to send in the wire
pub struct ParamsBlr {
    /// Definitions of the data types
    pub(crate) blr: Bytes,
    /// Actual values of the data
    pub(crate) values: Bytes,
}

/// Convert the parameters to a blr (binary representation)
pub fn params_to_blr(
    params: &[Param],
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
        match p {
            Param::Text(s) => {
                if s.len() > MAX_DATA_LENGTH {
                    return Err("Parameter too big! Not supported yet".into());
                }

                blr.put_u8(consts::blr::TEXT);
                blr.put_u16_le(s.len() as u16);

                values.put_slice(s.as_bytes());
                if s.len() % 4 != 0 {
                    // 4 byte align
                    values.put_slice(&[0; 4][..4 - (s.len() as usize % 4)])
                }
            }
            Param::Integer(i) => {
                blr.put_slice(&[
                    consts::blr::INT64,
                    0, // Scale
                ]);

                values.put_i64(*i);
            }
            Param::Floating(f) => {
                blr.put_u8(consts::blr::DOUBLE);

                values.put_f64(*f);
            }
            Param::Timestamp(ts) => {
                blr.put_u8(consts::blr::TIMESTAMP);

                values.put_i32(ts.timestamp_date);
                values.put_u32(ts.timestamp_time);
            }
            Param::Null => {
                // Represent as empty text
                blr.put_u8(consts::blr::TEXT);
                blr.put_u16_le(0);
            }
        }

        if version < consts::ProtocolVersion::V13 {
            // Null indicator
            values.put_i32_le(if p.is_null() { -1 } else { 0 });
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
fn null_bitmap(values: &mut BytesMut, params: &[Param]) {
    for bitmap in params.chunks(32).map(|params| {
        params
            .iter()
            .fold((0, 0), |(bitmap, i), p| {
                if p.is_null() {
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
