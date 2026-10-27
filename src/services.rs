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
//! Both clients speak to the service manager: the native one
//! (`linking` / `dynamic_loading`) through `isc_service_*`, and the
//! pure-rust one (`pure_rust`) over the wire protocol's
//! `op_service_*` opcodes, with the same SRP handshake and wire
//! encryption as a database connection. The builder picks the native
//! client when it is compiled in, else the pure-rust one;
//! [ServiceManagerBuilder::pure_rust] forces the latter.
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

use rsfbclient_core::{ibase, services, FbError};

pub use rsfbclient_core::services::{
    LimboAction, LimboAdvice, LimboParticipant, LimboState, LimboTransaction, SvcAccessMode, SvcBackupOptions,
    SvcInfoItem, SvcLineReply, SvcNbakKeep, SvcNbakOptions, SvcProperties, SvcRepairOptions, SvcReplicaMode,
    SvcReserveSpace, SvcRestoreOptions, SvcShutdownKind, SvcShutdownMode, SvcStatsOptions, SvcUser,
    SvcValidateOptions, SvcWriteMode,
};

/// What [`ServiceManager::server_info`] reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SvcServerInfo {
    /// `LI-V5.0.1.1469 Firebird 5.0`
    pub version: String,
    /// `Firebird/Linux/AMD/Intel/x64`
    pub implementation: Option<String>,
    /// `isc_info_svc_capabilities` bit mask (architecture hints).
    pub capabilities: Option<u32>,
    pub root_dir: Option<String>,
    pub lock_dir: Option<String>,
    pub msg_dir: Option<String>,
    pub security_db: Option<String>,
    /// Attachments the service process can see (SuperServer: all of them).
    pub attachments: Option<u32>,
    /// Databases attached in that process.
    pub databases: Option<Vec<String>>,
}

#[cfg(feature = "linking")]
use rsfbclient_native::DynLink;
#[cfg(feature = "dynamic_loading")]
use rsfbclient_native::DynLoad;
#[cfg(feature = "native_client")]
use rsfbclient_native::NativeServiceManager;
#[cfg(feature = "pure_rust")]
use rsfbclient_rust::RustServiceManager;

/// Builder for a [ServiceManager] attachment.
///
/// Mirrors the connection builders in miniature: host/port/user/pass
/// plus the choice of client — the dynamically linked fbclient (the
/// default when the `linking` feature is on), a dynamically loaded one
/// ([ServiceManagerBuilder::with_dyn_load]), or the pure-rust wire
/// client ([ServiceManagerBuilder::pure_rust], the default when no
/// native client is compiled in).
#[derive(Clone)]
pub struct ServiceManagerBuilder {
    host: String,
    port: u16,
    user: String,
    pass: String,
    lib_path: Option<String>,
    force_pure_rust: bool,
    expected_db: Option<String>,
}

impl Default for ServiceManagerBuilder {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3050,
            user: "SYSDBA".to_string(),
            pass: "masterkey".to_string(),
            lib_path: None,
            force_pure_rust: false,
            expected_db: None,
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

    /// Use the pure-rust wire client even when a native client is
    /// compiled in (requires the `pure_rust` feature).
    pub fn pure_rust(&mut self) -> &mut Self {
        self.force_pure_rust = true;
        self
    }

    /// The database the coming actions concern (a server path or
    /// alias). Servers configured with a per-database security database
    /// need it to know where to authenticate; pure-rust client only.
    pub fn expected_db<S: Into<String>>(&mut self, db: S) -> &mut Self {
        self.expected_db = Some(db.into());
        self
    }

