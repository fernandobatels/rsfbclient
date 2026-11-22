//! Bind and fetch fixed-point values on actual Firebird attachments.
mk_tests_default! {
    use crate::{prelude::*, EngineVersion, FbError, FixedDecimal, Row, SimpleConnection, SqlType, SystemInfos};
    use rsfbclient_core::ibase;

    #[test]
    fn bigint_and_smallint_fixed_point_round_trip_without_rounding() -> Result<(), FbError> {
        let mut connection: SimpleConnection = cbuilder().connect()?.into();
        connection.execute(
            "CREATE TABLE RSFB_FIXED_POINT (AMOUNT NUMERIC(18,2), SMALL_VALUE NUMERIC(4,2), MISSING NUMERIC(9,3), PAYLOAD BLOB SUB_TYPE BINARY)",
            (),
        )?;
        let amount = FixedDecimal::new(-9_007_199_254_740_993, -2);
        let small = FixedDecimal::new(7, -2);
        connection.execute(
            "INSERT INTO RSFB_FIXED_POINT (AMOUNT, SMALL_VALUE, PAYLOAD) VALUES (?,?,?)",
            (amount, small, vec![255, 0, 23]),
        )?;

        let sql = "SELECT AMOUNT, SMALL_VALUE, MISSING FROM RSFB_FIXED_POINT";
        let row: Row = connection.query_first(sql, ())?.unwrap();
        assert_eq!(row.cols[0].raw_type, ibase::SQL_INT64);
        assert_eq!(row.cols[0].raw_scale, -2);
        assert_eq!(row.cols[1].raw_type, ibase::SQL_SHORT);
        assert_eq!(row.cols[1].raw_scale, -2);
        assert_eq!(row.cols[2].raw_scale, -3);
        assert!(matches!(row.cols[0].value, SqlType::Decimal(value) if value == amount));
        assert_eq!(row.get::<String>(0)?, "-90071992547409.93");
        assert_eq!(row.get::<String>(1)?, "0.07");
        assert_eq!(row.get::<FixedDecimal>(1)?, small);
        assert_eq!(row.get::<f64>(1)?, 0.07);
        assert_eq!(row.get::<Option<FixedDecimal>>(2)?, None);

        // A BLOB column routes wire results through the blob-capable parser.
        let with_blob: (FixedDecimal, Vec<u8>) = connection
            .query_first("SELECT AMOUNT, PAYLOAD FROM RSFB_FIXED_POINT", ())?
            .unwrap();
        assert_eq!(with_blob, (amount, vec![255, 0, 23]));

        #[cfg(feature = "pure_rust")]
        if let Some(wire) = connection.as_pure_rust_mut() {
            let mut seen = 0;
            wire.stream_raw(sql, |values| {
                assert!(matches!(values[0], crate::RawValue::Decimal(value) if value == amount));
                assert!(matches!(values[1], crate::RawValue::Decimal(value) if value == small));
                assert!(matches!(values[2], crate::RawValue::Null));
                seen += 1;
                Ok(())
            })?;
            assert_eq!(seen, 1);
        }

        connection.close()?;
        cbuilder().connect()?.execute("DROP TABLE RSFB_FIXED_POINT", ())?;
        Ok(())
    }

    #[test]
    fn wide_fixed_point_round_trip_keeps_all_38_digits() -> Result<(), FbError> {
        let mut connection = cbuilder().connect()?;
        if !matches!(connection.server_engine()?, EngineVersion::V4 | EngineVersion::V5) {
            return Ok(());
        }
        let wide = FixedDecimal::new(12_345_678_901_234_567_890_123_456_789_012_345_678, -4);
        let tiny = FixedDecimal::new(-7, -30);
        let sql = "SELECT CAST(? AS DECIMAL(38,4)), CAST(? AS NUMERIC(38,30)) FROM RDB$DATABASE";
        let row: Row = connection.query_first(sql, (wide, tiny))?.unwrap();
        assert_eq!(row.cols[0].raw_type, ibase::SQL_INT128);
        assert_eq!(row.cols[0].raw_scale, -4);
        assert_eq!(row.get::<FixedDecimal>(0)?, wide);
        assert_eq!(row.get::<FixedDecimal>(1)?, tiny);
        assert_eq!(row.get::<String>(0)?, "1234567890123456789012345678901234.5678");
        assert_eq!(row.get::<String>(1)?, "-0.000000000000000000000000000007");
        Ok(())
    }
}
