//! Database information (`isc_database_info` / `op_info_database`): the
//! header-page numbers a health check wants — page size, ODS, the four
//! transaction counters, sweep interval, forced writes — plus the live
//! counters (reads, writes, attachments, limbo transactions).
//!
//! The request is a list of item tags; the reply is a stream of
//! `tag, u16 LE length, value` items ending in `isc_info_end`. Integers are
//! little-endian and as wide as the length says (4 bytes, or 8 on 3.0+ when
//! a transaction id no longer fits in 32 bits), so every number is decoded
//! by length.

use crate::ibase;
use chrono::NaiveDateTime;

/// One raw item of a database information reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbInfoItem {
    pub tag: u8,
    pub data: Vec<u8>,
}

/// Split a reply into its items. Stops at `isc_info_end`, `isc_info_truncated`
/// or the first byte that cannot be read.
pub fn parse_db_info(buffer: &[u8]) -> Vec<DbInfoItem> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buffer.len() {
        let tag = buffer[i];
        i += 1;
        if tag as u32 == ibase::isc_info_end || tag as u32 == ibase::isc_info_truncated {
            break;
        }
        let Some(len) = buffer.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize) else { break };
        let end = i + 2 + len;
        if end > buffer.len() {
            break;
        }
        out.push(DbInfoItem { tag, data: buffer[i + 2..end].to_vec() });
        i = end;
    }
    out
}

/// A little-endian integer of 1, 2, 4 or 8 bytes (`isc_portable_integer`).
pub fn int_of(data: &[u8]) -> i64 {
    match data.len() {
        0 => 0,
        1 => data[0] as i8 as i64,
        2 => i16::from_le_bytes([data[0], data[1]]) as i64,
        4 => i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64,
        8 => {
            let mut a = [0u8; 8];
            a.copy_from_slice(data);
            i64::from_le_bytes(a)
        }
        n => {
            // An odd width: take what fits, little-endian.
            let mut a = [0u8; 8];
            a[..n.min(8)].copy_from_slice(&data[..n.min(8)]);
            i64::from_le_bytes(a)
        }
    }
}

/// The items [`DbInfo::parse`] understands, in one request. Every one of
/// them exists on Firebird 2.5; a server that does not know an item answers
/// it with `isc_info_error`, which the parser skips.
pub fn health_items() -> Vec<u8> {
    vec![
        ibase::isc_info_page_size,
        ibase::isc_info_ods_version,
        ibase::isc_info_ods_minor_version,
        ibase::isc_info_oldest_transaction,
        ibase::isc_info_oldest_active,
        ibase::isc_info_oldest_snapshot,
        ibase::isc_info_next_transaction,
        ibase::isc_info_active_tran_count,
        ibase::isc_info_sweep_interval,
        ibase::isc_info_forced_writes,
        ibase::isc_info_no_reserve,
        ibase::isc_info_db_read_only,
        ibase::isc_info_db_sql_dialect,
        ibase::isc_info_set_page_buffers,
        ibase::isc_info_num_buffers,
        ibase::isc_info_allocation,
        ibase::isc_info_db_size_in_pages,
        ibase::isc_info_attachment_id,
        ibase::isc_info_creation_date,
        ibase::isc_info_reads,
        ibase::isc_info_writes,
        ibase::isc_info_fetches,
        ibase::isc_info_marks,
        ibase::isc_info_limbo,
        ibase::isc_info_user_names,
        ibase::isc_info_firebird_version,
        ibase::isc_info_end as u8,
    ]
}