    /// Attach to the server's service manager.
    pub fn attach(&self) -> Result<ServiceManager, FbError> {
        if let Some(lib_path) = &self.lib_path {
            #[cfg(feature = "dynamic_loading")]
            {
                let spb = services::attach_spb_legacy(&self.user, &self.pass);
                let inner = NativeServiceManager::<DynLoad>::attach_dyn_load(
                    lib_path, &self.host, self.port, &spb,
                )?;
                return Ok(ServiceManager {
                    inner: SvcContainer::DynLoad(inner),
                });
            }
            #[cfg(not(feature = "dynamic_loading"))]
            {
                let _ = lib_path;
                return Err(FbError::from(
                    "with_dyn_load() requires the 'dynamic_loading' feature",
                ));
            }
        }

        if !self.force_pure_rust {
            #[cfg(feature = "linking")]
            {
                let spb = services::attach_spb_legacy(&self.user, &self.pass);
                let inner =
                    NativeServiceManager::<DynLink>::attach_dyn_link(&self.host, self.port, &spb)?;
                return Ok(ServiceManager {
                    inner: SvcContainer::Linking(inner),
                });
            }
        }

        #[cfg(feature = "pure_rust")]
        {
            let inner = RustServiceManager::attach(
                &self.host,
                self.port,
                &self.user,
                &self.pass,
                self.expected_db.as_deref(),
            )?;
            return Ok(ServiceManager {
                inner: SvcContainer::PureRust(inner),
            });
        }

        #[cfg(not(feature = "pure_rust"))]
        Err(FbError::from(
            "No service client available: enable the 'linking' feature, use with_dyn_load(), or enable 'pure_rust'",
        ))
    }
}

