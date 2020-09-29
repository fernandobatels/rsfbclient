//! Helpers for abstracting over database attachment types

use super::error::FbError;
#[derive(Clone)]
pub struct ConnectionArgsEmbedded {
    pub user: String,
    pub db_name: String,
}
#[derive(Clone)]
pub struct ConnectionArgsRemote {
    pub host: String,
    pub user: String,
    pub db_name: String,
    pub port: u16,
    pub pass: String,
}

pub struct Embedded {}
pub struct Remote {}

pub trait FirebirdClientAttach<A> {
    /// The type of database handle to return
    type DbHandle: Send;

    /// Arguments needed to attach to the database
    type ConnArgs: Send + Sync + Clone;

    fn attach_database(&mut self, connargs: &Self::ConnArgs) -> Result<Self::DbHandle, FbError>;
}
