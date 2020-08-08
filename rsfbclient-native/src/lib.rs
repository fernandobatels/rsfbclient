//! `FirebirdConnection` implementation for the native fbclient

mod connection;
pub(crate) mod ibase;
pub(crate) mod status;

pub use connection::NativeFbClient;
