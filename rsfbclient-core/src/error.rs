//! Error type for the connection

use std::ops::Range;
use std::str::Utf8Error;
use std::string::FromUtf8Error;
use thiserror::Error;

use crate::SqlType;

#[derive(Debug, Error)]
pub enum FbError {
    #[error("sql error {code}: {msg}")]
    Sql {
        msg: String,
        code: i32,
        diagnostics: Box<SqlDiagnostics>,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("error: {0}")]
    Other(String),
}

/// Machine-readable information from the Firebird status vector.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SqlDiagnostics {
    /// Engine error codes in original order, excluding warnings.
    pub gds_codes: Vec<u32>,
    /// Five-character SQLSTATE, when available.
    pub sql_state: Option<String>,
    /// Byte ranges of engine stack entries in the SQL error message.
    pub psql_contexts: Vec<Range<usize>>,
    /// Errors reported by external statements, including the data source.
    pub external_psql_contexts: Vec<ExternalPsqlContext>,
}

impl SqlDiagnostics {
    /// The first engine error code in the status vector.
    pub fn primary_gds_code(&self) -> Option<u32> {
        self.gds_codes.first().copied()
    }
}

/// The target and detail of a Firebird external statement error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalPsqlContext {
    /// Byte range of the detail in the containing SQL error message.
    pub detail: Range<usize>,
    /// The external data source reported by the engine.
    pub data_source: String,
}

impl ExternalPsqlContext {
    #[doc(hidden)]
    pub fn from_status_entry(
        code: u32,
        formatted: &str,
        arguments: &[String],
        start: usize,
    ) -> Option<Self> {
        let source = match code {
            crate::ibase::isc_eds_statement => arguments.get(3)?,
            crate::ibase::isc_eds_connection => arguments.get(2)?,
            _ => return None,
        };
        let detail = arguments.get(1)?;
        if detail.is_empty() {
            return None;
        }
        let mut occurrences = formatted.match_indices(detail);
        let (offset, _) = occurrences.next()?;
        if occurrences.next().is_some() {
            return None;
        }
        let offset = start + offset;
        Some(Self {
            detail: offset..offset + detail.len(),
            data_source: source.trim().to_owned(),
        })
    }
}

impl From<String> for FbError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

impl From<&str> for FbError {
    fn from(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

impl From<FromUtf8Error> for FbError {
    fn from(e: FromUtf8Error) -> Self {
        Self::Other(format!("Found column with an invalid UTF-8 string: {}", e))
    }
}

impl From<Utf8Error> for FbError {
    fn from(e: Utf8Error) -> Self {
        Self::Other(format!("Found column with an invalid UTF-8 string: {}", e))
    }
}

pub fn err_column_null(type_name: &str) -> FbError {
    FbError::Other(format!(
        "This is a null value. Use the Option<{}> to safe access this column and avoid errors",
        type_name
    ))
}

pub fn err_type_conv<T>(from: SqlType, to: &str) -> Result<T, FbError> {
    Err(FbError::Other(format!(
        "Can't convert {:?} column to {}",
        from, to
    )))
}
