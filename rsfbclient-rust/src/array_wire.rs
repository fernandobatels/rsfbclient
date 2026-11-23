//! Typed slice XDR. All field lengths here are native slot lengths, not wire lengths.
use super::*;

pub(super) fn request(
    op: WireOp,
    tr: u32,
    id: ArrayId,
    slice: &ArraySlice,
    values: &[u8],
) -> Result<BytesMut, FbError> {
    let mut packet = BytesMut::new();
    packet.put_u32(op as u32);
    packet.put_u32(tr);
    packet.put_u64(id.as_u64());
    packet.put_i32(slice.byte_len());
    packet.put_wire_bytes(slice.sdl());
    packet.put_u32(0);
    packet.put_u32(values.len() as u32);
    if !values.is_empty() {
        slice.validate(values)?;
    }
    for slot in values.chunks_exact(slice.stride()) {
        match slice.element().blr {
            15 | 23 => opaque(&mut packet, slot),
            41 => {
                let length = slot
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or("Unterminated ARRAY CString")?;
                packet.put_i32(length as i16 as i32);
                opaque(&mut packet, &slot[..length]);
            }
            7 => packet.put_i32(i16::from_le_bytes([slot[0], slot[1]]) as i32),
            35 => {
                reverse(&mut packet, &slot[..4]);
                reverse(&mut packet, &slot[4..8]);
            }
            28 | 29 => {
                let zone = if slice.element().blr == 28 { 4 } else { 8 };
                for part in slot[..zone].chunks_exact(4) {
                    reverse(&mut packet, part);
                }
                packet.put_i32(i16::from_le_bytes([slot[zone], slot[zone + 1]]) as i32);
            }
            _ => reverse(&mut packet, slot),
        }
    }
    Ok(packet)
}

fn opaque(out: &mut BytesMut, value: &[u8]) {
    out.extend_from_slice(value);
    out.extend_from_slice(&[0; 3][..(4 - value.len() % 4) % 4]);
}
fn reverse(out: &mut BytesMut, value: &[u8]) {
    out.extend(value.iter().rev().copied());
}
fn read_opaque(resp: &mut Bytes, out: &mut [u8]) -> Result<(), FbError> {
    resp.copy_to_slice(out)?;
    resp.advance((4 - out.len() % 4) % 4)
}
fn read_reverse(resp: &mut Bytes, out: &mut [u8]) -> Result<(), FbError> {
    resp.copy_to_slice(out)?;
    out.reverse();
    Ok(())
}
fn invalid(message: &str) -> FbError {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

pub(super) fn response(
    resp: &mut Bytes,
    lazy: &mut u32,
    slice: &ArraySlice,
) -> Result<Vec<u8>, FbError> {
    let operation = skip_lazy_responses(resp, lazy)?;
    if operation == WireOp::Response as u32 {
        parse_response(resp)?;
        return Err("Firebird returned no typed ARRAY slice".into());
    }
    if operation != WireOp::Slice as u32 {
        return err_conn_rejected(operation);
    }
    let reported = resp.get_i32()?;
    let encoded = resp.get_i32()?;
    if reported != encoded
        || !(0..=slice.byte_len()).contains(&encoded)
        || encoded as usize % slice.stride() != 0
    {
        return Err(invalid("Invalid Firebird typed ARRAY slice length"));
    }
    let mut values = vec![0; slice.byte_len() as usize];
    for slot in values[..encoded as usize].chunks_exact_mut(slice.stride()) {
        match slice.element().blr {
            15 | 23 => read_opaque(resp, slot)?,
            41 => {
                let length = resp.get_i32()?;
                let short = length as u16;
                if length != short as i16 as i32 || usize::from(short) >= slot.len() {
                    return Err(invalid("Invalid ARRAY CString element length"));
                }
                read_opaque(resp, &mut slot[..usize::from(short)])?;
            }
            7 => {
                let value = resp.get_i32()?;
                let short =
                    i16::try_from(value).map_err(|_| invalid("Invalid ARRAY SMALLINT value"))?;
                slot.copy_from_slice(&short.to_le_bytes());
            }
            35 => {
                read_reverse(resp, &mut slot[..4])?;
                read_reverse(resp, &mut slot[4..8])?;
            }
            28 | 29 => {
                let zone = if slice.element().blr == 28 { 4 } else { 8 };
                for part in slot[..zone].chunks_exact_mut(4) {
                    read_reverse(resp, part)?;
                }
                let value = resp.get_i32()?;
                if value != value as i16 as i32 {
                    return Err(invalid("Invalid ARRAY timezone identifier"));
                }
                slot[zone..zone + 2].copy_from_slice(&(value as u16).to_le_bytes());
            }
            _ => read_reverse(resp, slot)?,
        }
    }
    slice
        .validate(&values)
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(values)
}