/// The decoded answer to [`health_items`]. Every field is `None` when the
/// server did not answer that item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbInfo {
    pub page_size: Option<u32>,
    pub ods_major: Option<u32>,
    pub ods_minor: Option<u32>,
    pub oldest_transaction: Option<i64>,
    pub oldest_active: Option<i64>,
    pub oldest_snapshot: Option<i64>,
    pub next_transaction: Option<i64>,
    pub active_transactions: Option<i64>,
    pub sweep_interval: Option<u32>,
    pub forced_writes: Option<bool>,
    /// `true` when pages are filled completely (`gfix -use full`).
    pub no_reserve: Option<bool>,
    pub read_only: Option<bool>,
    pub sql_dialect: Option<u32>,
    /// The header's page-buffer count (0 = server default).
    pub page_buffers: Option<u32>,
    /// Cache pages actually in use by this engine.
    pub buffers_in_use: Option<u32>,
    /// Highest allocated page number.
    pub allocation: Option<i64>,
    pub size_in_pages: Option<i64>,
    pub attachment_id: Option<i64>,
    pub creation_date: Option<NaiveDateTime>,
    pub reads: Option<i64>,
    pub writes: Option<i64>,
    pub fetches: Option<i64>,
    pub marks: Option<i64>,
    /// Ids of the transactions in limbo.
    pub limbo: Vec<i64>,
    /// One entry per attachment (the names the engine lets this user see).
    pub user_names: Vec<String>,
    /// Version lines (`Firebird/Linux/AMD/Intel/x64 (access method), version
    /// "LI-V5.0.1.1469 Firebird 5.0"`).
    pub versions: Vec<String>,
}

