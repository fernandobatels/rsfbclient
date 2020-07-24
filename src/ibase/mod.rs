//!
//! Rust Firebird Client
//!
//! fbclient functions and constants
//!

#[cfg(feature = "dynamic_loading")]
mod dynamic;
#[cfg(not(feature = "dynamic_loading"))]
mod linked;

#[cfg(feature = "dynamic_loading")]
pub use dynamic::*;
#[cfg(not(feature = "dynamic_loading"))]
pub use linked::*;
