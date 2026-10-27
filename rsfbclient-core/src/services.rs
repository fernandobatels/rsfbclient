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

// ── Start-request encoders beyond strings and u32 ────────────────────────────

/// Start-request eight-byte argument: tag + u64 little-endian. Only the
/// `*_64` transaction tags (3.0+) use it.
pub fn push_u64_arg(req: &mut Vec<u8>, tag: u8, value: u64) {
    req.push(tag);
    req.extend_from_slice(&value.to_le_bytes());
}

/// Start-request one-byte argument: tag + the byte. The mode selectors
/// (`isc_spb_prp_shutdown_mode`, `write_mode`, `access_mode`,
/// `reserve_space`, `replica_mode`, `online_mode`) are encoded this way.
pub fn push_byte_arg(req: &mut Vec<u8>, tag: u8, value: u8) {
    req.push(tag);
    req.push(value);
}

/// The send-items block that makes one information query return after
/// `seconds` with `isc_info_svc_timeout` instead of blocking on a silent
/// action: tag, two-byte length, the value.
pub fn svc_timeout_send_items(seconds: u16) -> Vec<u8> {
    let mut v = vec![ibase::isc_info_svc_timeout as u8, 2, 0];
    v.extend_from_slice(&seconds.to_le_bytes());
    v
}

// ── gfix: repair / sweep / limbo ─────────────────────────────────────────────

/// What `isc_action_svc_repair` should do — the `gfix` switches.
/// Everything defaults to off; combine as gfix would (`validate` +
/// `full`, `validate` + `check_only`, `sweep` alone, …).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SvcRepairOptions {
    /// `gfix -validate`: walk every page, fix orphans. Exclusive access.
    pub validate: bool,
    /// `gfix -full`: with `validate`, also check record and blob chains.
    pub full: bool,
    /// `gfix -no_update`: report problems without fixing anything.
    pub check_only: bool,
    /// `gfix -ignore`: skip checksum verification.
    pub ignore_checksums: bool,
    /// `gfix -mend`: mark damaged records so a backup can skip them. May
    /// lose data; implies `validate` + `full` on the server.
    pub mend: bool,
    /// `gfix -sweep`: run a sweep now (online).
    pub sweep: bool,
    /// `gfix -list`: list limbo transactions (read with
    /// `isc_info_svc_limbo_trans`).
    pub list_limbo: bool,
    /// `gfix -kill`: remove references to unavailable shadow files.
    pub kill_shadows: bool,
    /// `gfix -icu` (3.0+): rebuild indices that depend on ICU collations.
    pub icu: bool,
    /// `gfix -upgrade` (5.0+): upgrade the ODS minor version in place.
    pub upgrade: bool,
    /// `gfix -parallel n` (5.0+): workers for sweep / icu.
    pub parallel_workers: Option<u32>,
}

impl SvcRepairOptions {
    fn flags(&self) -> u32 {
        let mut f = 0u32;
        if self.validate {
            f |= ibase::isc_spb_rpr_validate_db;
        }
        if self.full {
            f |= ibase::isc_spb_rpr_full;
        }
        if self.check_only {
            f |= ibase::isc_spb_rpr_check_db;
        }
        if self.ignore_checksums {
            f |= ibase::isc_spb_rpr_ignore_checksum;
        }
        if self.mend {
            f |= ibase::isc_spb_rpr_mend_db;
        }
        if self.sweep {
            f |= ibase::isc_spb_rpr_sweep_db;
        }
        if self.list_limbo {
            f |= ibase::isc_spb_rpr_list_limbo_trans;
        }
        if self.kill_shadows {
            f |= ibase::isc_spb_rpr_kill_shadows;
        }
        if self.icu {
            f |= ibase::isc_spb_rpr_icu;
        }
        if self.upgrade {
            f |= ibase::isc_spb_rpr_upgrade_db;
        }
        f
    }
}

/// The start request for `isc_action_svc_repair` on `db_name` (a SERVER
/// path or alias) with `options`.
pub fn repair_request(db_name: &str, options: SvcRepairOptions) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_repair as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    push_u32_arg(&mut req, ibase::isc_spb_options as u8, options.flags());
    if let Some(n) = options.parallel_workers {
        push_u32_arg(&mut req, ibase::isc_spb_rpr_par_workers as u8, n);
    }
    req
}

/// `gfix -sweep`.
pub fn sweep_request(db_name: &str, parallel_workers: Option<u32>) -> Vec<u8> {
    repair_request(
        db_name,
        SvcRepairOptions {
            sweep: true,
            parallel_workers,
            ..Default::default()
        },
    )
}

/// `gfix -list`: start this, then drain `isc_info_svc_limbo_trans`
/// replies with [`parse_limbo_list`].
pub fn limbo_list_request(db_name: &str) -> Vec<u8> {
    repair_request(
        db_name,
        SvcRepairOptions {
            list_limbo: true,
            ..Default::default()
        },
    )
}

/// How to resolve one limbo transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimboAction {
    /// `gfix -commit id`
    Commit,
    /// `gfix -rollback id`
    Rollback,
    /// `gfix -two_phase id`: let the engine decide from the participants'
    /// states — commit if any of them committed, else roll back.
    TwoPhase,
}

