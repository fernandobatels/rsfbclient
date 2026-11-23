//! Engine codes and source contexts survive the native and wire clients.
mk_tests_default! {
    use crate::{prelude::*, FbError};
    use rsfbclient_core::ibase;

    #[test]
    fn engine_status_keeps_real_psql_context_separate_from_user_text() -> Result<(), FbError> {
        let user_text = "Literal @1 / SQLSTATE=08006 / At block line: 99, col: 1";
        let mut conn = cbuilder().connect()?;
        conn.execute(&format!("CREATE EXCEPTION RSFB_STATUS_TEST '{user_text}'"), ())?;
        let error = conn.execute("EXECUTE BLOCK AS\nBEGIN\n EXCEPTION RSFB_STATUS_TEST;\nEND", ()).unwrap_err();
        let FbError::Sql { msg, code, diagnostics } = error else {
            panic!("Expected a Firebird SQL error, got {error}");
        };
        assert_eq!(code, -836);
        assert_eq!(diagnostics.primary_gds_code(), Some(ibase::isc_except));
        assert_eq!(diagnostics.sql_state.as_deref(), Some("HY000"));
        assert!(diagnostics.gds_codes.contains(&ibase::isc_stack_trace));
        assert!(msg.contains(user_text), "{msg}");
        assert_eq!(diagnostics.psql_contexts.len(), 1, "{msg}");
        let frame = &msg[diagnostics.psql_contexts[0].clone()];
        assert!(frame.contains("line: 3"), "{frame}");
        assert!(!frame.contains("line: 99"), "{frame}");
        assert!(diagnostics.external_psql_contexts.is_empty());
        conn.execute("DROP EXCEPTION RSFB_STATUS_TEST", ())?;
        Ok(())
    }

    #[test]
    fn duplicate_keys_report_the_integrity_sqlstate_and_leave_the_connection_usable() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;
        conn.execute("CREATE TABLE RSFB_STATUS_KEYS (ID INT PRIMARY KEY)", ())?;
        conn.execute("INSERT INTO RSFB_STATUS_KEYS VALUES (1)", ())?;
        let error = conn.execute("INSERT INTO RSFB_STATUS_KEYS VALUES (1)", ()).unwrap_err();
        let FbError::Sql { msg, code, diagnostics } = error else {
            panic!("Expected a Firebird SQL error, got {error}");
        };
        assert_eq!(code, -803);
        assert_eq!(diagnostics.sql_state.as_deref(), Some("23000"));
        assert!(diagnostics.gds_codes.contains(&ibase::isc_unique_key_violation));
        assert!(diagnostics.psql_contexts.is_empty(), "{msg}");
        let rows: Vec<(i32,)> = conn.query("SELECT ID FROM RSFB_STATUS_KEYS", ())?;
        assert_eq!(rows, [(1,)]);
        conn.close()?;
        cbuilder().connect()?.execute("DROP TABLE RSFB_STATUS_KEYS", ())?;
        Ok(())
    }
}
