//! The Firebird Diesel implementation

#[macro_use]
extern crate diesel;
pub mod backend;
pub mod connection;
pub mod query_builder;
pub mod types;
pub mod value;

pub mod prelude {
    pub use crate::connection::*;
}

#[cfg(test)]
mod tests;
