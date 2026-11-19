//! Preparation reports the declared descriptors without evaluating the SQL.
mk_tests_default! {
    use crate::{prelude::*, EngineVersion, FbError, SimpleConnection, StmtType, SystemInfos};
    use rsfbclient_core::ibase;

    #[test]
    fn prepared_description_keeps_origins_parameters_and_transaction() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        conn.execute("RECREATE TABLE RSFB_DESCRIBE (ID INTEGER NOT NULL, TOTAL NUMERIC(18,4))", ())?;
        let _ = conn.execute("DROP SEQUENCE RSFB_DESCRIBE_PROBE", ());
        conn.execute("CREATE SEQUENCE RSFB_DESCRIBE_PROBE", ())?;
        conn.execute("INSERT INTO RSFB_DESCRIBE VALUES (1, 12.3)", ())?;
        conn.begin_transaction()?;
        let current_transaction = "SELECT CURRENT_TRANSACTION FROM RDB$DATABASE";
        let before: Option<(i64,)> = conn.query_first(current_transaction, ())?;
        let alias = if conn.server_engine()? >= EngineVersion::V4 {
            "An alias longer than thirty-one characters"
        } else {
            "Total amount"
        };
        let sql = format!(
            "SELECT D.TOTAL AS \"{alias}\", GEN_ID(RSFB_DESCRIBE_PROBE, 1) AS VISIT
             FROM RSFB_DESCRIBE D WHERE D.ID = :id OR D.TOTAL = :total"
        );

        let desc = conn.describe_statement(&sql, true)?;
        assert_eq!(desc.statement_type, StmtType::Select);
        assert_eq!(desc.parameters.len(), 2);
        assert_eq!(desc.parameters[0].sql_type, ibase::SQL_LONG);
        assert_eq!(desc.parameters[1].sql_type, ibase::SQL_INT64);
        assert_eq!(desc.parameters[1].scale, -4);
        assert_eq!(desc.columns[0].name, alias);
        assert_eq!(desc.columns[0].relation.as_deref(), Some("RSFB_DESCRIBE"));
        assert_eq!(desc.columns[0].field.as_deref(), Some("TOTAL"));
        assert_eq!(desc.columns[0].sql_type, ibase::SQL_INT64);
        assert_eq!(desc.columns[0].scale, -4);
        assert!(desc.columns[0].nullable);
        assert!(desc.columns[1].field.is_none());
        assert!(desc.columns[1].relation.is_none());
        assert_eq!(conn.query_first::<_, (i64,)>(current_transaction, ())?, before);
        assert_eq!(
            conn.query_first::<_, (i64,)>("SELECT GEN_ID(RSFB_DESCRIBE_PROBE, 0) FROM RDB$DATABASE", ())?,
            Some((0,))
        );
        assert_eq!(conn.prepare_statement_type("DELETE FROM RSFB_DESCRIBE")?, StmtType::Delete);
        let delete = conn.describe_statement("DELETE FROM RSFB_DESCRIBE WHERE ID=?", false)?;
        assert!(delete.columns.is_empty());
        assert_eq!(delete.parameters.len(), 1);
        conn.rollback()?;
        conn.close()?;
        let mut cleanup = cbuilder().connect()?;
        cleanup.execute("DROP TABLE RSFB_DESCRIBE", ())?;
        cleanup.execute("DROP SEQUENCE RSFB_DESCRIBE_PROBE", ())?;
        Ok(())
    }

    #[test]
    fn wide_description_resumes_after_truncated_buffers() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        let columns = (0..120).map(|i| {
            format!("CAST(? AS INTEGER) AS \"Prepared column {i} surname\"")
        }).collect::<Vec<_>>();
        let sql = format!("SELECT {} FROM RDB$DATABASE", columns.join(", "));
        let desc = conn.describe_statement(&sql, false)?;
        assert_eq!(desc.columns.len(), 120);
        assert_eq!(desc.parameters.len(), 120);
        assert!(desc.parameters.iter().all(|desc| desc.sql_type == ibase::SQL_LONG));
        for (index, column) in desc.columns.iter().enumerate() {
            assert_eq!(column.name, format!("Prepared column {index} surname"));
        }
        Ok(())
    }
}
