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

pub use rsfbclient_core::services::{SvcBackupOptions, SvcRestoreOptions};

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
