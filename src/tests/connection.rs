//!
//! Rust Firebird Client
//!
//! Connection tests
//!

#[cfg(test)]
mod test {
    use crate::*;

    #[test]
    fn string_conn() -> Result<(), FbError> {
        builder_native()
            .with_string::<DynLink, &str>(
                "firebird://SYSDBA:masterkey@localhost:3050/test.fdb?dialect=3",
            )?
            .connect()?;

        Ok(())
    }
}
