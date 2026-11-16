//! Firebird 4 introduced DECFLOAT and temporal values with time zones.
mk_tests_default! {
    use crate::{prelude::*, EngineVersion, FbError, Row, SystemInfos};
    use rsfbclient_core::ibase;

    #[test]
    fn extended_scalars_keep_server_text_and_original_types() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;
        if conn.server_engine()? < EngineVersion::V4 {
            return Ok(());
        }

        let expressions = [
            ("CAST('1234.567890123456' AS DECFLOAT(16))", ibase::SQL_DEC16),
            ("CAST('123456789012345678901234567890.1234' AS DECFLOAT(34))", ibase::SQL_DEC34),
            ("TIME '12:30:00.1234 Europe/Prague'", ibase::SQL_TIME_TZ),
            ("TIMESTAMP '2025-01-01 12:30:00.1234 Europe/Prague'", ibase::SQL_TIMESTAMP_TZ),
            ("CAST(NULL AS DECFLOAT(34))", ibase::SQL_DEC34),
        ];
        for (expression, kind) in expressions {
            let sql = format!(
                "SELECT {expression}, CAST({expression} AS VARCHAR(128)) FROM RDB$DATABASE"
            );
            let row: Row = conn.query_first(&sql, ())?.expect("one row");
            assert_eq!(row.cols[0].raw_type, kind);
            assert_eq!(row.get::<Option<String>>(0)?, row.get::<Option<String>>(1)?);
        }

        let exact: (String,) = conn.query_first(
            "SELECT CAST('123456789012345678901234567890.1234' AS DECFLOAT(34)) FROM RDB$DATABASE", (),
        )?.expect("one row");
        assert_eq!(exact.0, "123456789012345678901234567890.1234");
        Ok(())
    }
}
