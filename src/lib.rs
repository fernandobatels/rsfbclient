//!
//! Rust Firebird Client
//!

#[cfg(test)]
#[macro_use]
pub(crate) mod tests;

pub mod prelude {
    pub use crate::query::{Execute, Queryable};
    pub use rsfbclient_derive::IntoParams;
}

mod connection;
mod query;
mod statement;
mod transaction;

pub use crate::{
    connection::{Connection, ConnectionConfiguration, FirebirdClientFactory, SimpleConnection},
    query::{Execute, Queryable},
    statement::Statement,
    transaction::{Transaction, SimpleTransaction},
};
pub use rsfbclient_core::{
    Column, Dialect, FbError, FromRow, IntoParam, IntoParams, ParamsType, Row, SqlType,
};

#[doc(hidden)]
pub use rsfbclient_core::{charset, Charset};

#[cfg(feature = "pool")]
pub use crate::connection::pool::FirebirdConnectionManager;

//builders are behind feature gates inside this module
pub use crate::connection::builders;
pub use builders::*;