/// The start request resolving limbo transaction `id` one way. Ids that
/// do not fit in a signed 32-bit integer use the `_64` tags (3.0+).
pub fn limbo_resolve_request(db_name: &str, id: u64, action: LimboAction) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_repair as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    let (tag32, tag64) = match action {
        LimboAction::Commit => (ibase::isc_spb_rpr_commit_trans, ibase::isc_spb_rpr_commit_trans_64),
        LimboAction::Rollback => (ibase::isc_spb_rpr_rollback_trans, ibase::isc_spb_rpr_rollback_trans_64),
        LimboAction::TwoPhase => (ibase::isc_spb_rpr_recover_two_phase, ibase::isc_spb_rpr_recover_two_phase_64),
    };
    if id <= i32::MAX as u64 {
        push_u32_arg(&mut req, tag32 as u8, id as u32);
    } else {
        push_u64_arg(&mut req, tag64 as u8, id);
    }
    req
}

/// State of one participant of a limbo transaction, as the engine's
/// transaction description record reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimboState {
    Limbo,
    Committed,
    RolledBack,
    Unknown,
}

/// What the engine would do for the transaction under `gfix -two_phase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimboAdvice {
    Commit,
    Rollback,
    Unknown,
}

/// One database taking part in a multi-database limbo transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimboParticipant {
    pub id: u64,
    pub state: LimboState,
    pub remote_site: Option<String>,
    pub db_path: Option<String>,
}

/// One limbo transaction from `gfix -list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimboTransaction {
    pub id: u64,
    /// `true` when a transaction description record names other
    /// databases (`isc_spb_multi_tra_id`); `false` for a transaction
    /// prepared in this database alone.
    pub multi_database: bool,
    pub host_site: Option<String>,
    pub participants: Vec<LimboParticipant>,
    pub advice: Option<LimboAdvice>,
}

fn take_u32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn take_u64(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8).map(|s| {
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        u64::from_le_bytes(a)
    })
}

fn take_string(b: &[u8], at: usize) -> Option<(String, usize)> {
    let len = b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)?;
    let text = b.get(at + 2..at + 2 + len)?;
    Some((String::from_utf8_lossy(text).into_owned(), at + 2 + len))
}

/// Decode the payload of one or more `isc_info_svc_limbo_trans` replies
/// (concatenated, without the item tag and length). Tolerates the 32-bit
/// tags of 2.5 and the `_64` ones of 3.0+ side by side. Stops quietly at
/// the first byte it does not understand, keeping what it has.
pub fn parse_limbo_list(payload: &[u8]) -> Vec<LimboTransaction> {
    let mut out: Vec<LimboTransaction> = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let tag = payload[i] as u32;
        i += 1;
        match tag {
            t if t == ibase::isc_spb_single_tra_id || t == ibase::isc_spb_multi_tra_id || t == ibase::isc_spb_single_tra_id_64 || t == ibase::isc_spb_multi_tra_id_64 => {
                let wide = t == ibase::isc_spb_single_tra_id_64 || t == ibase::isc_spb_multi_tra_id_64;
                let id = if wide {
                    match take_u64(payload, i) {
                        Some(v) => {
                            i += 8;
                            v
                        }
                        None => break,
                    }
                } else {
                    match take_u32(payload, i) {
                        Some(v) => {
                            i += 4;
                            v as u64
                        }
                        None => break,
                    }
                };
                out.push(LimboTransaction {
                    id,
                    multi_database: t == ibase::isc_spb_multi_tra_id || t == ibase::isc_spb_multi_tra_id_64,
                    host_site: None,
                    participants: Vec::new(),
                    advice: None,
                });
            }
            t if t == ibase::isc_spb_tra_id || t == ibase::isc_spb_tra_id_64 => {
                let id = if t == ibase::isc_spb_tra_id_64 {
                    match take_u64(payload, i) {
                        Some(v) => {
                            i += 8;
                            v
                        }
                        None => break,
                    }
                } else {
                    match take_u32(payload, i) {
                        Some(v) => {
                            i += 4;
                            v as u64
                        }
                        None => break,
                    }
                };
                match out.last_mut() {
                    Some(tx) => tx.participants.push(LimboParticipant { id, state: LimboState::Unknown, remote_site: None, db_path: None }),
                    None => break,
                }
            }
            t if t == ibase::isc_spb_tra_state => {
                let Some(&s) = payload.get(i) else { break };
                i += 1;
                let state = match s as u32 {
                    x if x == ibase::isc_spb_tra_state_limbo => LimboState::Limbo,
                    x if x == ibase::isc_spb_tra_state_commit => LimboState::Committed,
                    x if x == ibase::isc_spb_tra_state_rollback => LimboState::RolledBack,
                    _ => LimboState::Unknown,
                };
                if let Some(p) = out.last_mut().and_then(|tx| tx.participants.last_mut()) {
                    p.state = state;
                }
            }
            t if t == ibase::isc_spb_tra_host_site || t == ibase::isc_spb_tra_remote_site || t == ibase::isc_spb_tra_db_path => {
                let Some((text, next)) = take_string(payload, i) else { break };
                i = next;
                let Some(tx) = out.last_mut() else { break };
                if t == ibase::isc_spb_tra_host_site {
                    tx.host_site = Some(text);
                } else if let Some(p) = tx.participants.last_mut() {
                    if t == ibase::isc_spb_tra_remote_site {
                        p.remote_site = Some(text);
                    } else {
                        p.db_path = Some(text);
                    }
                }
            }
            t if t == ibase::isc_spb_tra_advise => {
                let Some(&a) = payload.get(i) else { break };
                i += 1;
                let advice = match a as u32 {
                    x if x == ibase::isc_spb_tra_advise_commit => LimboAdvice::Commit,
                    x if x == ibase::isc_spb_tra_advise_rollback => LimboAdvice::Rollback,
                    _ => LimboAdvice::Unknown,
                };
                if let Some(tx) = out.last_mut() {
                    tx.advice = Some(advice);
                }
            }
            _ => break,
        }
    }
    out
}

