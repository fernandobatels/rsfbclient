//! Helpers for abstracting over database attachment types
//! to be used by implementers of FirebirdClient.

use super::charset::Charset;

#[derive(Clone)]
pub struct AttachmentArgsEmbedded {
    pub user: String,
    pub db_name: String,
    pub charset: Charset,
}
#[derive(Clone)]
pub struct AttachmentArgsRemote {
    pub host: String,
    pub user: String,
    pub db_name: String,
    pub port: u16,
    pub pass: String,
    pub charset: Charset,
}

pub struct Embedded;
pub struct Remote;


pub trait FirebirdClientAttach<A> {
    /// The type of database handle to return
    type DbHandle: Send;

    /// Arguments needed to attach to the database
    type AttachArgs: Send + Sync + Clone;
    type Builder: AttachmentBuilder<Args = Self::AttachArgs>

    fn attach_database(&mut self, connargs: &Self::AttachArgs) -> Result<Self::DbHandle, FbError>;
}

pub trait AttachmentBuilder : ToOwned {
  type Args : ToOwned;
  fn to_attachment_args(&self) -> &Self::Args;
}

pub trait AttachmentBuilderEmbedded {

    /// Database name or path. Default: test.fdb
    fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self;

    /// Username. Default: SYSDBA
    fn user<S: Into<String>>(&mut self, user: S) -> &mut Self;

    /// Charset. Default: UTF_8
    fn charset(&mut self, charset: Charset) -> &mut Self;
}

pub trait AttachmentBuilderRemote {
    /// Hostname or IP address of the server. Default: localhost
    fn host<S: Into<String>>(&mut self, host: S) -> &mut Self;

    /// TCP Port of the server. Default: 3050
    fn port(&mut self, port: u16) -> &mut Self;

    /// Password. Default: masterkey
    fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self;
}

impl AttachmentBuilder for AttachmentArgsEmbedded {
  type Args = Self;
  fn to_attachment_args(&self) -> &Self::Args {
    self
  }
}

impl AttachmentBuilderEmbedded for AttachmentArgsEmbedded {
    /// Database name or path. Default: test.fdb
    fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
        self.db_name = db_name.into();
        self
    }

    /// Username. Default: SYSDBA
    fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
        self.user = user.into();
        self
    }

    fn charset(&mut self, charset: Charset) -> &mut Self {
        self.charset = charset;
        self
    }
}

impl AttachmentBuilder for AttachmentArgsRemote {
  type Args = Self;
  fn to_attachment_args(&self) -> &Self::Args {
    self
  }
}
impl AttachmentBuilderEmbedded for AttachmentArgsRemote {
  fn db_name<S: Into<String>>(&mut self, db_name: S) -> &mut Self {
      self.db_name = db_name.into();
      self
  }

  fn user<S: Into<String>>(&mut self, user: S) -> &mut Self {
      self.user = user.into();
      self
  }
  fn charset(&mut self, charset: Charset) -> &mut Self {
      self.charset = charset;
      self
  }
}

impl AttachmentBuilderRemote for AttachmentArgsRemote {
    fn host<S: Into<String>>(&mut self, host: S) -> &mut Self {
        self.host = host.into();
        self
    }

    fn port(&mut self, port: u16) -> &mut Self {
        self.port = port;
        self
    }


    fn pass<S: Into<String>>(&mut self, pass: S) -> &mut Self {
        self.pass = pass.into();
        self
    }
}

