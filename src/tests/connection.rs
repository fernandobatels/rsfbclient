//!
//! Rust Firebird Client
//!
//! Connection tests
//!

mk_tests_default! {
    #[allow(unused_imports)]
    use crate::*;
    use std::time::SystemTime;

    #[test]
    #[cfg(all(feature = "linking", not(feature = "embedded_tests"), not(feature = "pure_rust")))]
    fn string_conn1() -> Result<(), FbError> {
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
    fn string_conn2() -> Result<(), FbError> {

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
    fn string_conn3() -> Result<(), FbError> {
        builder_native()
            .from_string(
                "firebird:///tmp/embedded_tests.fdb?dialect=3",
            )?
            .connect()?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "dynamic_loading", feature = "embedded_tests", not(feature = "linking"), not(feature = "pure_rust")))]
    fn string_conn4() -> Result<(), FbError> {

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
    fn string_conn5() -> Result<(), FbError> {
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

    #[test]
    #[cfg(all(feature = "linking", not(feature = "embedded_tests")))]
    fn roles_from_string_conn() -> Result<(), FbError> {

        let mut conn = cbuilder()
            .connect()?;

        let epoch = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        conn.execute(&format!("create user role_str_con{} password '123'", epoch), ())?;

        conn.execute("drop role app;", ()).ok();
        conn.execute("create role app;", ())?;

        conn.execute("drop table sale;", ()).ok();
        conn.execute("create table sale (id int);", ())?;

        conn.execute("revoke all on sale from public;", ())?;
        conn.execute("grant all on sale to app;", ())?;
        conn.execute(&format!("grant app to user role_str_con{};", epoch), ())?;

        conn.close()?;

        let mut uconn = builder_native()
            .from_string(
                &format!("firebird://role_str_con{}:123@localhost:3050/test.fdb", epoch),
            )?
            .connect()?;
        assert!(uconn.execute("insert into sale (id) values (?)", (1,)).is_err());
        uconn.close()?;

        let mut uconn = builder_native()
            .from_string(
                &format!("firebird://role_str_con{}:123@localhost:3050/test.fdb?role_name=app", epoch),
            )?
            .connect()?;

        uconn.execute("insert into sale (id) values (?)", (1,))?;

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "linking", not(feature = "embedded_tests")))]
    fn roles_from_conn_builder() -> Result<(), FbError> {

        let mut conn = cbuilder()
            .connect()?;

        let epoch = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        conn.execute(&format!("create user role_cb{} password '123'", epoch), ())?;

        conn.execute("drop role app2;", ()).ok();
        conn.execute("create role app2;", ())?;

        conn.execute("drop table sale;", ()).ok();
        conn.execute("create table sale (id int);", ())?;

        conn.execute("revoke all on sale from public;", ())?;
        conn.execute("grant all on sale to app2;", ())?;
        conn.execute(&format!("grant app2 to user role_cb{};", epoch), ())?;

        conn.close()?;

        let mut uconn = builder_native()
            .with_dyn_link()
            .with_remote()
            .user(format!("role_cb{}", epoch))
            .pass("123")
            .connect()?;
        assert!(uconn.execute("insert into sale (id) values (?)", (1,)).is_err());
        uconn.close()?;

        let mut uconn = builder_native()
            .with_dyn_link()
            .with_remote()
            .user(format!("role_cb{}", epoch))
            .pass("123")
            .role("app2")
            .connect()?;

        uconn.execute("insert into sale (id) values (?)", (1,))?;

        Ok(())
    }
}
