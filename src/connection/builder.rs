use super::{ConnectionConfiguration, FirebirdClientFactory};
use crate::charset::Charset;
use crate::{Connection, Dialect, FbError};
use rsfbclient_core::FirebirdClient;

impl<Cli: FirebirdClient> ConnectionConfiguration<Cli> {
    /// Open a new connection to the database
    pub fn connect<F: FirebirdClientFactory<C = Cli>>(
        &self,
        factory: F,
    ) -> Result<Connection<F::C>, FbError> {
        Connection::open(factory.new()?, self)
    }
}

///A default factory implementation for a statically linked native firebird client
#[cfg(feature = "linking")]
pub mod native_static_client {
    use super::*;
    use rsfbclient_native::NativeFbClient;
    struct StaticLinked(Charset);

    ///Get a factory instance for a statically linked version of the official firebird client
    ///The UTF_8 charset is provided at the top level as a convenience
    pub fn static_linked_client(
        charset: Charset,
    ) -> impl FirebirdClientFactory<C = NativeFbClient> {
        StaticLinked(charset)
    }
    impl FirebirdClientFactory for StaticLinked {
        type C = NativeFbClient;
        fn new(&self) -> Result<Self::C, FbError> {
            NativeFbClient::new_static_linked(self.0.clone())
        }
    }
}

///A default factory implementation for a dynamically linked native firebird client
#[cfg(feature = "dynamic_loading")]
pub mod native_dynlinked_client {
    use super::*;
    use rsfbclient_native::NativeFbClient;

    struct DynLinked(Charset, String);

    ///Get a factory instance for a dynamically loaded version of the official firebird client
    ///The UTF_8 charset is provided at the top level as a convenience
    pub fn dyn_linked_client<S: Into<String>>(
        charset: Charset,
        path: S,
    ) -> impl FirebirdClientFactory<C = NativeFbClient> {
        DynLinked(charset, path.into())
    }

    impl FirebirdClientFactory for DynLinked {
        type C = NativeFbClient;
        fn new(&self) -> Result<Self::C, FbError> {
            NativeFbClient::new_dyn_linked(self.0.clone(), &self.1)
        }
    }
}

///A default builder and factory implementation for the pure rust firebird client
#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
pub mod native_builder {
    use super::*;
    use rsfbclient_native::{NativeFbClient, RemoteConfig};

    type NativeConfig = ConnectionConfiguration<NativeFbClient>;
    impl NativeConfig {
        /// Username. Default: SYSDBA
        pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
            self.attachment_conf.user = user.into();
            self
        }

        /// Database name or path. Default: test.fdb
        pub fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
            self.attachment_conf.db_name = db_name.into();
            self
        }

        //private helper accessor for the Option<RemoteConfig> buried inside
        //the configuration
        fn get_initialized_remote(&mut self) -> &mut RemoteConfig {
            self.attachment_conf
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
        ///
        /// Password. Default: masterkey
        pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
            self.get_initialized_remote().password = pass.into();
            self
        }

        /// SQL Dialect. Default: 3
        pub fn dialect(&mut self, dialect: Dialect) -> &mut Self {
            self.dialect = dialect;
            self
        }

        /// Statement cache size. Default: 20
        pub fn stmt_cache_size(&mut self, stmt_cache_size: usize) -> &mut Self {
            self.stmt_cache_size = stmt_cache_size;
            self
        }

        /// Instantiate a default configuration for an embedded attachment
        pub fn embedded_default() -> Self {
            EmbeddedNativeConfig::default().0
        }

        /// Instantiate a default configuration for a remote attachment
        pub fn remote_default() -> Self {
            RemoteNativeConfig::default().0
        }
    }

    struct EmbeddedNativeConfig(NativeConfig);

    impl Default for EmbeddedNativeConfig {
        fn default() -> Self {
            //in a moment we will use the builder to
            //actually set the attachment defaults...
            let attachment_conf = Default::default();
            let mut result = NativeConfig {
                attachment_conf,
                dialect: Dialect::D3,
                stmt_cache_size: 20,
            };
            //..here
            result.user("SYSDBA").db_name("test.fdb");
            EmbeddedNativeConfig(result)
        }
    }

    struct RemoteNativeConfig(NativeConfig);
    impl Default for RemoteNativeConfig {
        fn default() -> Self {
            let EmbeddedNativeConfig(mut conf) = EmbeddedNativeConfig::default();
            conf.host("localhost").port(3050).pass("masterkey");
            RemoteNativeConfig(conf)
        }
    }
}

#[cfg(feature = "pure_rust")]
pub mod pure_rust_builder {
    use super::*;
    use rsfbclient_rust::RustFbClient;

    struct PureRustClientConfig(Charset);

    impl FirebirdClientFactory for PureRustClientConfig {
        type C = RustFbClient;
        fn new(&self) -> Result<Self::C, FbError> {
            RustFbClient::new(self.0.clone())
        }
    }

    ///Get a factory instance for a pure rust client
    ///The UTF_8 charset is provided at the top level as a convenience
    pub fn pure_rust_client(charset: Charset) -> impl FirebirdClientFactory<C = RustFbClient> {
        PureRustClientConfig(charset)
    }

    type PureRustConfig = ConnectionConfiguration<rsfbclient_rust::RustFbClient>;
    impl PureRustConfig {
        /// Username. Default: SYSDBA
        pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
            self.attachment_conf.user = user.into();
            self
        }

        /// Database name or path. Default: test.fdb
        pub fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
            self.attachment_conf.db_name = db_name.into();
            self
        }

        /// Hostname or IP address of the server. Default: localhost
        pub fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
            self.attachment_conf.host = host.into();
            self
        }

        /// TCP Port of the server. Default: 3050
        pub fn port(&mut self, port: u16) -> &mut Self {
            self.attachment_conf.port = port;
            self
        }
        ///
        /// Password. Default: masterkey
        pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
            self.attachment_conf.pass = pass.into();
            self
        }

        /// SQL Dialect. Default: 3
        pub fn dialect(&mut self, dialect: Dialect) -> &mut Self {
            self.dialect = dialect;
            self
        }

        /// Statement cache size. Default: 20
        pub fn stmt_cache_size(&mut self, stmt_cache_size: usize) -> &mut Self {
            self.stmt_cache_size = stmt_cache_size;
            self
        }
    }

    impl Default for PureRustConfig {
        fn default() -> Self {
            let attachment_conf = Default::default();
            let mut result = Self {
                attachment_conf,
                dialect: Dialect::D3,
                stmt_cache_size: 20,
            };
            result
                .host("localhost")
                .port(3050)
                .user("SYSDBA")
                .db_name("test.fdb")
                .pass("masterkey");
            result
        }
    }
}
