use super::*;
use crate::connection::conn_string;
use crate::{charset, Charset};
use rsfbclient_rust::{RustFbClient, RustFbClientAttachmentConfig};

impl FirebirdClientFactory for PureRustConnectionBuilder {
    type C = RustFbClient;
    fn new_instance(&self) -> Result<Self::C, FbError> {
        Ok(RustFbClient::new(self.1.clone()))
    }

    fn get_conn_conf(&self) -> &ConnectionConfiguration<RustFbClientAttachmentConfig> {
        &self.0
    }
}

/// A builder for a firebird client implemented in pure rust.
/// Does not currently support embedded connections.
///
/// Use `builder_pure_rust()` to obtain a new instance.
pub struct PureRustConnectionBuilder(
    ConnectionConfiguration<RustFbClientAttachmentConfig>,
    Charset,
);

impl From<&PureRustConnectionBuilder> for ConnectionConfiguration<RustFbClientAttachmentConfig> {
    fn from(arg: &PureRustConnectionBuilder) -> Self {
        arg.0.clone()
    }
}

/// Get a new instance of PureRustConnectionBuilder
pub fn builder_pure_rust() -> PureRustConnectionBuilder {
    Default::default()
}

impl PureRustConnectionBuilder {
    pub fn connect(&self) -> Result<Connection<impl FirebirdClient>, FbError> {
        Connection::open(self.new_instance()?, &self.0)
    }

    /// Username. Default: SYSDBA
    pub fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
        self.0.attachment_conf.user = user.into();
        self
    }

    /// Database name or path. Default: test.fdb
    pub fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
        self.0.attachment_conf.db_name = db_name.into();
        self
    }

    /// Hostname or IP address of the server. Default: localhost
    pub fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
        self.0.attachment_conf.host = host.into();
        self
    }

    /// TCP Port of the server. Default: 3050
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.0.attachment_conf.port = port;
        self
    }

    /// Password. Default: masterkey
    pub fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
        self.0.attachment_conf.pass = pass.into();
        self
    }

    /// SQL Dialect. Default: 3
    pub fn dialect(&mut self, dialect: Dialect) -> &mut Self {
        self.0.dialect = dialect;
        self
    }

    /// Statement cache size. Default: 20
    pub fn stmt_cache_size(&mut self, stmt_cache_size: usize) -> &mut Self {
        self.0.stmt_cache_size = stmt_cache_size;
        self
    }

    /// Connection charset. Default: UTF-8
    pub fn charset(&mut self, charset: Charset) -> &mut Self {
        self.1 = charset;
        self
    }

    /// Setup the connection using the string
    /// pattern.
    ///
    /// You can use the others methods(`host()`,`user()`...) to config
    /// some default values.
    ///
    /// Basic string format: `firebird://{user}:{pass}@{host}:{port}/{db_name}?charset={charset}&dialect={dialect}`
    pub fn from_string(&mut self, s_conn: &str) -> Result<&mut Self, FbError> {
        let settings = conn_string::parse(s_conn)?;

        if let Some(host) = settings.host {
            self.host(host);
        }

        if let Some(port) = settings.port {
            self.port(port);
        }

        if let Some(user) = settings.user {
            self.user(user);
        }

        if let Some(pass) = settings.pass {
            self.pass(pass);
        }

        self.db_name(settings.db_name);

        if let Some(charset) = settings.charset {
            self.charset(charset);
        }

        if let Some(dialect) = settings.dialect {
            self.dialect(dialect);
        }

        if let Some(stmt_cache_size) = settings.stmt_cache_size {
            self.stmt_cache_size(stmt_cache_size);
        }

        Ok(self)
    }
}

impl Default for PureRustConnectionBuilder {
    fn default() -> Self {
        let conn_conf = Default::default();
        let charset = charset::UTF_8;
        let mut result = Self(conn_conf, charset);

        result
            .host("localhost")
            .port(3050)
            .user("SYSDBA")
            .db_name("test.fdb")
            .pass("masterkey");

        result
    }
}
