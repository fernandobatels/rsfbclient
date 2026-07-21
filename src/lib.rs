//!
//! Rust Firebird Client
//!
//! # How to use it
//!
//! ### 1. Start by choosing the lib variation you want
//! ```rust,ignore
//! // To use the offcial ('native') Firebird client .dll/.so/.dylib
//! // (needs to find dll at build time)
//! rsfbclient::builder_native().with_dyn_link()
//! // Or using dynamic loading
//! rsfbclient::builder_native().with_dyn_load("/my/firebird/here/lib/libfbclient.so")
//! // Or using the pure rust implementation
//! rsfbclient::builder_pure_rust()
//! ```
//!
//! ### 2. Set your connection params
//! ```rust,ignore
//! // For a remote server, using a dynamically linked native client
//! let mut conn = rsfbclient::builder_native()
//!     .with_dyn_link()
//!     .with_remote()
//!     .host("my.host.com.br")
//!     .db_name("awesome.fdb")
//!     .connect()?
//! // Or if you need a embedded/local only access
//! let mut conn = rsfbclient::builder_native()
//!     .with_dyn_link()
//!     .with_embedded()
//!     .db_name("/path/to/awesome.fdb")
//!     .connect()?
//! ```
//!
//! You also can choose a string connection configuration
//! ```rust,ignore
//! // Using the native Firebird client
//! rsfbclient::builder_native()
//!     .from_string("firebird://SYSDBA:masterkey@my.host.com.br:3050/awesome.fdb?charset=ascii")
//! // Or using the pure rust implementation
//! rsfbclient::builder_pure_rust()
//!     .from_string("firebird://SYSDBA:masterkey@my.host.com.br:3050/awesome.fdb?charset=ascii")
//! ```
//!
//! ### 3. Now you can use the lib
//! ```rust,ignore
//! let rows = conn.query_iter("select col_a, col_b, col_c from test", ())?;
//! ...
//! ```
//!
//! # Simple Connection/Transaction
//!
//! Sometimes you will need store the [Connection](./struct.Connection.html) and [Transaction](struct.Transaction.html) types into a struct field without care about Firebird Client variation. To do this, you can use the [SimpleConnection](struct.SimpleConnection.html) and [SimpleTransaction](struct.SimpleTransaction.html) types.
//!
//! To use, you only need use the [From](std::convert::From) trait, calling the `into()` method. Example:
//! ```rust,ignore
//! let mut conn: SimpleConnection = rsfbclient::builder_native()
//!     .with_dyn_link()
//!     .with_remote()
//!     .host("my.host.com.br")
//!     .db_name("awesome.fdb")
//!     .connect()?
//!     .into();
//! ```
//!
//! # Transactions
//!
//! Every [Connection](./struct.Connection.html) keeps one **default transaction**, started
//! lazily by the first `query`/`execute` call with the configuration given at connect time
//! (the builder's `transaction(...)`/`with_transaction(...)` options; the defaults are
//! `ReadCommited` + record version, `Wait` without timeout, read-write). Outside of an
//! explicit `begin_transaction`, every statement is committed automatically.
//!
//! Two properties of the default transaction are worth knowing:
//!
//! - [Connection::commit](./struct.Connection.html#method.commit) and
//!   [Connection::rollback](./struct.Connection.html#method.rollback) are **retaining**
//!   operations: they end the current unit of work but keep the same physical transaction
//!   alive. Its configuration — and, for `TrIsolationLevel::Concurrency` (SNAPSHOT), the
//!   snapshot it started with — persists for the whole life of the connection.
//! - Because the physical transaction is reused,
//!   [begin_transaction_config](./struct.Connection.html#method.begin_transaction_config)
//!   only applies its configuration if the default transaction has not started yet (i.e.
//!   before the first statement on the connection).
//!
//! When you need a transaction with its own isolation level, lock policy or lifetime —
//! e.g. a SNAPSHOT reader beside a NO WAIT writer — create an explicit transaction object:
//! [Transaction](./struct.Transaction.html) for a typed connection, or
//! [SimpleTransaction](./struct.SimpleTransaction.html) for a
//! [SimpleConnection](./struct.SimpleConnection.html). Explicit objects take a
//! `TransactionConfiguration` and offer a real, consuming `commit()`/`rollback()` (plus
//! `_retaining` variants):
//! ```rust,ignore
//! let snapshot = TransactionConfiguration {
//!     isolation: TrIsolationLevel::Concurrency,
//!     lock_resolution: TrLockResolution::NoWait,
//!     ..TransactionConfiguration::default()
//! };
//! let mut tr = SimpleTransaction::new(&mut conn, snapshot)?;
//! tr.execute("update accounts set balance = balance + 1 where id = 1", ())?;
//! tr.commit()?; // consuming: this transaction really ends here
//! ```
//! See `examples/isolation_levels.rs` for the full demonstration (snapshot visibility and
//! NO WAIT update conflicts, live).
//!
//! # Data type mappings
//!
//! Column values arrive through [SqlType](./enum.SqlType.html), which is deliberately
//! coarse. The supported mappings:
//!
//! | Firebird type | Rust type |
//! |---|---|
//! | `SMALLINT`, `INTEGER`, `BIGINT` | `i64` (or any smaller integer type via `try_into`) |
//! | `FLOAT`, `DOUBLE PRECISION`, `NUMERIC`, `DECIMAL` | `f64` / `f32` |
//! | `CHAR`, `VARCHAR` | `String` (decoded with the connection charset) |
//! | `BLOB SUB_TYPE TEXT` | `String` |
//! | `BLOB SUB_TYPE BINARY` | `Vec<u8>` |
//! | `DATE`, `TIME`, `TIMESTAMP` | `chrono::NaiveDate` / `NaiveTime` / `NaiveDateTime` |
//! | `BOOLEAN` | `bool` (Firebird 3+) |
//! | any nullable column | `Option<T>` |
//!
//! The sharp edges:
//!
//! - **`NUMERIC`/`DECIMAL` go through `f64`**: scaled values whose integer form exceeds
//!   2^53 lose precision silently (e.g. `NUMERIC(18,2)` storing `90071992547409.93` reads
//!   back as `90071992547409.92`). When the exact digits matter, `CAST` the column to
//!   `VARCHAR` in SQL and parse, or keep the value in a wider text/integer form.
//! - **Firebird 4+ types are not supported by the row reader**: selecting an `INT128`,
//!   `DECFLOAT(16/34)`, `TIMESTAMP WITH TIME ZONE` or `TIME WITH TIME ZONE` column (or a
//!   blob with `sub_type > 1`) fails at describe time with *"Unsupported column type"*.
//!   `CAST` such columns to `VARCHAR`/`BIGINT`/plain `TIMESTAMP` in the SQL to move the
//!   conversion server-side.
//!
//! See `examples/type_mapping.rs` for all of the above, live.
//!
//! # Cargo features
//! All features can be used at the same time if needed.
//!
//! ### `linking`
//! Will use the dynamic library of the official `fbclient` at runtime and compiletime. Used in systems where there is already a firebird client installed and configured.
//! ### `dynamic_loading`
//! Can find the official `fbclient` native library by path at runtime, does not need the library at compiletime. Useful when you need to build in a system without a firebird client installed.
//! ### `pure_rust`
//! Uses a pure rust implementation of the firebird wire protocol, does not need the native library at all. Useful for cross-compilation and allow a single binary to be deployed without needing to install the firebird client.

#[cfg(test)]
#[macro_use]
pub(crate) mod tests;

pub mod prelude {
    pub use crate::query::{Execute, Queryable};
    pub use crate::transaction::{transaction_builder, TransactionConfigurationBuilder};
    pub use rsfbclient_core::{
        TrDataAccessMode, TrIsolationLevel, TrLockResolution, TrRecordVersion,
        TransactionConfiguration,
    };
    pub use rsfbclient_derive::IntoParams;
}

mod connection;
mod events;
mod query;
mod statement;
mod transaction;
mod utils;

pub use crate::{
    connection::{Connection, ConnectionConfiguration, FirebirdClientFactory, SimpleConnection},
    events::RemoteEventsManager,
    query::{Execute, Queryable},
    statement::Statement,
    transaction::{SimpleTransaction, Transaction},
    utils::{EngineVersion, SystemInfos},
};
pub use rsfbclient_core::{
    Column, ColumnToVal, Dialect, FbError, FromRow, IntoParam, IntoParams, ParamsType, Row, SqlType,
};

#[doc(hidden)]
pub use rsfbclient_core::{charset, Charset};

//builders are behind feature gates inside this module
pub use crate::connection::builders;
pub use builders::*;
