use crate::{client::FirebirdWireConnection, consts};
use bytes::{BufMut, Bytes, BytesMut};
use rsfbclient_core::{FbError, SqlType};

/// Maximum parameter data length
pub const MAX_DATA_LENGTH: usize = 32767;

#[derive(Debug)]
/// Data for the parameters to send in the wire
pub struct ParamsBlr {
    /// Definitions of the data types
    pub(crate) blr: Bytes,
    /// Actual values of the data
    pub(crate) values: Bytes,
}

/// Convert the parameters to a blr (binary representation)
pub fn params_to_blr(
    conn: &mut FirebirdWireConnection,
    tr_handle: &mut crate::TrHandle,
    params: &[SqlType],
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

    if conn.version >= consts::ProtocolVersion::V13 {
        // Insert a null indicator bitmap
        null_bitmap(&mut values, params);
    }

    // Handle blob creation and blr conversion
    let mut handle_blob = |conn: &mut FirebirdWireConnection,
                           blr: &mut BytesMut,
                           values: &mut BytesMut,
                           data: &[u8]| {
        let (blob_handle, id) = conn.create_blob(tr_handle)?;

        conn.put_segments(blob_handle, &data)?;

        conn.close_blob(blob_handle)?;

        blr.put_u8(consts::blr::QUAD);
        blr.put_u8(0); // Blob type

        values.put_u64(id.0);

        Ok::<_, FbError>(())
    };

    for p in params {
        match p {
            SqlType::Text(s) => {
                let bytes = conn.charset.encode(s)?;
                if bytes.len() > MAX_DATA_LENGTH {
                    // Data too large, send as blob
                    handle_blob(conn, &mut blr, &mut values, &bytes)?;
                } else {
                    blr.put_u8(consts::blr::TEXT);
                    blr.put_u16_le(bytes.len() as u16);

                    values.put_slice(&bytes);
                    if bytes.len() % 4 != 0 {
                        // 4 byte align
                        values.put_slice(&[0; 4][..4 - (bytes.len() as usize % 4)])
                    }
                }
            }

            SqlType::Binary(data) => handle_blob(conn, &mut blr, &mut values, &data)?,

            SqlType::Integer(i) => {
                blr.put_slice(&[
                    consts::blr::INT64,
                    0, // Scale
                ]);

                values.put_i64(*i);
            }

            SqlType::Floating(f) => {
                blr.put_u8(consts::blr::DOUBLE);

                values.put_f64(*f);
            }

            SqlType::Timestamp(dt) => {
                blr.put_u8(consts::blr::TIMESTAMP);

                let ts = rsfbclient_core::date_time::encode_timestamp(*dt);
                values.put_i32(ts.timestamp_date);
                values.put_u32(ts.timestamp_time);
            }

            SqlType::Boolean(b) => {
                blr.put_u8(consts::blr::BOOL);

                values.put_slice(if *b { &[1, 0, 0, 0] } else { &[0, 0, 0, 0] });
            }

            SqlType::Null => {
                // Represent as empty text
                blr.put_u8(consts::blr::TEXT);
                blr.put_u16_le(0);
            }
        }

        if conn.version < consts::ProtocolVersion::V13 {
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
fn null_bitmap(values: &mut BytesMut, params: &[SqlType]) {
    for bitmap in params.chunks(32).map(|params| {
        params.iter().enumerate().fold(0, |bitmap, (i, p)| {
            if p.is_null() {
                bitmap | (1 << i)
            } else {
                bitmap
            }
        })
    }) {
        values.put_u32_le(bitmap);
    }
}
