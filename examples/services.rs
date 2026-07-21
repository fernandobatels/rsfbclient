//!
//! Rust Firebird Client
//!
//! Example of the Services API: server version and a verbose
//! backup/restore round trip through the service manager — the same
//! channel gbak uses.
//!
//! Needs the native client (linking or dynamic_loading feature); the
//! pure-rust wire implementation cannot reach the service manager.
//!
//! All paths below are SERVER paths: the backup file is created on the
//! server, by the server, no matter where this program runs.
//!

#![allow(unused_variables, unused_mut)]

fn main() {
    #[cfg(any(feature = "linking", feature = "dynamic_loading"))]
    example::run().unwrap();

    #[cfg(not(any(feature = "linking", feature = "dynamic_loading")))]
    println!(
        "The Services API needs the native client (enable the linking or dynamic_loading feature)"
    );
}

#[cfg(any(feature = "linking", feature = "dynamic_loading"))]
mod example {
    use rsfbclient::services::{ServiceManager, SvcBackupOptions, SvcRestoreOptions};
    use rsfbclient::FbError;

    pub fn run() -> Result<(), FbError> {
        #[cfg(feature = "linking")]
        let mut svc = ServiceManager::builder()
            .host("localhost")
            .user("SYSDBA")
            .pass("masterkey")
            .attach()?;

        #[cfg(all(feature = "dynamic_loading", not(feature = "linking")))]
        let mut svc = ServiceManager::builder()
            .host("localhost")
            .user("SYSDBA")
            .pass("masterkey")
            .with_dyn_load("./fbclient.lib")
            .attach()?;

        println!("server version: {}", svc.server_version()?);

        // Verbose backup: the gbak log streams through the closure
        // while the backup runs on the server.
        println!("backing up examples.fdb:");
        svc.backup_with_output(
            "examples.fdb",
            "examples.fbk",
            SvcBackupOptions::default(),
            |line| println!("  {}", line),
        )?;

        // Restore to a new database, quietly.
        let opts = SvcRestoreOptions {
            replace: true,
            ..SvcRestoreOptions::default()
        };
        svc.restore("examples.fbk", "examples_restored.fdb", opts)?;
        println!("restored examples.fbk -> examples_restored.fdb");

        Ok(())
    }
}