// ── gfix: database properties, shutdown, online ──────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcWriteMode {
    /// `gfix -write sync`: forced writes on.
    Sync,
    /// `gfix -write async`.
    Async,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcAccessMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcReserveSpace {
    /// `gfix -use reserve`: keep ~20% of each page for versions (default).
    Reserve,
    /// `gfix -use full`: fill pages completely.
    UseFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcReplicaMode {
    None,
    ReadOnly,
    ReadWrite,
}

/// Properties `isc_action_svc_properties` can change in one request.
/// `None` / `false` leaves a property alone.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct SvcProperties {
    /// `gfix -buffers n` (0 = use the server's default).
    pub page_buffers: Option<u32>,
    /// `gfix -housekeeping n` (0 = no automatic sweep).
    pub sweep_interval: Option<u32>,
    /// `gfix -sql_dialect 1|3`.
    pub sql_dialect: Option<u32>,
    pub write_mode: Option<SvcWriteMode>,
    pub access_mode: Option<SvcAccessMode>,
    pub reserve_space: Option<SvcReserveSpace>,
    /// `gfix -replica …` (4.0+).
    pub replica_mode: Option<SvcReplicaMode>,
    /// `gfix -nolinger` (3.0+): close the file at the next last detach.
    pub nolinger: bool,
    /// `gfix -activate`: activate the shadow (primary must be offline).
    pub activate_shadow: bool,
}

/// The start request applying `props` to `db_name`.
pub fn properties_request(db_name: &str, props: &SvcProperties) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_properties as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    if let Some(n) = props.page_buffers {
        push_u32_arg(&mut req, ibase::isc_spb_prp_page_buffers as u8, n);
    }
    if let Some(n) = props.sweep_interval {
        push_u32_arg(&mut req, ibase::isc_spb_prp_sweep_interval as u8, n);
    }
    if let Some(n) = props.sql_dialect {
        push_u32_arg(&mut req, ibase::isc_spb_prp_set_sql_dialect as u8, n);
    }
    if let Some(m) = props.write_mode {
        let v = match m {
            SvcWriteMode::Sync => ibase::isc_spb_prp_wm_sync,
            SvcWriteMode::Async => ibase::isc_spb_prp_wm_async,
        };
        push_byte_arg(&mut req, ibase::isc_spb_prp_write_mode as u8, v as u8);
    }
    if let Some(m) = props.access_mode {
        let v = match m {
            SvcAccessMode::ReadOnly => ibase::isc_spb_prp_am_readonly,
            SvcAccessMode::ReadWrite => ibase::isc_spb_prp_am_readwrite,
        };
        push_byte_arg(&mut req, ibase::isc_spb_prp_access_mode as u8, v as u8);
    }
    if let Some(m) = props.reserve_space {
        let v = match m {
            SvcReserveSpace::Reserve => ibase::isc_spb_prp_res,
            SvcReserveSpace::UseFull => ibase::isc_spb_prp_res_use_full,
        };
        push_byte_arg(&mut req, ibase::isc_spb_prp_reserve_space as u8, v as u8);
    }
    if let Some(m) = props.replica_mode {
        let v = match m {
            SvcReplicaMode::None => ibase::isc_spb_prp_rm_none,
            SvcReplicaMode::ReadOnly => ibase::isc_spb_prp_rm_readonly,
            SvcReplicaMode::ReadWrite => ibase::isc_spb_prp_rm_readwrite,
        };
        push_byte_arg(&mut req, ibase::isc_spb_prp_replica_mode as u8, v as u8);
    }
    let mut flags = 0u32;
    if props.nolinger {
        flags |= ibase::isc_spb_prp_nolinger;
    }
    if props.activate_shadow {
        flags |= ibase::isc_spb_prp_activate;
    }
    if flags != 0 {
        push_u32_arg(&mut req, ibase::isc_spb_options as u8, flags);
    }
    req
}

/// Shutdown / online levels, strictest last. A database can only be shut
/// *down* to a stricter level and brought *online* to a looser one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcShutdownMode {
    /// Fully online.
    Normal,
    /// `multi`: only SYSDBA / owner may attach, any number of times.
    Multi,
    /// `single`: exactly one SYSDBA / owner attachment.
    Single,
    /// `full`: nobody, not even SYSDBA (file copy, mend, ODS upgrade).
    Full,
}

impl SvcShutdownMode {
    fn byte(self) -> u8 {
        (match self {
            SvcShutdownMode::Normal => ibase::isc_spb_prp_sm_normal,
            SvcShutdownMode::Multi => ibase::isc_spb_prp_sm_multi,
            SvcShutdownMode::Single => ibase::isc_spb_prp_sm_single,
            SvcShutdownMode::Full => ibase::isc_spb_prp_sm_full,
        }) as u8
    }
}

/// How a shutdown treats the people connected while the timeout runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcShutdownKind {
    /// `-force n`: after n seconds every attachment is dropped and its
    /// transactions rolled back.
    Force,
    /// `-attach n`: refuse new attachments; fail if some remain after n s.
    DenyAttachments,
    /// `-tran n`: refuse new transactions; fail if some remain after n s.
    DenyTransactions,
}

/// `gfix -shut <mode> -force|-attach|-tran <seconds>`. The timeout must
/// be 0–32767.
pub fn shutdown_request(db_name: &str, mode: SvcShutdownMode, kind: SvcShutdownKind, timeout_seconds: u32) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_properties as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    push_byte_arg(&mut req, ibase::isc_spb_prp_shutdown_mode as u8, mode.byte());
    let tag = match kind {
        SvcShutdownKind::Force => ibase::isc_spb_prp_force_shutdown,
        SvcShutdownKind::DenyAttachments => ibase::isc_spb_prp_attachments_shutdown,
        SvcShutdownKind::DenyTransactions => ibase::isc_spb_prp_transactions_shutdown,
    };
    push_u32_arg(&mut req, tag as u8, timeout_seconds.min(32767));
    req
}

