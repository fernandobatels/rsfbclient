//! Charset definitions and functions
//!
//! [Reference](http://www.destructor.de/firebird/charsets.htm)

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
            charset.decode(bytes, DecoderTrap::Strict).map_err(|e| {
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
            fb: self.fb,
            str: self.str,
        }
    }
}

/// The default charset. Works in most cases
pub const UTF_8: Charset = Charset {
    fb: "UTF8",
    str: None, // Will use the std from_utf8
};

/// Western Europe. Latin 1
pub const ISO_8859_1: Charset = Charset {
    fb: "ISO8859_1",
    str: Some(all::ISO_8859_1),
};

/// Central Europe
pub const ISO_8859_2: Charset = Charset {
    fb: "ISO8859_2",
    str: Some(all::ISO_8859_2),
};

/// Southern Europe
pub const ISO_8859_3: Charset = Charset {
    fb: "ISO8859_3",
    str: Some(all::ISO_8859_3),
};

/// North European
pub const ISO_8859_4: Charset = Charset {
    fb: "ISO8859_4",
    str: Some(all::ISO_8859_4),
};

/// Cyrillic
pub const ISO_8859_5: Charset = Charset {
    fb: "ISO8859_5",
    str: Some(all::ISO_8859_5),
};

/// Arabic
pub const ISO_8859_6: Charset = Charset {
    fb: "ISO8859_6",
    str: Some(all::ISO_8859_6),
};

/// Modern Greek
pub const ISO_8859_7: Charset = Charset {
    fb: "ISO8859_7",
    str: Some(all::ISO_8859_7),
};

/// Baltic
pub const ISO_8859_13: Charset = Charset {
    fb: "ISO8859_13",
    str: Some(all::ISO_8859_13),
};

/// Central Europe
pub const WIN_1250: Charset = Charset {
    fb: "WIN1250",
    str: Some(all::WINDOWS_1250),
};

/// Cyrillic
pub const WIN_1251: Charset = Charset {
    fb: "WIN1251",
    str: Some(all::WINDOWS_1251),
};

/// Western Europe, America. Latin-1 with Windows extensions. Brazilian Portuguese
pub const WIN_1252: Charset = Charset {
    fb: "WIN1252",
    str: Some(all::WINDOWS_1252),
};

/// Modern Greek
pub const WIN_1253: Charset = Charset {
    fb: "WIN1253",
    str: Some(all::WINDOWS_1253),
};

/// Turkish
pub const WIN_1254: Charset = Charset {
    fb: "WIN1254",
    str: Some(all::WINDOWS_1254),
};

/// Arabic
pub const WIN_1256: Charset = Charset {
    fb: "WIN1256",
    str: Some(all::WINDOWS_1256),
};

/// Baltic
pub const WIN_1257: Charset = Charset {
    fb: "WIN1257",
    str: Some(all::WINDOWS_1257),
};

/// Vietnamese
pub const WIN_1258: Charset = Charset {
    fb: "WIN1258",
    str: Some(all::WINDOWS_1258),
};

/// English
pub const ASCII: Charset = Charset {
    fb: "ASCII",
    str: Some(all::ASCII),
};

/// Russian
pub const KOI8_R: Charset = Charset {
    fb: "KOI8R",
    str: Some(all::KOI8_R),
};

/// Ukrainian
pub const KOI8_U: Charset = Charset {
    fb: "KOI8U",
    str: Some(all::KOI8_U),
};

/// Japanese
pub const EUC_JP: Charset = Charset {
    fb: "EUCJ_0208",
    str: Some(all::EUC_JP),
};

/// Chinese
pub const BIG5_2003: Charset = Charset {
    fb: "BIG_5",
    str: Some(all::BIG5_2003),
};
