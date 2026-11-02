//! Firebird client implementation in pure rust

mod arc4;
mod blr;
mod client;
mod consts;
mod events;
mod raw;
mod services;
mod srp;
mod util;
mod wire;
mod xsqlda;

pub use client::{DbHandle, RustFbClient, RustFbClientAttachmentConfig, StmtHandle, SvcHandle, TrHandle};
pub use services::RustServiceManager;
pub use raw::RawValue;

#[cfg(feature = "fuzz_testing")]
pub use self::{blr::*, wire::*, xsqlda::*};
