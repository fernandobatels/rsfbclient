//! Backend-agnostic pieces of the Firebird Services API: the option
//! structs, the service parameter blocks (SPB) an action is started with,
//! and the decoding of the information replies. Both the native client
//! (`isc_service_*`) and the pure-rust wire client speak these bytes; only
//! the transport differs.
//!
//! Two different length encodings live in the Services API, which is the
//! classic trap: the *attach* SPB (version 2) uses a one-byte length per
//! argument, while a *start* request uses a two-byte little-endian length
//! for strings and four little-endian bytes for numbers.

use crate::ibase;

/// Options for a `gbak`-style backup. All default to `false`, matching
/// `gbak`'s defaults.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SvcBackupOptions {
    /// Back up metadata only, no table data (`gbak -m`)
    pub metadata_only: bool,
    /// Ignore checksum errors while reading (`gbak -ig`)
    pub ignore_checksums: bool,
    /// Ignore in-limbo transactions (`gbak -l`)
    pub ignore_limbo: bool,
    /// Do not run garbage collection during the backup (`gbak -g`);
    /// commonly enabled for faster backups of busy databases
    pub no_garbage_collect: bool,
}

/// Options for a `gbak`-style restore.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SvcRestoreOptions {
    /// Replace the target database if it already exists (`gbak -rep`).
    /// When `false` (the default) the restore fails if the target
    /// exists — the safe behaviour, same as `gbak -c`.
    pub replace: bool,
    /// Page size of the restored database (`gbak -p`); `None` keeps
    /// the page size recorded in the backup
    pub page_size: Option<u32>,
}

/// Version-2 attach SPB with legacy credentials: version tags, then the
/// user name and plaintext password, each a tag byte, a ONE-byte length
/// and the bytes. This is what the native client hands to
/// `isc_service_attach`; the wire client builds its own SPB because the
/// password travels through the SRP handshake instead.
pub fn attach_spb_legacy(user: &str, pass: &str) -> Vec<u8> {
    let mut spb = vec![
        ibase::isc_spb_version as u8,
        ibase::isc_spb_current_version as u8,
    ];
    push_attach_arg(&mut spb, ibase::isc_spb_user_name as u8, user.as_bytes());
    push_attach_arg(&mut spb, ibase::isc_spb_password as u8, pass.as_bytes());
    spb
}

/// Attach-SPB argument: tag, ONE-byte length, bytes. Values longer than
/// 255 bytes are truncated; nothing the attach block carries is that long.
pub fn push_attach_arg(spb: &mut Vec<u8>, tag: u8, value: &[u8]) {
    let value = &value[..value.len().min(255)];
    spb.push(tag);
    spb.push(value.len() as u8);
    spb.extend_from_slice(value);
}

/// Start-request string argument: tag, TWO-byte little-endian length, bytes.
pub fn push_string_arg(req: &mut Vec<u8>, tag: u8, value: &str) {
    req.push(tag);
    req.extend_from_slice(&(value.len() as u16).to_le_bytes());
    req.extend_from_slice(value.as_bytes());
}

/// Start-request numeric argument: tag + four-byte little-endian value.
pub fn push_u32_arg(req: &mut Vec<u8>, tag: u8, value: u32) {
    req.push(tag);
    req.extend_from_slice(&value.to_le_bytes());
}

