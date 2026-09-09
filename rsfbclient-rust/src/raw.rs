//! Zero-copy row streaming for the pure-rust backend.
//!
//! [`RawValue`] carries a column value with no owned allocation: text is a
//! `Bytes` handle onto the wire read buffer (an O(1) refcount clone, no copy and
//! no `String`), scalars are plain values. Paired with a reusable scratch
//! `Vec<RawValue>` per fetch, this skips the per-row `Vec<Column>` and the
//! per-text-column `String` that the `Column` API allocates.
//!
//! Blob columns are not supported on this path (they need a deferred round-trip);
//! callers fall back to the `Column` API when the statement has blobs.

use bytes::Bytes;
use chrono::NaiveDateTime;

/// A single column value, borrowed from the wire buffer where possible.
#[derive(Debug, Clone)]
pub enum RawValue {
    Null,
    Integer(i64),
    Int128(i128),
    Floating(f64),
    Boolean(bool),
    Timestamp(NaiveDateTime),
    /// Raw column bytes straight from the wire buffer, in the connection charset
    /// (UTF-8 for a UTF8 connection). A refcounted slice: cloning is O(1) and
    /// there is no `String` allocation nor a copy out of the buffer.
    Text(Bytes),
}

impl RawValue {
    /// Build a `Text` value from a byte slice. Mainly for tests and consumers
    /// that don't hold a wire `Bytes`; the streaming path passes the wire slice
    /// directly.
    pub fn text_bytes(bytes: &[u8]) -> Self {
        RawValue::Text(Bytes::copy_from_slice(bytes))
    }
}
