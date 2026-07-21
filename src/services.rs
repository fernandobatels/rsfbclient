//!
//! Firebird Services API: server-side administration through the
//! `service_mgr` endpoint — the same channel the `gbak` and
//! `fbtracemgr` tools use.
//!
//! The Services API is separate from the SQL protocol: you attach to a
//! *service manager* rather than to a database, start an *action*
//! (backup, restore, ...) described by a service parameter block, and
//! then drain the action's output line by line with information
//! requests. Everything happens **on the server**: the database paths
//! and backup-file paths passed to [ServiceManager::backup] and
//! [ServiceManager::restore] are server paths, and the files they
//! produce are owned by the server process.
//!
//! Only the NATIVE client (`linking` or `dynamic_loading` features)
//! can speak to the service manager — the service protocol is carried
//! by dedicated opcodes that the pure-rust wire implementation does not
//! implement, so this module is not available with only the
//! `pure_rust` feature.
//!
//! # Example
//!
//! ```rust,ignore
//! use rsfbclient::services::ServiceManager;
//!
//! let mut svc = ServiceManager::builder()
//!     .host("localhost")
//!     .user("SYSDBA")
//!     .pass("masterkey")
//!     .attach()?;
//!
//! println!("server version: {}", svc.server_version()?);
//!
//! // Verbose backup: gbak's log arrives through the closure, one
//! // line at a time, while the backup runs.
//! svc.backup_with_output(
//!     "/data/mydb.fdb",              // server path!
//!     "/data/mydb.fbk",              // server path!
//!     SvcBackupOptions::default(),
//!     |line| println!("gbak: {}", line),
//! )?;
//!
//! // Restore over an existing database.
//! let opts = SvcRestoreOptions {
//!     replace: true,
//!     ..SvcRestoreOptions::default()
//! };
//! svc.restore("/data/mydb.fbk", "/data/mydb-copy.fdb", opts)?;
//! ```

use rsfbclient_core::{ibase, FbError};

#[cfg(feature = "linking")]
use rsfbclient_native::DynLink;
#[cfg(feature = "dynamic_loading")]
use rsfbclient_native::DynLoad;
use rsfbclient_native::NativeServiceManager;

/// Options for [ServiceManager::backup] /
/// [ServiceManager::backup_with_output]. All default to `false`,
/// matching `gbak`'s defaults.
#[derive(Clone, Copy, Default)]
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

/// Options for [ServiceManager::restore] /
/// [ServiceManager::restore_with_output].
#[derive(Clone, Copy, Default)]
pub struct SvcRestoreOptions {
    /// Replace the target database if it already exists (`gbak -rep`).
    /// When `false` (the default) the restore fails if the target
    /// exists — the safe behaviour, same as `gbak -c`.
    pub replace: bool,
    /// Page size of the restored database (`gbak -p`); `None` keeps
    /// the page size recorded in the backup
    pub page_size: Option<u32>,
}

/// Builder for a [ServiceManager] attachment.
///
/// Mirrors the connection builders in miniature: host/port/user/pass
/// plus the choice between the dynamically linked fbclient (the
/// default when the `linking` feature is on) and a dynamically loaded
/// one ([ServiceManagerBuilder::with_dyn_load]).
#[derive(Clone)]
pub struct ServiceManagerBuilder {
    host: String,
    port: u16,
    user: String,
    pass: String,
    lib_path: Option<String>,
}

impl Default for ServiceManagerBuilder {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            lib_path: None,
        }
    }
}

impl ServiceManagerBuilder {
    /// Hostname or IP of the server. Default: `localhost`
    pub fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
        self.host = host.into();
        self
    }

    /// TCP port of the server. Default: `3050`
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = port;
        self
    }

    /// User name. Default: `SYSDBA`
    pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
        self.user = user.into();
        self
    }

    /// Password. Default: `masterkey`
    pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
        self.pass = pass.into();
        self
    }

    /// Load the fbclient library from this path at runtime instead of
    /// using the dynamically linked one (requires the
    /// `dynamic_loading` feature).
    pub fn with_dyn_load<S: Into<String>>(&mut self, lib_path: S) -> &mut Self {
        self.lib_path = Some(lib_path.into());
        self
    }

    /// Attach to the server's service manager.
    pub fn attach(&self) -> Result<ServiceManager, FbError> {
        let spb = build_attach_spb(&self.user, &self.pass);

        match &self.lib_path {
            None => {
                #[cfg(feature = "linking")]
                {
                    let inner = NativeServiceManager::<DynLink>::attach_dyn_link(
                        &self.host, self.port, &spb,
                    )?;
                    Ok(ServiceManager {
                        inner: SvcContainer::Linking(inner),
                    })
                }
                #[cfg(not(feature = "linking"))]
                Err(FbError::from(
                    "The 'linking' feature is disabled; use with_dyn_load() to load a client library by path",
                ))
            }
            Some(lib_path) => {
                #[cfg(feature = "dynamic_loading")]
                {
                    let inner = NativeServiceManager::<DynLoad>::attach_dyn_load(
                        lib_path, &self.host, self.port, &spb,
                    )?;
                    Ok(ServiceManager {
                        inner: SvcContainer::DynLoad(inner),
                    })
                }
                #[cfg(not(feature = "dynamic_loading"))]
                {
                    let _ = lib_path;
                    Err(FbError::from(
                        "with_dyn_load() requires the 'dynamic_loading' feature",
                    ))
                }
            }
        }
    }
}

enum SvcContainer {
    #[cfg(feature = "linking")]
    Linking(NativeServiceManager<DynLink>),
    #[cfg(feature = "dynamic_loading")]
    DynLoad(NativeServiceManager<DynLoad>),
}

