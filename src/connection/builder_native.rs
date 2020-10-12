use super::{Connection, ConnectionConfiguration, FirebirdClientFactory};
use crate::{charset, Charset, Dialect, FbError};
use rsfbclient_core::FirebirdClient;
pub use rsfbclient_native::{DynLink, DynLoad};
use rsfbclient_native::{LinkageMarker, NativeFbAttachmentConfig, NativeFbClient, RemoteConfig};
use std::marker::PhantomData;

//used as markers
#[doc(hidden)]
pub struct LinkageNotConfigured;
#[doc(hidden)]
pub struct ConnTypeNotConfigured;
#[doc(hidden)]
pub struct Embedded;
#[doc(hidden)]
pub struct Remote;

#[doc(hidden)]
pub trait ConfiguredConnType: Send + Sync {}
#[doc(hidden)]
impl ConfiguredConnType for Embedded {}
#[doc(hidden)]
impl ConfiguredConnType for Remote {}

#[doc(hidden)]
pub trait ConfiguredLinkage {}

#[doc(hidden)]
impl ConfiguredLinkage for DynLink {}
#[doc(hidden)]
impl ConfiguredLinkage for DynLoad {}

//TODO: Doc. Include notes about needing to call
//one of as_remote(), as_embedded()
//and one of linked(), with_dynlib()
#[derive(Clone)]
pub struct NativeConnectionBuilder<LinkageType, ConnectionType> {
    _marker_linkage: PhantomData<LinkageType>,
    _marker_conntype: PhantomData<ConnectionType>,
    conn_conf: ConnectionConfiguration<NativeFbAttachmentConfig>,

    charset: Charset,
    lib_path: Option<String>,
}

// dev notes: impl combinations for NativeConnectionBuilder:
// <LinkageNotconfigured, ConnTypeNotConfigured): What you get when you first call builder_native()
// (A,B): methods user can use at any time, common to all implementations
// (A,ConnTypeNotConfigured): user can still choose a connection type
// (NotConfigured,A): user can still choose a linkage type
// (Configured,Configured) user configured everything needed, and can call connect()

pub fn builder_native() -> NativeConnectionBuilder<LinkageNotConfigured, ConnTypeNotConfigured> {
    Default::default()
}

impl<A, B> NativeConnectionBuilder<A, B>
where
    A: ConfiguredLinkage,
    A: LinkageMarker,
    B: ConfiguredConnType,
    Self: FirebirdClientFactory<C = NativeFbClient<A>>,
{
    pub fn connect(&self) -> Result<Connection<impl FirebirdClient>, FbError> {
        Connection::open(self.new_instance()?, &self.conn_conf)
    }
}

#[cfg(feature = "dynamic_loading")]
impl<A> FirebirdClientFactory for NativeConnectionBuilder<DynLoad, A>
where
    A: ConfiguredConnType,
{
    //would be better if we could use 'impl FirebirdClient' here
    type C = NativeFbClient<rsfbclient_native::DynLoad>;

    fn new_instance(&self) -> Result<Self::C, FbError> {
        //not ideal, but the types should
        //guarantee this is ok
        let path = self.lib_path.as_ref().unwrap();

        rsfbclient_native::DynLoad {
            charset: self.charset.clone(),
            lib_path: path.clone(),
        }
        .try_to_client()
    }
}

#[cfg(feature = "linking")]
impl<A> FirebirdClientFactory for NativeConnectionBuilder<DynLink, A>
where
    A: ConfiguredConnType,
{
    //would be better if we could use 'impl FirebirdClient' here
    type C = NativeFbClient<rsfbclient_native::DynLink>;

    fn new_instance(&self) -> Result<Self::C, FbError> {
        Ok(rsfbclient_native::DynLink(self.charset.clone()).to_client())
    }
}

