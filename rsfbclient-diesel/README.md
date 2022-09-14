
# Firebird adapter for diesel ORM

[Diesel](https://crates.io/crates/diesel) is a safe, extensible ORM and Query Builder for Rust. With this crate you can use it for access the Firebird database.

This crate only implements the firebird backend for Diesel. To use diesel features, you must import it.

By default the lib will use the [native client](https://docs.rs/rsfbclient-native/0.20.0/rsfbclient_native/struct.NativeFbClient.html). If you want use the [pure rust client](https://docs.rs/rsfbclient-rust/0.20.0/rsfbclient_rust/struct.RustFbClient.html), enable the `pure_rust` feature.

### Establishing a connection

```rust,ignore
use diesel::prelude::*;
use rsfbclient_diesel::FbConnection;

let conn = FbConnection::establish("firebird://SYSDBA:masterkey@localhost/test.fdb");
```

### CRUD example

We also provide a [CRUD example](https://github.com/fernandobatels/rsfbclient/tree/master/rsfbclient-diesel/examples/employee) with `employee.fdb` database.