/// The start request for a verbose backup of `db_name` (a SERVER path or
/// alias) into `backup_file` (a SERVER path).
pub fn backup_request(db_name: &str, backup_file: &str, options: SvcBackupOptions) -> Vec<u8> {
    let mut flags = 0u32;
    if options.metadata_only {
        flags |= ibase::isc_spb_bkp_metadata_only;
    }
    if options.ignore_checksums {
        flags |= ibase::isc_spb_bkp_ignore_checksums;
    }
    if options.ignore_limbo {
        flags |= ibase::isc_spb_bkp_ignore_limbo;
    }
    if options.no_garbage_collect {
        flags |= ibase::isc_spb_bkp_no_garbage_collect;
    }

    let mut req = vec![ibase::isc_action_svc_backup as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    push_string_arg(&mut req, ibase::isc_spb_bkp_file as u8, backup_file);
    push_u32_arg(&mut req, ibase::isc_spb_options as u8, flags);
    req.push(ibase::isc_spb_verbose as u8);
    req
}

/// The start request for a verbose restore of `backup_file` (a SERVER
/// path) into `db_name` (a SERVER path).
pub fn restore_request(backup_file: &str, db_name: &str, options: SvcRestoreOptions) -> Vec<u8> {
    let flags = if options.replace {
        ibase::isc_spb_res_replace
    } else {
        ibase::isc_spb_res_create
    };

    let mut req = vec![ibase::isc_action_svc_restore as u8];
    push_string_arg(&mut req, ibase::isc_spb_bkp_file as u8, backup_file);
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    push_u32_arg(&mut req, ibase::isc_spb_options as u8, flags);
    if let Some(page_size) = options.page_size {
        push_u32_arg(&mut req, ibase::isc_spb_res_page_size as u8, page_size);
    }
    req.push(ibase::isc_spb_verbose as u8);
    req
}

/// One `isc_info_svc_line` reply: `Some(line)` while the running action
/// still produces output, `None` once it is over (an empty line, an
/// `isc_info_end`, or anything that is not a line).
pub fn parse_svc_line(buffer: &[u8]) -> Option<String> {
    let text = parse_svc_string(buffer, ibase::isc_info_svc_line as u8)?;
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// A string reply tagged `tag`: tag byte, TWO-byte little-endian length,
/// bytes. `None` when the buffer starts with any other tag.
pub fn parse_svc_string(buffer: &[u8], tag: u8) -> Option<String> {
    if buffer.len() < 3 || buffer[0] != tag {
        return None;
    }
    let len = u16::from_le_bytes([buffer[1], buffer[2]]) as usize;
    let end = (3 + len).min(buffer.len());
    Some(String::from_utf8_lossy(&buffer[3..end]).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_spb_is_version2_with_byte_lengths() {
        let spb = attach_spb_legacy("SYSDBA", "masterkey");
        let mut want = vec![2u8, 2, 28, 6];
        want.extend_from_slice(b"SYSDBA");
        want.extend_from_slice(&[29, 9]);
        want.extend_from_slice(b"masterkey");
        assert_eq!(spb, want);
    }

    #[test]
    fn backup_request_layout() {
        let req = backup_request(
            "/d/a.fdb",
            "/d/a.fbk",
            SvcBackupOptions {
                no_garbage_collect: true,
                ..Default::default()
            },
        );
        let mut want = vec![1u8]; // isc_action_svc_backup
        want.extend_from_slice(&[106, 8, 0]);
        want.extend_from_slice(b"/d/a.fdb");
        want.extend_from_slice(&[5, 8, 0]);
        want.extend_from_slice(b"/d/a.fbk");
        want.extend_from_slice(&[108, 8, 0, 0, 0]); // options = no_garbage_collect
        want.push(107); // verbose
        assert_eq!(req, want);
    }

    #[test]
    fn restore_request_create_vs_replace_and_page_size() {
        let create = restore_request("/d/a.fbk", "/d/b.fdb", SvcRestoreOptions::default());
        assert_eq!(create[0], 2);
        let opts_at = 1 + 3 + 8 + 3 + 8;
        assert_eq!(&create[opts_at..opts_at + 5], &[108, 0, 0x20, 0, 0]); // isc_spb_res_create = 0x2000
        assert_eq!(create[opts_at + 5], 107);

        let replace = restore_request(
            "/d/a.fbk",
            "/d/b.fdb",
            SvcRestoreOptions {
                replace: true,
                page_size: Some(16384),
            },
        );
        assert_eq!(&replace[opts_at..opts_at + 5], &[108, 0, 0x10, 0, 0]); // isc_spb_res_replace = 0x1000
        assert_eq!(&replace[opts_at + 5..opts_at + 10], &[10, 0, 0x40, 0, 0]); // page size 16384
        assert_eq!(replace[opts_at + 10], 107);
    }

    #[test]
    fn svc_line_parsing() {
        let mut buf = vec![62u8, 5, 0];
        buf.extend_from_slice(b"hello");
        buf.push(1);
        assert_eq!(parse_svc_line(&buf), Some("hello".to_string()));
        assert_eq!(parse_svc_line(&[62, 0, 0, 1]), None);
        assert_eq!(parse_svc_line(&[1]), None);
        assert_eq!(parse_svc_line(&[]), None);
        // Truncated buffer: take what is there.
        assert_eq!(parse_svc_string(&[55, 9, 0, b'a', b'b'], 55), Some("ab".to_string()));
    }
}
