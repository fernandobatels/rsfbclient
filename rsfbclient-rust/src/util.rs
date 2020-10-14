//! Utility traits and functions

use bytes::Bytes;
use rsfbclient_core::FbError;
use std::convert::TryFrom;

use crate::consts::WireOp;

pub trait BufMutWireExt: bytes::BufMut {
    /// Put a u32 with the bytes length and the byte data
    /// with padding to align for 4 bytes
    fn put_wire_bytes(&mut self, bytes: &[u8])
    where
        Self: Sized,
    {
        let len = bytes.len() as usize;

        self.put_u32(len as u32);
        self.put(bytes);
        if len % 4 != 0 {
            self.put_slice(&[0; 4][..4 - (len % 4)]);
        }
    }
}

impl<T> BufMutWireExt for T where T: bytes::BufMut {}

/// Trait extension to Bytes with methods that do not panic
pub trait BytesWireExt {
    /// Returns the number of bytes between the current position and the end of the buffer
    fn remaining(&self) -> usize;

    /// Advance the internal cursor of the Buf
    fn advance(&mut self, cnt: usize) -> Result<(), FbError>;

    /// Get the length of the bytes from the first u32
    /// and return the bytes read, advancing the cursor
    /// to align to 4 bytes
    fn get_wire_bytes(&mut self) -> Result<Bytes, FbError>;

    /// Gets an unsigned 8 bit integer from `self`
    fn get_u8(&mut self) -> Result<u8, FbError>;

    /// Gets an unsigned 16 bit integer from `self` in the little-endian byte order
    fn get_u16_le(&mut self) -> Result<u16, FbError>;

    /// Gets an unsigned 32 bit integer from `self` in the big-endian byte order
    fn get_u32(&mut self) -> Result<u32, FbError>;

    /// Gets an unsigned 32 bit integer from `self` in the little-endian byte order
    fn get_u32_le(&mut self) -> Result<u32, FbError>;

    /// Gets an signed 32 bit integer from `self` in the big-endian byte order
    fn get_i32(&mut self) -> Result<i32, FbError>;

    /// Gets an signed 32 bit integer from `self` in the little-endian byte order
    fn get_i32_le(&mut self) -> Result<i32, FbError>;

    /// Gets an unsigned 64 bit integer from `self` in the big-endian byte order
    fn get_u64(&mut self) -> Result<u64, FbError>;

    /// Gets an signed 64 bit integer from `self` in the big-endian byte order
    fn get_i64(&mut self) -> Result<i64, FbError>;

    /// Gets an IEEE754 double-precision (8 bytes) floating point number from `self` in big-endian byte order
    fn get_f64(&mut self) -> Result<f64, FbError>;

    fn copy_to_slice(&mut self, dst: &mut [u8]) -> Result<(), FbError>;
}

impl BytesWireExt for Bytes {
    fn remaining(&self) -> usize {
        bytes::Buf::remaining(self)
    }

    fn advance(&mut self, cnt: usize) -> Result<(), FbError> {
        if self.remaining() < cnt {
            return err_invalid_response();
        }

        bytes::Buf::advance(self, cnt);

        Ok(())
    }

    fn get_wire_bytes(&mut self) -> Result<Bytes, FbError> {
        let len = self.get_u32()? as usize;

        if self.remaining() < len {
            return err_invalid_response();
        }
        let bytes = self.slice(..len);

        self.advance(len)?;
        if len % 4 != 0 {
            let pad = 4 - (len % 4);
            if self.remaining() < pad {
                return err_invalid_response();
            }
            self.advance(pad)?;
        }

        Ok(bytes)
    }

    fn get_u8(&mut self) -> Result<u8, FbError> {
        if self.remaining() == 0 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_u8(self))
    }

    fn get_u16_le(&mut self) -> Result<u16, FbError> {
        if self.remaining() < 2 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_u16_le(self))
    }

    fn get_u32(&mut self) -> Result<u32, FbError> {
        if self.remaining() < 4 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_u32(self))
    }

    fn get_u32_le(&mut self) -> Result<u32, FbError> {
        if self.remaining() < 4 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_u32_le(self))
    }

    fn get_i32(&mut self) -> Result<i32, FbError> {
        if self.remaining() < 4 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_i32(self))
    }

    fn get_i32_le(&mut self) -> Result<i32, FbError> {
        if self.remaining() < 4 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_i32_le(self))
    }

    fn get_u64(&mut self) -> Result<u64, FbError> {
        if self.remaining() < 8 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_u64(self))
    }

    fn get_i64(&mut self) -> Result<i64, FbError> {
        if self.remaining() < 8 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_i64(self))
    }

    fn get_f64(&mut self) -> Result<f64, FbError> {
        if self.remaining() < 8 {
            return err_invalid_response();
        }
        Ok(bytes::Buf::get_f64(self))
    }

    fn copy_to_slice(&mut self, dst: &mut [u8]) -> Result<(), FbError> {
        if self.remaining() < dst.len() {
            return err_invalid_response();
        }
        bytes::Buf::copy_to_slice(self, dst);

        Ok(())
    }
}

pub fn err_invalid_response<T>() -> Result<T, FbError> {
    Err("Invalid server response, missing bytes".into())
}

pub fn err_conn_rejected<T>(op_code: u32) -> Result<T, FbError> {
    Err(format!(
        "Connection rejected with code {}{}",
        op_code,
        WireOp::try_from(op_code as u8)
            .map(|op| format!(" ({:?})", op))
            .unwrap_or_default()
    )
    .into())
}
