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

pub type PureRustConfig = ConnectionConfiguration<rsfbclient_rust::RustFbClient>;
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
