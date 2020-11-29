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
            .with_string::<DynLink, &str>(
                "firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3",
            )?
            .connect()?;

        builder_native()
            .with_string::<DynLink, &str>(
                "firebird://localhost:3050/test.fdb",
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "linking", feature = "embedded_tests", not(feature = "dynamic_loading"), not(feature = "pure_rust")))]
    fn string_conn() -> Result<(), FbError> {
        builder_native()
            .with_string::<DynLink, &str>(
                "firebird://test.fdb?dialect=3",
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "pure_rust", not(feature = "native_client")))]
    fn string_conn() -> Result<(), FbError> {
        builder_pure_rust()
            .with_string(
                "firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3",
            )?
            .connect()?;

        builder_pure_rust()
            .with_string(
                "firebird://localhost:3050/test.fdb",
            )?
            .connect()?;

        Ok(())
    }
}
