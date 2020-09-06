//! Charset definitions and functions

use encoding::{all, types::EncodingRef, DecoderTrap, EncoderTrap};
use std::str;

use crate::FbError;

/// Charset definition. Used to encode/decode the
/// strings.
pub struct Charset {
    pub fb: &'static str,
    pub str: Option<EncodingRef>,
}

impl Charset {
    /// Decode the bytes using the current charset
    pub fn decode(&self, bytes: &[u8]) -> Result<String, FbError> {
        if let Some(charset) = self.str {
            charset
                .decode(bytes, DecoderTrap::Strict)
                .map(|str| str.to_string())
                .map_err(|e| {
                    format!(
                        "Found column with an invalid {} string: {}",
                        charset.name(),
                        e
                    )
                    .into()
                })
        } else {
            str::from_utf8(bytes)
                .map(|str| str.to_string())
                .map_err(|e| format!("Found column with an invalid UTF-8 string: {}", e).into())
        }
    }

    // Encode the string into bytes using the current charset
    pub fn encode(&self, str: String) -> Result<Vec<u8>, FbError> {
        if let Some(charset) = self.str {
            charset.encode(&str, EncoderTrap::Strict).map_err(|e| {
                format!(
                    "Found param with an invalid {} string: {}",
                    charset.name(),
                    e
                )
                .into()
            })
        } else {
            Ok(str.into_bytes())
        }
    }
}

impl Clone for Charset {
    fn clone(&self) -> Self {
        Self {
            fb: self.fb.clone(),
            str: self.str.clone(),
        }
    }
}

/// The default charset. Works in most cases
pub const UTF_8: Charset = Charset {
    fb: "UTF8",
    str: None, // Will use the std from_utf8
};

pub const ISO_8859_1: Charset = Charset {
    fb: "ISO8859_1",
    str: Some(all::ISO_8859_1),
};

pub const WIN_1252: Charset = Charset {
    fb: "WIN1252",
    str: Some(all::WINDOWS_1252),
};
