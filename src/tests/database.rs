//!
//! Rust Firebird Client
//!
//! Database(create and drop) tests
//!

mk_tests_default! {
    #[allow(unused_imports)]
    use crate::*;

    #[test]
    #[cfg(all(feature = "linking", not(feature = "embedded_tests"), not(feature = "pure_rust")))]
    fn dyn_linking() -> Result<(), FbError> {

        let conn = builder_native()
            .from_string(
                "firebird://localhost:3050/test_create_db1.fdb",
            )?
            .create_database()?;

        conn.drop_database()?;

        let conn = builder_native()
            .with_dyn_link()
            .with_remote()
            .db_name("test_create_db11.fdb")
            .user("SYSDBA")
            .host("localhost")
            .create_database()?;

        conn.drop_database()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "dynamic_loading", not(feature = "embedded_tests"), not(feature = "pure_rust")))]
    fn dyn_load() -> Result<(), FbError> {

        #[cfg(target_os = "linux")]
        let libfbclient = "libfbclient.so";
        #[cfg(target_os = "windows")]
        let libfbclient = "fbclient.dll";
        #[cfg(target_os = "macos")]
        let libfbclient = "libfbclient.dylib";

        let conn = builder_native()
            .from_string(
                &format!("firebird://localhost:3050/test_create_db2.fdb?lib={}", libfbclient),
            )?
            .create_database()?;

        conn.drop_database()?;

        let conn = builder_native()
            .with_dyn_load(libfbclient)
            .with_remote()
            .db_name("test_create_db2.fdb")
            .user("SYSDBA")
            .host("localhost")
            .create_database()?;

        conn.drop_database()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "linking", feature = "embedded_tests", not(feature = "dynamic_loading"), not(feature = "pure_rust")))]
    fn dyn_linking_embedded() -> Result<(), FbError> {
        let conn = builder_native()
            .from_string(
                "firebird:///tmp/embedded_test_create_db1.fdb?dialect=3",
            )?
            .create_database()?;

        conn.drop_database()?;

        let conn = builder_native()
            .with_dyn_link()
            .with_embedded()
            .db_name("/tmp/embedded_test_create_db11.fdb")
            .user("SYSDBA")
            .create_database()?;

        conn.drop_database()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "dynamic_loading", feature = "embedded_tests", not(feature = "linking"), not(feature = "pure_rust")))]
    fn dyn_load_embedded() -> Result<(), FbError> {

        #[cfg(target_os = "linux")]
        let libfbclient = "libfbclient.so";
        #[cfg(target_os = "windows")]
        let libfbclient = "fbclient.dll";
        #[cfg(target_os = "macos")]
        let libfbclient = "libfbclient.dylib";

        let conn = builder_native()
            .from_string(
                &format!("firebird:///tmp/embedded_test_create_db2.fdb?dialect=3&lib={}", libfbclient),
            )?
            .create_database()?;

        conn.drop_database()?;

        let conn = builder_native()
            .with_dyn_load(libfbclient)
            .with_embedded()
            .db_name("/tmp/embedded_test_create_db22.fdb")
            .user("SYSDBA")
            .create_database()?;

        conn.drop_database()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "pure_rust", not(feature = "native_client")))]
    fn pure_rust() -> Result<(), FbError> {
        let conn = builder_pure_rust()
            .from_string(
                "firebird://localhost:3050/test_create_db3.fdb",
            )?
            .create_database()?;

        conn.drop_database()?;

        let conn = builder_pure_rust()
            .db_name("test_create_db33.fdb")
            .user("SYSDBA")
            .create_database()?;

        conn.drop_database()?;

        Ok(())
    }
}