/// `gfix -online <mode>`.
pub fn online_request(db_name: &str, mode: SvcShutdownMode) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_properties as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    push_byte_arg(&mut req, ibase::isc_spb_prp_online_mode as u8, mode.byte());
    req
}

// ── gstat ────────────────────────────────────────────────────────────────────

/// Which parts of a `gstat` report to produce.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct SvcStatsOptions {
    /// `-h`: header page only (cheap, works on a live database).
    pub header: bool,
    /// `-d`: data pages per table.
    pub data_pages: bool,
    /// `-i`: index pages.
    pub index_pages: bool,
    /// `-r`: record and version lengths (implies data pages).
    pub record_versions: bool,
    /// `-s`: system tables too.
    pub system_relations: bool,
    /// `-e` (3.0+): encryption statistics.
    pub encryption: bool,
    /// Hide creation date and GUID.
    pub no_creation: bool,
    /// `-t NAME …`: restrict to these tables (exact, case-sensitive names).
    pub tables: Vec<String>,
    /// 2.5 does not know `isc_spb_sts_table` as a string tag: the names
    /// travel in `isc_spb_command_line` instead.
    pub tables_on_command_line: bool,
}

/// The start request for `isc_action_svc_db_stats`.
pub fn db_stats_request(db_name: &str, options: &SvcStatsOptions) -> Vec<u8> {
    let mut flags = 0u32;
    if options.header {
        flags |= ibase::isc_spb_sts_hdr_pages;
    }
    if options.data_pages {
        flags |= ibase::isc_spb_sts_data_pages;
    }
    if options.index_pages {
        flags |= ibase::isc_spb_sts_idx_pages;
    }
    if options.record_versions {
        flags |= ibase::isc_spb_sts_record_versions;
    }
    if options.system_relations {
        flags |= ibase::isc_spb_sts_sys_relations;
    }
    if options.encryption {
        flags |= ibase::isc_spb_sts_encryption;
    }
    if options.no_creation {
        flags |= ibase::isc_spb_sts_nocreation;
    }
    let mut req = vec![ibase::isc_action_svc_db_stats as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    push_u32_arg(&mut req, ibase::isc_spb_options as u8, flags);
    if !options.tables.is_empty() {
        if options.tables_on_command_line {
            let line = options.tables.iter().map(|t| format!("-t {t}")).collect::<Vec<_>>().join(" ");
            push_string_arg(&mut req, ibase::isc_spb_command_line as u8, &line);
        } else {
            for t in &options.tables {
                push_string_arg(&mut req, ibase::isc_spb_sts_table as u8, t);
            }
        }
    }
    req
}

// ── online validation (2.5.4+ / 3.0+) ────────────────────────────────────────

/// `gfix -validate -tab … -idx … -lock_timeout n`: table-by-table checks
/// on a live database. Patterns use `SIMILAR TO` syntax against the
/// stored (case-sensitive) names.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct SvcValidateOptions {
    pub tables_include: Option<String>,
    pub tables_exclude: Option<String>,
    pub indices_include: Option<String>,
    pub indices_exclude: Option<String>,
    /// Seconds to wait for a table lock; 0 = don't wait. Server default 10.
    pub lock_timeout: Option<u32>,
}

/// The start request for `isc_action_svc_validate`.
pub fn validate_request(db_name: &str, options: &SvcValidateOptions) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_validate as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    if let Some(p) = &options.tables_include {
        push_string_arg(&mut req, ibase::isc_spb_val_tab_incl as u8, p);
    }
    if let Some(p) = &options.tables_exclude {
        push_string_arg(&mut req, ibase::isc_spb_val_tab_excl as u8, p);
    }
    if let Some(p) = &options.indices_include {
        push_string_arg(&mut req, ibase::isc_spb_val_idx_incl as u8, p);
    }
    if let Some(p) = &options.indices_exclude {
        push_string_arg(&mut req, ibase::isc_spb_val_idx_excl as u8, p);
    }
    if let Some(t) = options.lock_timeout {
        push_u32_arg(&mut req, ibase::isc_spb_val_lock_timeout as u8, t);
    }
    req
}

// ── server log, users ────────────────────────────────────────────────────────

/// `isc_action_svc_get_fb_log`: the server's `firebird.log`, read with
/// `isc_info_svc_to_eof`. SYSDBA only.
pub fn fb_log_request() -> Vec<u8> {
    vec![ibase::isc_action_svc_get_fb_log as u8]
}

/// `isc_action_svc_display_user`: every user, or just `user`, from the
/// security database that governs `db_name` (3.0+; ignored by 2.5).
/// Read with `isc_info_svc_get_users` and [`parse_svc_users`].
pub fn display_users_request(db_name: Option<&str>, user: Option<&str>) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_display_user as u8];
    if let Some(u) = user {
        push_string_arg(&mut req, ibase::isc_spb_sec_username as u8, u);
    }
    if let Some(db) = db_name {
        push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db);
    }
    req
}

/// One user from the security database.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct SvcUser {
    pub name: String,
    pub first_name: String,
    pub middle_name: String,
    pub last_name: String,
    pub user_id: Option<u32>,
    pub group_id: Option<u32>,
    /// 3.0+ (`display_user_adm`): has the ADMIN role in the security DB.
    pub admin: Option<bool>,
}