impl<A, B> NativeConnectionBuilder<A, B> {
    /// Username. Default: SYSDBA
    pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
        self.conn_conf.attachment_conf.user = user.into();
        self
    }

    /// Database name or path. Default: test.fdb
    pub fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
        self.conn_conf.attachment_conf.db_name = db_name.into();
        self
    }

    /// SQL Dialect. Default: 3
    pub fn dialect(&mut self, dialect: Dialect) -> &mut Self {
        self.conn_conf.dialect = dialect;
        self
    }

    /// Statement cache size. Default: 20
    pub fn stmt_cache_size(&mut self, stmt_cache_size: usize) -> &mut Self {
        self.conn_conf.stmt_cache_size = stmt_cache_size;
        self
    }

    //never export this. It would allow users to bypass the type safety.
    fn safe_transmute<X, Y>(self) -> NativeConnectionBuilder<X, Y> {
        NativeConnectionBuilder {
            _marker_linkage: PhantomData,
            _marker_conntype: PhantomData,
            conn_conf: self.conn_conf,
            charset: self.charset,
            lib_path: self.lib_path,
        }
    }
}

impl Default for NativeConnectionBuilder<LinkageNotConfigured, ConnTypeNotConfigured> {
    fn default() -> Self {
        let mut self_result = Self {
            _marker_linkage: PhantomData,
            _marker_conntype: PhantomData,
            conn_conf: Default::default(),
            charset: charset::UTF_8,
            lib_path: None,
        };

        self_result.conn_conf.dialect = Dialect::D3;
        self_result.conn_conf.stmt_cache_size = 20;
        self_result.conn_conf.attachment_conf.remote = None;
        self_result.conn_conf.attachment_conf.user = "SYSDBA".to_string();
        self_result.conn_conf.attachment_conf.db_name = "test.fdb".to_string();

        self_result
    }
}

//can only use these methods on a remote builder
impl<A> NativeConnectionBuilder<A, Remote> {
    //private helper accessor for the Option<RemoteConfig> buried inside
    //the configuration
    fn get_initialized_remote(&mut self) -> &mut RemoteConfig {
        self.conn_conf
            .attachment_conf
            .remote
            .get_or_insert(Default::default())
    }

    /// Hostname or IP address of the server. Default: localhost
    pub fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
        self.get_initialized_remote().host = host.into();
        self
    }

    /// TCP Port of the server. Default: 3050
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.get_initialized_remote().port = port;
        self
    }

    /// Password. Default: masterkey
    pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
        self.get_initialized_remote().pass = pass.into();
        self
    }
}

impl<A> NativeConnectionBuilder<A, ConnTypeNotConfigured> {
    //adds the default remote config and then allows user to configure it
    //TODO: Doc
    pub fn as_remote(mut self) -> NativeConnectionBuilder<A, Remote> {
        let mut remote: RemoteConfig = Default::default();
        remote.host = "localhost".to_string();
        remote.port = 3050;
        remote.pass = "masterkey".to_string();
        self.conn_conf.attachment_conf.remote = Some(remote);
        self.safe_transmute()
    }

    //does nothing since the embedded config is common to both connection types
    //TODO: Doc
    pub fn as_embedded(self) -> NativeConnectionBuilder<A, Embedded> {
        self.safe_transmute()
    }
}

impl<A> NativeConnectionBuilder<LinkageNotConfigured, A> {
    //TODO: Doc: note about how this is dynamic linking,
    // not static linking in some way
    #[cfg(feature = "linking")]
    pub fn with_dyn_link(self) -> NativeConnectionBuilder<DynLink, A> {
        self.safe_transmute()
    }

    #[cfg(feature = "dynamic_loading")]
    /// Searches for the firebird client at runtime, in the specified path.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rsfbclient::ConnectionBuilder;
    ///
    /// // On windows
    /// ConnectionBuilder::with_dynlib("fbclient.dll");
    ///
    /// // On linux
    /// ConnectionBuilder::with_dynlib("libfbclient.so");
    ///
    /// // Any platform, file located relative to the
    /// // folder where the executable was run
    /// ConnectionBuilder::with_dynlib("./fbclient.lib");
    /// ```
    /// Requires feature 'dynamic_loading'.
    pub fn with_dyn_load<S: Into<String>>(
        mut self,
        lib_path: S,
    ) -> NativeConnectionBuilder<DynLoad, A> {
        self.lib_path = Some(lib_path.into());
        self.safe_transmute()
    }
}
