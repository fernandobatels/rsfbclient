//! `FirebirdConnection` implementation for the native fbclient

pub(crate) mod column;
mod connection;
pub(crate) mod ibase;
#[cfg(all(target_os = "macos", feature = "dynamic_loading"))]
mod macos_transport;
pub(crate) mod params;
pub mod services;
pub(crate) mod status;
pub(crate) mod varchar;
pub(crate) mod xsqlda;

pub use connection::{NativeFbAttachmentConfig, NativeFbClient, RemoteConfig, WireCrypt};

pub use connection::{DynLink, DynLoad, LinkageMarker};

pub use services::NativeServiceManager;

pub mod managed;