enum SvcContainer {
    #[cfg(feature = "linking")]
    Linking(NativeServiceManager<DynLink>),
    #[cfg(feature = "dynamic_loading")]
    DynLoad(NativeServiceManager<DynLoad>),
    #[cfg(feature = "pure_rust")]
    PureRust(RustServiceManager),
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
        services::parse_svc_string(&buffer, ibase::isc_info_svc_server_version as u8)
            .ok_or_else(|| FbError::from("Unexpected service version reply"))
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
        self.start(&services::backup_request(db_name, backup_file, options))?;
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
        self.start(&services::restore_request(backup_file, db_name, options))?;
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
            #[cfg(feature = "pure_rust")]
            SvcContainer::PureRust(s) => s.detach(),
        }
    }

    /// Start a raw service action: `request` is the action byte and its
    /// arguments (`rsfbclient_core::services` builds the common ones).
    /// Read its output with [ServiceManager::query].
    pub fn start(&mut self, request: &[u8]) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            SvcContainer::Linking(s) => s.start(request),
            #[cfg(feature = "dynamic_loading")]
            SvcContainer::DynLoad(s) => s.start(request),
            #[cfg(feature = "pure_rust")]
            SvcContainer::PureRust(s) => s.start(request),
        }
    }

    /// Raw service query (`isc_service_query`): `send` / `receive` items,
    /// reply written into `buffer`.
    pub fn query(&mut self, send: &[u8], receive: &[u8], buffer: &mut [u8]) -> Result<(), FbError> {
        match &mut self.inner {
            #[cfg(feature = "linking")]
            SvcContainer::Linking(s) => s.query(send, receive, buffer),
            #[cfg(feature = "dynamic_loading")]
            SvcContainer::DynLoad(s) => s.query(send, receive, buffer),
            #[cfg(feature = "pure_rust")]
            SvcContainer::PureRust(s) => s.query(send, receive, buffer),
        }
    }

    /// Raw service query returning the reply bytes. The buffer offered is
    /// the largest the native API's 16-bit lengths allow.
    pub fn info(&mut self, send: &[u8], receive: &[u8]) -> Result<Vec<u8>, FbError> {
        let mut buffer = vec![0u8; 32000];
        self.query(send, receive, &mut buffer)?;
        Ok(buffer)
    }

    /// Ask for `receive` items and decode the reply.
    pub fn info_items(&mut self, receive: &[u8]) -> Result<Vec<SvcInfoItem>, FbError> {
        let raw = self.info(&[], receive)?;
        Ok(services::parse_svc_items(&raw))
    }

    fn info_text(&mut self, tag: u32) -> Result<Option<String>, FbError> {
        let items = self.info_items(&[tag as u8])?;
        Ok(items.into_iter().find_map(|it| match it {
            SvcInfoItem::Text { tag: t, text } if t == tag as u8 => Some(text),
            _ => None,
        }))
    }

    /// What the server says about itself. The SYSDBA-only items
    /// (directories, security database, attached databases) come back
    /// `None` for another user instead of failing the whole call.
    pub fn server_info(&mut self) -> Result<SvcServerInfo, FbError> {
        let version = self.server_version()?;
        let implementation = self.info_text(ibase::isc_info_svc_implementation)?;
        let capabilities = self
            .info_items(&[ibase::isc_info_svc_capabilities as u8])
            .ok()
            .and_then(|items| {
                items.into_iter().find_map(|it| match it {
                    SvcInfoItem::Number { value, .. } => Some(value),
                    _ => None,
                })
            });
        let root_dir = self.info_text(ibase::isc_info_svc_get_env).ok().flatten();
        let lock_dir = self.info_text(ibase::isc_info_svc_get_env_lock).ok().flatten();
        let msg_dir = self.info_text(ibase::isc_info_svc_get_env_msg).ok().flatten();
        let security_db = self.info_text(ibase::isc_info_svc_user_dbpath).ok().flatten();
        let (attachments, databases) = match self.info_items(&[ibase::isc_info_svc_svr_db_info as u8]) {
            Ok(items) => items
                .into_iter()
                .find_map(|it| match it {
                    SvcInfoItem::ServerDbInfo { attachments, names, .. } => Some((Some(attachments), Some(names))),
                    _ => None,
                })
                .unwrap_or((None, None)),
            Err(_) => (None, None),
        };
        Ok(SvcServerInfo { version, implementation, capabilities, root_dir, lock_dir, msg_dir, security_db, attachments, databases })
    }

    /// Is the started action still running (`isc_info_svc_running`)?
    pub fn is_running(&mut self) -> Result<bool, FbError> {
        let items = self.info_items(&[ibase::isc_info_svc_running as u8])?;
        Ok(items.iter().any(|it| matches!(it, SvcInfoItem::Number { value, .. } if *value != 0)))
    }

    /// One line of the running action's output. With a timeout the call
    /// returns [`SvcLineReply::Timeout`] after that many seconds of silence
    /// instead of blocking — the way to keep a Stop button responsive on
    /// an action that prints nothing (sweep, validate, properties).
    pub fn next_line(&mut self, timeout_seconds: Option<u16>) -> Result<SvcLineReply, FbError> {
        let send = timeout_seconds.map(services::svc_timeout_send_items).unwrap_or_default();
        let mut buffer = vec![0u8; 8192];
        self.query(&send, &[ibase::isc_info_svc_line as u8], &mut buffer)?;
        Ok(services::parse_svc_line_reply(&buffer))
    }

    /// Drain the running action's output as raw bytes (`isc_info_svc_to_eof`),
    /// chunk by chunk, until the server reports the end. For the server log
    /// and anything else too long for lines.
    pub fn read_to_eof<F: FnMut(&[u8])>(&mut self, mut on_chunk: F) -> Result<(), FbError> {
        loop {
            let raw = self.info(&[], &[ibase::isc_info_svc_to_eof as u8])?;
            let chunk = services::parse_svc_to_eof(&raw);
            if !chunk.data.is_empty() {
                on_chunk(&chunk.data);
            }
            if !chunk.more {
                return Ok(());
            }
        }
    }

    /// `gfix -list`: the limbo transactions of `db_name` (a SERVER path or
    /// alias), with their participants and the engine's advice.
    pub fn limbo_list(&mut self, db_name: &str) -> Result<Vec<LimboTransaction>, FbError> {
        self.start(&services::limbo_list_request(db_name))?;
        let mut payload = Vec::new();
        // The list can be split across replies at any byte; an empty
        // payload says it is over.
        for _ in 0..1024 {
            let raw = self.info(&[], &[ibase::isc_info_svc_limbo_trans as u8])?;
            let mut got = false;
            for it in services::parse_svc_items(&raw) {
                if let SvcInfoItem::Bytes { tag, data } = it {
                    if tag == ibase::isc_info_svc_limbo_trans as u8 && !data.is_empty() {
                        payload.extend_from_slice(&data);
                        got = true;
                    }
                }
            }
            if !got {
                break;
            }
        }
        Ok(services::parse_limbo_list(&payload))
    }

    /// Poll `isc_info_svc_line` until the running action's output is
    /// exhausted — the same loop `gbak -se` runs. Each drained line is
    /// handed to `on_line`.
    fn drain_output<F: FnMut(&str)>(&mut self, mut on_line: F) -> Result<(), FbError> {
        let receive = [ibase::isc_info_svc_line as u8];
        loop {
            let mut buffer = [0u8; 4096];
            self.query(&[], &receive, &mut buffer)?;
            match services::parse_svc_line(&buffer) {
                Some(line) => on_line(&line),
                None => return Ok(()),
            }
        }
    }
}
