//! `FirebirdConnection` implementation for the native fbclient

pub mod connection;
pub(crate) mod ibase;
pub(crate) mod params;
pub(crate) mod row;
pub(crate) mod status;
pub(crate) mod varchar;
pub(crate) mod xsqlda;

pub use connection::NativeFbClient;