/// An attachment to a Firebird server's service manager.
///
/// Obtained through [ServiceManager::builder]. Detaches on drop.
pub struct ServiceManager {
    inner: SvcContainer,
}

impl ServiceManager {
    /// Start building a service manager attachment.
    pub fn builder() -> ServiceManagerBuilder {
        ServiceManagerBuilder::default()
    }

    /// The server version string, e.g. `LI-V4.0.2.2816 Firebird 4.0`.
    pub fn server_version(&mut self) -> Result<String, FbError> {
        let receive = [ibase::isc_info_svc_server_version as u8];
        let mut buffer = [0u8; 1024];
        self.query(&[], &receive, &mut buffer)?;

        if buffer[0] != ibase::isc_info_svc_server_version as u8 {
            return Err(FbError::from("Unexpected service version reply"));
        }
        let len = u16::from_le_bytes([buffer[1], buffer[2]]) as usize;
        Ok(String::from_utf8_lossy(&buffer[3..3 + len]).to_string())
    }

    /// Run a `gbak`-style backup of `db_name` (a SERVER path) into
    /// `backup_file` (also a SERVER path), blocking until it finishes.
    ///
    /// The service's output is drained and discarded; use
    /// [ServiceManager::backup_with_output] to receive it.
    pub fn backup(
        &mut self,
        db_name: &str,
        backup_file: &str,
        options: SvcBackupOptions,
    ) -> Result<(), FbError> {
        self.backup_with_output(db_name, backup_file, options, |_| {})
    }

    /// Like [ServiceManager::backup], but streams the verbose `gbak`
    /// log through `on_line`, one line at a time, while the backup
    /// runs.
    pub fn backup_with_output<F: FnMut(&str)>(
        &mut self,
        db_name: &str,
        backup_file: &str,
        options: SvcBackupOptions,
        on_line: F,
    ) -> Result<(), FbError> {
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

        self.start(&req)?;
        self.drain_output(on_line)
    }

    /// Restore `backup_file` (a SERVER path) into `db_name` (also a
    /// SERVER path), blocking until it finishes.
    ///
    /// With the default options the restore fails if `db_name` already
    /// exists; set [SvcRestoreOptions::replace] to overwrite it.
    pub fn restore(
        &mut self,
        backup_file: &str,
        db_name: &str,
        options: SvcRestoreOptions,
    ) -> Result<(), FbError> {
        self.restore_with_output(backup_file, db_name, options, |_| {})
    }

    /// Like [ServiceManager::restore], but streams the verbose `gbak`
    /// log through `on_line`.
    pub fn restore_with_output<F: FnMut(&str)>(
        &mut self,
        backup_file: &str,
        db_name: &str,
        options: SvcRestoreOptions,
        on_line: F,
    ) -> Result<(), FbError> {
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

        self.start(&req)?;
        self.drain_output(on_line)
    }

    /// Detach from the service manager. Also called automatically on
    /// drop.
    pub fn detach(&mut self) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            SvcContainer::Linking(s) => s.detach(),
            #[cfg(feature = "dynamic_loading")]
            SvcContainer::DynLoad(s) => s.detach(),
        }
    }

    fn start(&mut self, request: &[u8]) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            SvcContainer::Linking(s) => s.start(request),
            #[cfg(feature = "dynamic_loading")]
            SvcContainer::DynLoad(s) => s.start(request),
        }
    }

    fn query(&mut self, send: &[u8], receive: &[u8], buffer: &mut [u8]) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            SvcContainer::Linking(s) => s.query(send, receive, buffer),
            #[cfg(feature = "dynamic_loading")]
            SvcContainer::DynLoad(s) => s.query(send, receive, buffer),
        }
    }

    /// Poll `isc_info_svc_line` until the running action's output is
    /// exhausted — the same loop `gbak -se` runs. Each drained line is
    /// handed to `on_line`.
    fn drain_output<F: FnMut(&str)>(&mut self, mut on_line: F) -> Result<(), FbError> {
        let receive = [ibase::isc_info_svc_line as u8];
        loop {
            let mut buffer = [0u8; 4096];
            self.query(&[], &receive, &mut buffer)?;

            if buffer[0] != ibase::isc_info_svc_line as u8 {
                // isc_info_end or anything unexpected: the action is over
                return Ok(());
            }
            let len = u16::from_le_bytes([buffer[1], buffer[2]]) as usize;
            if len == 0 {
                // an empty line ends the stream
                return Ok(());
            }
            on_line(&String::from_utf8_lossy(&buffer[3..3 + len]));
        }
    }
}

/// Version-2 attach SPB: version tags + credentials, each argument a
/// tag byte, a ONE-byte length and the bytes.
fn build_attach_spb(user: &str, pass: &str) -> Vec<u8> {
    let mut spb = vec![
        ibase::isc_spb_version as u8,
        ibase::isc_spb_current_version as u8,
    ];
    spb.push(ibase::isc_spb_user_name as u8);
    spb.push(user.len() as u8);
    spb.extend_from_slice(user.as_bytes());
    spb.push(ibase::isc_spb_password as u8);
    spb.push(pass.len() as u8);
    spb.extend_from_slice(pass.as_bytes());
    spb
}

/// Start-request string argument: tag, TWO-byte little-endian length,
/// bytes. (Attach and start blocks use different length encodings —
/// one of the classic Services API traps.)
fn push_string_arg(req: &mut Vec<u8>, tag: u8, value: &str) {
    req.push(tag);
    req.extend_from_slice(&(value.len() as u16).to_le_bytes());
    req.extend_from_slice(value.as_bytes());
}

/// Start-request numeric argument: tag + four-byte little-endian value.
fn push_u32_arg(req: &mut Vec<u8>, tag: u8, value: u32) {
    req.push(tag);
    req.extend_from_slice(&value.to_le_bytes());
}