/// Decode the payload of `isc_info_svc_get_users` (without the item tag
/// and length): a stream of `isc_spb_sec_*` items, a new user at each
/// `isc_spb_sec_username`.
pub fn parse_svc_users(payload: &[u8]) -> Vec<SvcUser> {
    let mut out: Vec<SvcUser> = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let tag = payload[i] as u32;
        i += 1;
        match tag {
            t if t == ibase::isc_spb_sec_username => {
                let Some((s, next)) = take_string(payload, i) else { break };
                i = next;
                out.push(SvcUser { name: s, ..Default::default() });
            }
            t if t == ibase::isc_spb_sec_firstname || t == ibase::isc_spb_sec_middlename || t == ibase::isc_spb_sec_lastname => {
                let Some((s, next)) = take_string(payload, i) else { break };
                i = next;
                if let Some(u) = out.last_mut() {
                    if t == ibase::isc_spb_sec_firstname {
                        u.first_name = s;
                    } else if t == ibase::isc_spb_sec_middlename {
                        u.middle_name = s;
                    } else {
                        u.last_name = s;
                    }
                }
            }
            t if t == ibase::isc_spb_sec_userid || t == ibase::isc_spb_sec_groupid || t == ibase::isc_spb_sec_admin => {
                let Some(v) = take_u32(payload, i) else { break };
                i += 4;
                if let Some(u) = out.last_mut() {
                    if t == ibase::isc_spb_sec_userid {
                        u.user_id = Some(v);
                    } else if t == ibase::isc_spb_sec_groupid {
                        u.group_id = Some(v);
                    } else {
                        u.admin = Some(v != 0);
                    }
                }
            }
            _ => break,
        }
    }
    out
}

// ── nbackup ──────────────────────────────────────────────────────────────────

/// History pruning for `nbackup -B` (4.0.3+): `-CLEAN_HISTORY -KEEP …`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvcNbakKeep {
    Days(u32),
    Rows(u32),
}

/// `nbackup -B`: a level or GUID based physical backup, on the server.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct SvcNbakOptions {
    /// Backup level (0 = full). Exclusive with `guid`.
    pub level: Option<u32>,
    /// 4.0+: back up the pages changed since the backup with this GUID.
    pub guid: Option<String>,
    /// The backup file to write (a SERVER path).
    pub file: String,
    /// `-T`: do not fire database triggers.
    pub no_triggers: bool,
    /// `-D on|off`: direct I/O.
    pub direct: Option<bool>,
    /// 4.0.3+: prune RDB$BACKUP_HISTORY after the backup.
    pub clean_history: Option<SvcNbakKeep>,
}

/// The start request for `isc_action_svc_nbak`.
pub fn nbak_request(db_name: &str, options: &SvcNbakOptions) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_nbak as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    if let Some(g) = &options.guid {
        push_string_arg(&mut req, ibase::isc_spb_nbk_guid as u8, g);
    } else {
        push_u32_arg(&mut req, ibase::isc_spb_nbk_level as u8, options.level.unwrap_or(0));
    }
    push_string_arg(&mut req, ibase::isc_spb_nbk_file as u8, &options.file);
    if options.no_triggers {
        push_u32_arg(&mut req, ibase::isc_spb_options as u8, ibase::isc_spb_nbk_no_triggers);
    }
    if let Some(d) = options.direct {
        push_string_arg(&mut req, ibase::isc_spb_nbk_direct as u8, if d { "on" } else { "off" });
    }
    if let Some(keep) = options.clean_history {
        req.push(ibase::isc_spb_nbk_clean_history as u8);
        match keep {
            SvcNbakKeep::Days(n) => push_u32_arg(&mut req, ibase::isc_spb_nbk_keep_days as u8, n),
            SvcNbakKeep::Rows(n) => push_u32_arg(&mut req, ibase::isc_spb_nbk_keep_rows as u8, n),
        }
    }
    req
}

/// `nbackup -R`: restore the chain `files` (level 0 first) into `db_name`.
/// `inplace` (4.0+) restores into an existing database; `sequence` keeps
/// the replication sequence.
pub fn nrest_request(db_name: &str, files: &[&str], inplace: bool, sequence: bool) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_nrest as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    for f in files {
        push_string_arg(&mut req, ibase::isc_spb_nbk_file as u8, f);
    }
    let mut flags = 0u32;
    if inplace {
        flags |= ibase::isc_spb_nbk_inplace;
    }
    if sequence {
        flags |= ibase::isc_spb_nbk_sequence;
    }
    if flags != 0 {
        push_u32_arg(&mut req, ibase::isc_spb_options as u8, flags);
    }
    req
}

/// `nbackup -F` (4.0+): clear the backup-lock flag WITHOUT merging the
/// delta — right for a file-system copy, data loss on the live file.
pub fn nfix_request(db_name: &str, sequence: bool) -> Vec<u8> {
    let mut req = vec![ibase::isc_action_svc_nfix as u8];
    push_string_arg(&mut req, ibase::isc_spb_dbname as u8, db_name);
    if sequence {
        push_u32_arg(&mut req, ibase::isc_spb_options as u8, ibase::isc_spb_nbk_sequence);
    }
    req
}

// ── Information replies ──────────────────────────────────────────────────────

