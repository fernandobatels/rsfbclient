//!
//! Rust Firebird Client
//!
//! Connection tests
//!

mk_tests_default! {
    use crate::*;

    #[test]
    #[cfg(all(feature = "linking", not(feature = "embedded_tests"), not(feature = "pure_rust")))]
    fn string_conn() -> Result<(), FbError> {
        builder_native()
            .from_string(
                "firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3",
            )?
            .connect()?;

        builder_native()
            .from_string(
                "firebird://localhost:3050/test.fdb",
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "dynamic_loading", not(feature = "embedded_tests"), not(feature = "pure_rust")))]
    fn string_conn() -> Result<(), FbError> {

        #[cfg(target_os = "linux")]
        let libfbclient = "libfbclient.so";
        #[cfg(target_os = "windows")]
        let libfbclient = "fbclient.dll";
        #[cfg(target_os = "macos")]
        let libfbclient = "libfbclient.dylib";

        builder_native()
            .from_string(
                &format!("firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3&lib={}", libfbclient),
            )?
            .connect()?;

        builder_native()
            .from_string(
                &format!("firebird://localhost:3050/test.fdb?lib={}", libfbclient),
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "linking", feature = "embedded_tests", not(feature = "dynamic_loading"), not(feature = "pure_rust")))]
    fn string_conn() -> Result<(), FbError> {
        builder_native()
            .from_string(
                "firebird:///tmp/embedded_tests.fdb?dialect=3",
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "dynamic_loading", feature = "embedded_tests", not(feature = "linking"), not(feature = "pure_rust")))]
    fn string_conn() -> Result<(), FbError> {

        #[cfg(target_os = "linux")]
        let libfbclient = "libfbclient.so";
        #[cfg(target_os = "windows")]
        let libfbclient = "fbclient.dll";
        #[cfg(target_os = "macos")]
        let libfbclient = "libfbclient.dylib";

        builder_native()
            .from_string(
                &format!("firebird:///tmp/embedded_tests.fdb?dialect=3&lib={}", libfbclient),
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "pure_rust", not(feature = "native_client")))]
    fn string_conn() -> Result<(), FbError> {
        builder_pure_rust()
            .from_string(
                "firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3",
            )?
            .connect()?;

        builder_pure_rust()
            .from_string(
                "firebird://localhost:3050/test.fdb",
            )?
            .connect()?;

        Ok(())
    }
}
