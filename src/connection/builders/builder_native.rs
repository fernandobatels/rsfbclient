use super::*;
use std::marker::PhantomData;

#[doc(hidden)]
pub use rsfbclient_native::{DynLink, DynLoad};

use rsfbclient_native::{LinkageMarker, NativeFbAttachmentConfig, NativeFbClient, RemoteConfig};

//used as markers
#[doc(hidden)]
#[derive(Clone)]
pub struct LinkageNotConfigured;
#[doc(hidden)]
#[derive(Clone)]
pub struct ConnTypeNotConfigured;
#[doc(hidden)]
#[derive(Clone)]
pub struct Embedded;
#[doc(hidden)]
#[derive(Clone)]
pub struct Remote;

//These traits are used to avoid duplicating some impl blocks
//while at the same time statically disallowing certain methods for
//NativeConnectionBuilder<A,B> where one of A or B is
//XYZNotConfigured
#[doc(hidden)]
pub trait ConfiguredConnType: Send + Sync {}
#[doc(hidden)]
impl ConfiguredConnType for Embedded {}
#[doc(hidden)]
impl ConfiguredConnType for Remote {}

#[doc(hidden)]
//note that there is also LinkageMarker implemented for DynLink and
//DynLoad in rsfbclient-native
pub trait ConfiguredLinkage {}
#[doc(hidden)]
impl ConfiguredLinkage for DynLink {}
#[doc(hidden)]
impl ConfiguredLinkage for DynLoad {}

/// A builder for a client using the official ('native') Firebird dll.
///
/// Use the `builder_native()` method to get a new builder instance, and the
/// provided configuration methods to change the default configuration params.
///
/// Note that one of `with_remote()`/`with_embedded()` and one of
/// `with_dyn_link()`/`with_dyn_load(...)` **must** be called in order to
/// enable creating a connection or calling other configuration methods.
#[derive(Clone)]
pub struct NativeConnectionBuilder<LinkageType, ConnectionType> {
    _marker_linkage: PhantomData<LinkageType>,
    _marker_conntype: PhantomData<ConnectionType>,
    conn_conf: ConnectionConfiguration<NativeFbAttachmentConfig>,

    charset: Charset,
    lib_path: Option<String>,
}

impl<A, B> From<&NativeConnectionBuilder<A, B>>
    for ConnectionConfiguration<NativeFbAttachmentConfig>
{
    fn from(arg: &NativeConnectionBuilder<A, B>) -> Self {
        arg.conn_conf.clone()
    }
}

// dev notes: impl combinations for NativeConnectionBuilder:
// <LinkageNotConfigured, ConnTypeNotConfigured>: What you get when you first call builder_native()
// <A,ConnTypeNotConfigured>: user still has to choose a connection type
// (LinkageNotConfigured,A): user still has to choose a linkage type
// <Configured,Configured> user configured linkage and connectiontype,
//   so they can continue to do other configuration or call connect()

/// Get a new instance of NativeConnectionBuilder
pub fn builder_native() -> NativeConnectionBuilder<LinkageNotConfigured, ConnTypeNotConfigured> {
    Default::default()
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

    fn get_conn_conf(&self) -> &ConnectionConfiguration<NativeFbAttachmentConfig> {
        &self.conn_conf
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

    fn get_conn_conf(&self) -> &ConnectionConfiguration<NativeFbAttachmentConfig> {
        &self.conn_conf
    }
}

impl<A, B> NativeConnectionBuilder<A, B>
where
    A: ConfiguredLinkage,
    B: ConfiguredConnType,
    //Needed to satisfy the bounds in rsfbclient_native
    A: LinkageMarker,
    Self: FirebirdClientFactory<C = NativeFbClient<A>>,
{
    /// Create a new connection from the fully-built builder
    pub fn connect(&self) -> Result<Connection<impl FirebirdClient>, FbError> {
        Connection::open(self.new_instance()?, &self.conn_conf)
    }
}

impl<A, B> NativeConnectionBuilder<A, B>
where
    A: ConfiguredLinkage,
    B: ConfiguredConnType,
{
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

    /// SQL Dialect. Default: 3
    pub fn charset(&mut self, charset: Charset) -> &mut Self {
        self.charset = charset;
        self
    }

    /// Statement cache size. Default: 20
    pub fn stmt_cache_size(&mut self, stmt_cache_size: usize) -> &mut Self {
        self.conn_conf.stmt_cache_size = stmt_cache_size;
        self
    }
}

impl<A, B> NativeConnectionBuilder<A, B> {
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
    /// Configure the native client for remote connections.
    /// This will allow configuration via the 'host', 'port' and 'pass' methods.
    pub fn with_remote(mut self) -> NativeConnectionBuilder<A, Remote> {
        let mut remote: RemoteConfig = Default::default();
        remote.host = "localhost".to_string();
        remote.port = 3050;
        remote.pass = "masterkey".to_string();
        self.conn_conf.attachment_conf.remote = Some(remote);
        self.safe_transmute()
    }

    //does nothing since the embedded config is common to both connection types
    /// Configure the native client for embedded connections.
    /// There is no 'host', 'port' or 'pass' to configure on the result of this
    /// method and attempts to call those methods will result in a
    /// compile error.
    ///
    /// Note that the embedded builder is only tested for firebird >=3.0.
    /// If the embedded connection fails, the client dll may attempt to use
    /// other means of connection automatically, such as XNET or localhost.
    ///
    /// On firebird 3.0 and above this may be restricted via the `Providers`
    /// config parameter of `firebird.conf` see official firebird documentation
    /// for more information.
    pub fn with_embedded(self) -> NativeConnectionBuilder<A, Embedded> {
        self.safe_transmute()
    }
}

impl<A> NativeConnectionBuilder<LinkageNotConfigured, A> {
    /// Uses the native client with dynamic linking.
    /// Requires that the dynamic library .dll/.so/.dylib can be found
    /// at compile time as well as runtime.
    ///
    /// Requires feature `linking`
    #[cfg(feature = "linking")]
    pub fn with_dyn_link(self) -> NativeConnectionBuilder<DynLink, A> {
        self.safe_transmute()
    }

    #[cfg(feature = "dynamic_loading")]
    /// Searches for the firebird client at runtime only, at the specified
    /// location.
    ///
    /// # Example
    ///
    /// ```no_run
    /// // On windows
    /// rsfbclient::builder_native()
    ///   .with_dyn_load("fbclient.dll")
    ///   .with_embedded();
    ///
    ///
    /// // On linux
    /// rsfbclient::builder_native()
    ///   .with_dyn_load("libfbclient.so")
    ///   .with_remote();
    ///
    /// // Any platform, file located relative to the
    /// // folder where the executable was run
    /// rsfbclient::builder_native()
    ///   .with_dyn_load("./fbclient.lib")
    ///   .with_embedded();
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