/// One decoded item of a service information reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvcInfoItem {
    /// `isc_info_svc_line`, `server_version`, `implementation`,
    /// `get_env*`, `user_dbpath`: a length-prefixed string.
    Text { tag: u8, text: String },
    /// `isc_info_svc_version`, `capabilities`, `running`: four bytes, no
    /// length prefix.
    Number { tag: u8, value: u32 },
    /// `isc_info_svc_limbo_trans`, `get_users`: a length-prefixed nested
    /// stream (see [`parse_limbo_list`], [`parse_svc_users`]); and
    /// `isc_info_svc_to_eof`, whose bytes are kept raw (a server log is
    /// not always UTF-8).
    Bytes { tag: u8, data: Vec<u8> },
    /// `isc_info_svc_svr_db_info`: attachments, databases, names.
    ServerDbInfo { attachments: u32, databases: u32, names: Vec<String> },
    /// The action's output is longer than the buffer: ask again.
    Truncated,
    /// The per-query timeout elapsed before the action produced anything.
    Timeout,
    /// `isc_info_data_not_ready`.
    NotReady,
    /// `isc_info_end`.
    End,
}

/// Decode a whole service information reply buffer into its items,
/// stopping at `isc_info_end` or at the first byte it cannot read.
pub fn parse_svc_items(buffer: &[u8]) -> Vec<SvcInfoItem> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buffer.len() {
        let tag = buffer[i];
        i += 1;
        match tag as u32 {
            t if t == ibase::isc_info_end => {
                out.push(SvcInfoItem::End);
                break;
            }
            t if t == ibase::isc_info_truncated => out.push(SvcInfoItem::Truncated),
            t if t == ibase::isc_info_svc_timeout => out.push(SvcInfoItem::Timeout),
            t if t == ibase::isc_info_data_not_ready => out.push(SvcInfoItem::NotReady),
            t if t == ibase::isc_info_svc_version || t == ibase::isc_info_svc_capabilities || t == ibase::isc_info_svc_running => {
                let Some(v) = take_u32(buffer, i) else { break };
                i += 4;
                out.push(SvcInfoItem::Number { tag, value: v });
            }
            t if t == ibase::isc_info_svc_limbo_trans || t == ibase::isc_info_svc_get_users || t == ibase::isc_info_svc_to_eof => {
                let Some(len) = buffer.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize) else { break };
                let end = (i + 2 + len).min(buffer.len());
                out.push(SvcInfoItem::Bytes { tag, data: buffer[i + 2..end].to_vec() });
                i = end;
            }
            t if t == ibase::isc_info_svc_svr_db_info => {
                let mut attachments = 0u32;
                let mut databases = 0u32;
                let mut names = Vec::new();
                loop {
                    let Some(&sub) = buffer.get(i) else { break };
                    i += 1;
                    match sub as u32 {
                        s if s == ibase::isc_spb_num_att => {
                            let Some(v) = take_u32(buffer, i) else { break };
                            i += 4;
                            attachments = v;
                        }
                        s if s == ibase::isc_spb_num_db => {
                            let Some(v) = take_u32(buffer, i) else { break };
                            i += 4;
                            databases = v;
                        }
                        s if s == ibase::isc_spb_dbname => {
                            let Some((name, next)) = take_string(buffer, i) else { break };
                            i = next;
                            names.push(name);
                        }
                        _ => break, // isc_info_flag_end, or anything unexpected
                    }
                }
                out.push(SvcInfoItem::ServerDbInfo { attachments, databases, names });
            }
            _ => {
                // Every other item the server answers with is a string.
                let Some((text, next)) = take_string(buffer, i) else { break };
                i = next;
                out.push(SvcInfoItem::Text { tag, text });
            }
        }
    }
    out
}

/// What one `isc_info_svc_line` query came back with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvcLineReply {
    /// A line of the running action's output.
    Line(String),
    /// Nothing within the timeout; the action is still running.
    Timeout,
    /// The action is over (an empty line).
    End,
}

/// Interpret a reply to a single `isc_info_svc_line` request, possibly
/// made with [`svc_timeout_send_items`].
pub fn parse_svc_line_reply(buffer: &[u8]) -> SvcLineReply {
    let items = parse_svc_items(buffer);
    for it in &items {
        match it {
            SvcInfoItem::Text { tag, text } if *tag == ibase::isc_info_svc_line as u8 => {
                return if text.is_empty() { SvcLineReply::End } else { SvcLineReply::Line(text.clone()) };
            }
            SvcInfoItem::Timeout => return SvcLineReply::Timeout,
            _ => {}
        }
    }
    SvcLineReply::End
}

/// One `isc_info_svc_to_eof` reply: the bytes, and whether more are
/// coming (`isc_info_truncated` — the action is still writing).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SvcChunk {
    pub data: Vec<u8>,
    pub more: bool,
}