impl DbInfo {
    /// Decode a reply to [`health_items`].
    pub fn parse(buffer: &[u8]) -> DbInfo {
        let mut info = DbInfo::default();
        for it in parse_db_info(buffer) {
            let d = &it.data;
            match it.tag {
                ibase::isc_info_page_size => info.page_size = Some(int_of(d) as u32),
                ibase::isc_info_ods_version => info.ods_major = Some(int_of(d) as u32),
                ibase::isc_info_ods_minor_version => info.ods_minor = Some(int_of(d) as u32),
                ibase::isc_info_oldest_transaction => info.oldest_transaction = Some(int_of(d)),
                ibase::isc_info_oldest_active => info.oldest_active = Some(int_of(d)),
                ibase::isc_info_oldest_snapshot => info.oldest_snapshot = Some(int_of(d)),
                ibase::isc_info_next_transaction => info.next_transaction = Some(int_of(d)),
                ibase::isc_info_active_tran_count => info.active_transactions = Some(int_of(d)),
                ibase::isc_info_sweep_interval => info.sweep_interval = Some(int_of(d) as u32),
                ibase::isc_info_forced_writes => info.forced_writes = Some(int_of(d) != 0),
                ibase::isc_info_no_reserve => info.no_reserve = Some(int_of(d) != 0),
                ibase::isc_info_db_read_only => info.read_only = Some(int_of(d) != 0),
                ibase::isc_info_db_sql_dialect => info.sql_dialect = Some(int_of(d) as u32),
                ibase::isc_info_set_page_buffers => info.page_buffers = Some(int_of(d) as u32),
                ibase::isc_info_num_buffers => info.buffers_in_use = Some(int_of(d) as u32),
                ibase::isc_info_allocation => info.allocation = Some(int_of(d)),
                ibase::isc_info_db_size_in_pages => info.size_in_pages = Some(int_of(d)),
                ibase::isc_info_attachment_id => info.attachment_id = Some(int_of(d)),
                ibase::isc_info_reads => info.reads = Some(int_of(d)),
                ibase::isc_info_writes => info.writes = Some(int_of(d)),
                ibase::isc_info_fetches => info.fetches = Some(int_of(d)),
                ibase::isc_info_marks => info.marks = Some(int_of(d)),
                ibase::isc_info_limbo => info.limbo.push(int_of(d)),
                ibase::isc_info_creation_date => {
                    if d.len() >= 8 {
                        let date = i32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                        let time = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                        info.creation_date = Some(crate::date_time::decode_timestamp(ibase::ISC_TIMESTAMP {
                            timestamp_date: date,
                            timestamp_time: time,
                        }));
                    }
                }
                ibase::isc_info_user_names => {
                    // The payload is one name: a one-byte length, then the bytes.
                    if let Some(&n) = d.first() {
                        let end = (1 + n as usize).min(d.len());
                        info.user_names.push(String::from_utf8_lossy(&d[1..end]).trim_end().to_string());
                    }
                }
                ibase::isc_info_firebird_version => {
                    // A count byte, then `len, bytes` per line.
                    let mut i = 1usize;
                    let count = d.first().copied().unwrap_or(0) as usize;
                    for _ in 0..count {
                        let Some(&len) = d.get(i) else { break };
                        let end = (i + 1 + len as usize).min(d.len());
                        info.versions.push(String::from_utf8_lossy(&d[i + 1..end]).to_string());
                        i = end;
                    }
                }
                _ => {}
            }
        }
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(tag: u8, data: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend_from_slice(&(data.len() as u16).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn integers_decode_by_width() {
        assert_eq!(int_of(&[1]), 1);
        assert_eq!(int_of(&[0xff]), -1);
        assert_eq!(int_of(&[0x00, 0x20, 0, 0]), 8192);
        assert_eq!(int_of(&[0, 0, 0, 0, 1, 0, 0, 0]), 1 << 32);
        assert_eq!(int_of(&[]), 0);
    }

    #[test]
    fn health_reply_parses_mixed_widths_and_lists() {
        let mut b = Vec::new();
        b.extend(item(ibase::isc_info_page_size, &8192u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_ods_version, &13u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_ods_minor_version, &1u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_oldest_transaction, &100u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_oldest_active, &150u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_oldest_snapshot, &150u32.to_le_bytes()));
        // 3.0+: an id beyond i32 arrives as 8 bytes.
        b.extend(item(ibase::isc_info_next_transaction, &5_000_000_000u64.to_le_bytes()));
        b.extend(item(ibase::isc_info_sweep_interval, &20000u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_forced_writes, &[1]));
        b.extend(item(ibase::isc_info_no_reserve, &[0]));
        b.extend(item(ibase::isc_info_db_read_only, &[0]));
        b.extend(item(ibase::isc_info_limbo, &77u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_limbo, &78u32.to_le_bytes()));
        b.extend(item(ibase::isc_info_user_names, b"\x06SYSDBA"));
        b.extend(item(ibase::isc_info_user_names, b"\x04JOHN"));
        // An item the server did not know.
        b.extend(item(ibase::isc_info_error as u8, &[0, 0, 0, 0]));
        b.extend(item(ibase::isc_info_firebird_version, b"\x01\x05LI-V5"));
        // 2026-10-27 = MJD 61340 since 1858-11-17; 09:01:33.0000 in 1/10000 s.
        let mut cd = 61340i32.to_le_bytes().to_vec();
        cd.extend_from_slice(&((9 * 3600 + 60 + 33) as u32 * 10000).to_le_bytes());
        b.extend(item(ibase::isc_info_creation_date, &cd));
        b.push(ibase::isc_info_end as u8);

        let info = DbInfo::parse(&b);
        assert_eq!(info.page_size, Some(8192));
        assert_eq!((info.ods_major, info.ods_minor), (Some(13), Some(1)));
        assert_eq!(info.oldest_transaction, Some(100));
        assert_eq!(info.next_transaction, Some(5_000_000_000));
        assert_eq!(info.sweep_interval, Some(20000));
        assert_eq!(info.forced_writes, Some(true));
        assert_eq!(info.no_reserve, Some(false));
        assert_eq!(info.read_only, Some(false));
        assert_eq!(info.limbo, vec![77, 78]);
        assert_eq!(info.user_names, vec!["SYSDBA", "JOHN"]);
        assert_eq!(info.versions, vec!["LI-V5"]);
        let cd = info.creation_date.expect("creation date");
        assert_eq!(cd.to_string(), "2026-10-27 09:01:33");
        assert_eq!(info.reads, None, "unasked items stay None");
    }

    #[test]
    fn truncated_and_short_replies_keep_what_is_whole() {
        let mut b = item(ibase::isc_info_page_size, &4096u32.to_le_bytes());
        b.push(ibase::isc_info_truncated as u8);
        b.extend(item(ibase::isc_info_sweep_interval, &1u32.to_le_bytes()));
        let info = DbInfo::parse(&b);
        assert_eq!(info.page_size, Some(4096));
        assert_eq!(info.sweep_interval, None);
        // A length that overruns the buffer ends the parse without a panic.
        assert_eq!(parse_db_info(&[14, 9, 0, 1, 2]), vec![]);
    }

    #[test]
    fn health_items_end_with_the_end_marker() {
        let items = health_items();
        assert_eq!(*items.last().unwrap(), ibase::isc_info_end as u8);
        assert!(items.contains(&ibase::isc_info_next_transaction));
    }
}