/// Interpret a reply to a single `isc_info_svc_to_eof` request.
pub fn parse_svc_to_eof(buffer: &[u8]) -> SvcChunk {
    let mut chunk = SvcChunk::default();
    for it in parse_svc_items(buffer) {
        match it {
            SvcInfoItem::Bytes { tag, data } if tag == ibase::isc_info_svc_to_eof as u8 => chunk.data = data,
            SvcInfoItem::Truncated => chunk.more = true,
            _ => {}
        }
    }
    chunk
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
    fn repair_request_flags_and_workers() {
        let req = repair_request("/d/a.fdb", SvcRepairOptions { validate: true, full: true, ignore_checksums: true, ..Default::default() });
        let mut want = vec![3u8, 106, 8, 0];
        want.extend_from_slice(b"/d/a.fdb");
        want.extend_from_slice(&[108, 0xa1, 0, 0, 0]); // 1 | 0x80 | 0x20
        assert_eq!(req, want);

        let sweep = sweep_request("DB", Some(4));
        assert_eq!(&sweep[..1], &[3]);
        assert_eq!(&sweep[sweep.len() - 10..], &[108, 2, 0, 0, 0, 52, 4, 0, 0, 0]);

        let list = limbo_list_request("DB");
        assert_eq!(&list[list.len() - 5..], &[108, 8, 0, 0, 0]);
    }

    #[test]
    fn limbo_resolve_picks_the_64_bit_tag_only_when_needed() {
        let small = limbo_resolve_request("DB", 67, LimboAction::Commit);
        assert_eq!(&small[small.len() - 5..], &[15, 67, 0, 0, 0]);
        let rb = limbo_resolve_request("DB", 67, LimboAction::Rollback);
        assert_eq!(rb[rb.len() - 5], 34);
        let tp = limbo_resolve_request("DB", 67, LimboAction::TwoPhase);
        assert_eq!(tp[tp.len() - 5], 17);
        let wide = limbo_resolve_request("DB", 5_000_000_000, LimboAction::Commit);
        let mut tail = vec![49u8];
        tail.extend_from_slice(&5_000_000_000u64.to_le_bytes());
        assert_eq!(&wide[wide.len() - 9..], &tail[..]);
    }

    #[test]
    fn limbo_list_decodes_single_and_multi_database_entries() {
        let mut p = vec![19u8]; // single_tra_id
        p.extend_from_slice(&41u32.to_le_bytes());
        p.push(29); // advise
        p.push(31); // rollback
        p.push(48); // multi_tra_id_64
        p.extend_from_slice(&6_000_000_000u64.to_le_bytes());
        p.push(26); // host site
        p.extend_from_slice(&[5, 0]);
        p.extend_from_slice(b"linux");
        p.push(18); // tra_id (participant)
        p.extend_from_slice(&67u32.to_le_bytes());
        p.push(21);
        p.push(22); // limbo
        p.push(27);
        p.extend_from_slice(&[6, 0]);
        p.extend_from_slice(b"remote");
        p.push(28);
        p.extend_from_slice(&[6, 0]);
        p.extend_from_slice(b"/x.fdb");
        p.push(46); // tra_id_64 (second participant)
        p.extend_from_slice(&68u64.to_le_bytes());
        p.push(21);
        p.push(23); // committed
        p.push(29);
        p.push(30); // commit

        let list = parse_limbo_list(&p);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], LimboTransaction { id: 41, multi_database: false, host_site: None, participants: vec![], advice: Some(LimboAdvice::Rollback) });
        let m = &list[1];
        assert_eq!(m.id, 6_000_000_000);
        assert!(m.multi_database);
        assert_eq!(m.host_site.as_deref(), Some("linux"));
        assert_eq!(m.participants.len(), 2);
        assert_eq!(m.participants[0], LimboParticipant { id: 67, state: LimboState::Limbo, remote_site: Some("remote".into()), db_path: Some("/x.fdb".into()) });
        assert_eq!(m.participants[1].state, LimboState::Committed);
        assert_eq!(m.advice, Some(LimboAdvice::Commit));

        // A cut-off stream keeps what was whole.
        assert_eq!(parse_limbo_list(&p[..6]).len(), 1);
        assert_eq!(parse_limbo_list(&[]).len(), 0);
    }

    #[test]
    fn properties_request_uses_one_byte_mode_selectors() {
        let req = properties_request(
            "DB",
            &SvcProperties {
                sweep_interval: Some(0),
                write_mode: Some(SvcWriteMode::Sync),
                access_mode: Some(SvcAccessMode::ReadOnly),
                reserve_space: Some(SvcReserveSpace::UseFull),
                replica_mode: Some(SvcReplicaMode::ReadOnly),
                nolinger: true,
                ..Default::default()
            },
        );
        let mut want = vec![8u8, 106, 2, 0, b'D', b'B'];
        want.extend_from_slice(&[6, 0, 0, 0, 0]); // sweep interval 0
        want.extend_from_slice(&[12, 38]); // write_mode sync
        want.extend_from_slice(&[13, 39]); // access_mode readonly
        want.extend_from_slice(&[11, 35]); // reserve_space use full
        want.extend_from_slice(&[46, 1]); // replica read-only
        want.extend_from_slice(&[108, 0, 4, 0, 0]); // nolinger 0x400
        assert_eq!(req, want);
        assert_eq!(properties_request("DB", &SvcProperties::default()), vec![8u8, 106, 2, 0, b'D', b'B']);
    }

    #[test]
    fn shutdown_and_online_requests() {
        let req = shutdown_request("DB", SvcShutdownMode::Single, SvcShutdownKind::DenyTransactions, 60);
        assert_eq!(req, vec![8u8, 106, 2, 0, b'D', b'B', 44, 2, 43, 60, 0, 0, 0]);
        let forced = shutdown_request("DB", SvcShutdownMode::Full, SvcShutdownKind::Force, 100_000);
        assert_eq!(&forced[6..], &[44, 3, 41, 0xff, 0x7f, 0, 0], "timeout clamped to 32767");
        assert_eq!(online_request("DB", SvcShutdownMode::Normal), vec![8u8, 106, 2, 0, b'D', b'B', 45, 0]);
    }

    #[test]
    fn db_stats_request_tables_as_string_tags_or_command_line() {
        let opts = SvcStatsOptions { header: true, data_pages: true, index_pages: true, record_versions: true, tables: vec!["T1".into(), "T2".into()], ..Default::default() };
        let req = db_stats_request("DB", &opts);
        let mut want = vec![11u8, 106, 2, 0, b'D', b'B', 108, 0x2d, 0, 0, 0];
        want.extend_from_slice(&[64, 2, 0, b'T', b'1', 64, 2, 0, b'T', b'2']);
        assert_eq!(req, want);

        let old = db_stats_request("DB", &SvcStatsOptions { tables_on_command_line: true, ..opts });
        let mut want = vec![11u8, 106, 2, 0, b'D', b'B', 108, 0x2d, 0, 0, 0, 105, 11, 0];
        want.extend_from_slice(b"-t T1 -t T2");
        assert_eq!(old, want);
    }

    #[test]
    fn validate_request_layout() {
        let req = validate_request("DB", &SvcValidateOptions { tables_include: Some("(CUST|ORD)%".into()), lock_timeout: Some(0), ..Default::default() });
        let mut want = vec![30u8, 106, 2, 0, b'D', b'B', 1, 11, 0];
        want.extend_from_slice(b"(CUST|ORD)%");
        want.extend_from_slice(&[5, 0, 0, 0, 0]);
        assert_eq!(req, want);
        assert_eq!(fb_log_request(), vec![12u8]);
    }

    #[test]
    fn nbackup_requests() {
        let req = nbak_request("DB", &SvcNbakOptions { level: Some(1), file: "/b/l1.nbk".into(), no_triggers: true, direct: Some(true), clean_history: Some(SvcNbakKeep::Days(7)), ..Default::default() });
        let mut want = vec![20u8, 106, 2, 0, b'D', b'B', 5, 1, 0, 0, 0, 6, 9, 0];
        want.extend_from_slice(b"/b/l1.nbk");
        want.extend_from_slice(&[108, 1, 0, 0, 0, 7, 2, 0, b'o', b'n', 9, 10, 7, 0, 0, 0]);
        assert_eq!(req, want);

        let by_guid = nbak_request("DB", &SvcNbakOptions { guid: Some("{G}".into()), file: "f".into(), ..Default::default() });
        assert_eq!(&by_guid[6..11], &[8, 3, 0, b'{', b'G']);

        let rest = nrest_request("DB", &["l0", "l1"], true, false);
        assert_eq!(rest, vec![21u8, 106, 2, 0, b'D', b'B', 6, 2, 0, b'l', b'0', 6, 2, 0, b'l', b'1', 108, 2, 0, 0, 0]);
        assert_eq!(nfix_request("DB", true), vec![31u8, 106, 2, 0, b'D', b'B', 108, 4, 0, 0, 0]);
    }

    #[test]
    fn users_request_and_reply() {
        assert_eq!(display_users_request(None, None), vec![7u8]);
        let req = display_users_request(Some("DB"), Some("JOHN"));
        assert_eq!(req, vec![7u8, 7, 4, 0, b'J', b'O', b'H', b'N', 106, 2, 0, b'D', b'B']);

        let mut p = vec![7u8, 6, 0];
        p.extend_from_slice(b"SYSDBA");
        p.extend_from_slice(&[10, 3, 0, b'S', b'q', b'l', 5, 0, 0, 0, 0, 13, 1, 0, 0, 0]);
        p.extend_from_slice(&[7, 4, 0, b'J', b'O', b'H', b'N', 12, 3, 0, b'D', b'o', b'e']);
        let users = parse_svc_users(&p);
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "SYSDBA");
        assert_eq!(users[0].first_name, "Sql");
        assert_eq!(users[0].user_id, Some(0));
        assert_eq!(users[0].admin, Some(true));
        assert_eq!(users[1].last_name, "Doe");
        assert_eq!(users[1].admin, None);
    }

    #[test]
    fn info_items_decode_every_reply_shape() {
        let mut b = vec![55u8, 5, 0];
        b.extend_from_slice(b"LI-V5");
        b.extend_from_slice(&[57, 0x02, 0, 0, 0]); // capabilities, no length
        b.extend_from_slice(&[67, 1, 0, 0, 0]); // running
        b.extend_from_slice(&[50, 5, 3, 0, 0, 0, 6, 1, 0, 0, 0, 106, 2, 0, b'D', b'B', 127]); // svr_db_info
        b.extend_from_slice(&[66, 5, 0, 19, 41, 0, 0, 0]); // limbo bytes
        b.extend_from_slice(&[63, 3, 0, 0xe9, b'a', b'b', 2]); // to_eof raw (non-UTF-8 kept), truncated
        b.push(64); // timeout marker
        b.push(1);
        b.push(99); // past the end: ignored
        let items = parse_svc_items(&b);
        assert_eq!(
            items,
            vec![
                SvcInfoItem::Text { tag: 55, text: "LI-V5".into() },
                SvcInfoItem::Number { tag: 57, value: 2 },
                SvcInfoItem::Number { tag: 67, value: 1 },
                SvcInfoItem::ServerDbInfo { attachments: 3, databases: 1, names: vec!["DB".into()] },
                SvcInfoItem::Bytes { tag: 66, data: vec![19, 41, 0, 0, 0] },
                SvcInfoItem::Bytes { tag: 63, data: vec![0xe9, b'a', b'b'] },
                SvcInfoItem::Truncated,
                SvcInfoItem::Timeout,
                SvcInfoItem::End,
            ]
        );
        let chunk = parse_svc_to_eof(&b[b.len() - 10..]);
        assert_eq!(chunk, SvcChunk { data: vec![0xe9, b'a', b'b'], more: true });
    }

    #[test]
    fn line_replies_with_timeouts() {
        assert_eq!(svc_timeout_send_items(5), vec![64u8, 2, 0, 5, 0]);
        let mut line = vec![62u8, 2, 0, b'o', b'k', 1];
        assert_eq!(parse_svc_line_reply(&line), SvcLineReply::Line("ok".into()));
        line = vec![62, 0, 0, 1];
        assert_eq!(parse_svc_line_reply(&line), SvcLineReply::End);
        assert_eq!(parse_svc_line_reply(&[64, 1]), SvcLineReply::Timeout);
        assert_eq!(parse_svc_line_reply(&[1]), SvcLineReply::End);
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
