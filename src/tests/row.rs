//!
//! Rust Firebird Client
//!
//! Fetched rows tests
//!

mk_tests_default! {
    use crate::{prelude::*, FbError, Row, EngineVersion, SystemInfos};
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    use rsfbclient_core::ColumnToVal;
    use std::str;
    use rand::{distributions::Standard, Rng};

    // Batch-fetch regression (FB_FETCH_BATCH in the pure_rust backend): fetch
    // more rows than one batch to CROSS batch boundaries. Validates count AND
    // order — order catches any framing drift between op_fetch responses
    // (batch continuation, end-of-batch, end-of-cursor).
    #[test]
    fn fetch_batches_crossing_boundary() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        // Generate 1..=500 without needing a table (crosses the default batch of 200).
        let sql = "EXECUTE BLOCK RETURNS (n int) AS
                   DECLARE i int = 0;
                   BEGIN
                     WHILE (i < 500) DO BEGIN
                       i = i + 1;
                       n = i;
                       SUSPEND;
                     END
                   END";

        let rows: Vec<(i32,)> = conn.query(sql, ())?;

        assert_eq!(500, rows.len());
        for (idx, (n,)) in rows.iter().enumerate() {
            assert_eq!(idx as i32 + 1, *n);
        }

        Ok(())
    }

    #[test]
    fn execute_affected_rows() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE EAFFECTEDROW", ()).ok();
        conn.execute("CREATE TABLE EAFFECTEDROW (id int)", ())?;

        let affected = conn.execute("insert into EAFFECTEDROW (id) values (10)", ())?;
        assert_eq!(1, affected);

        let affected = conn.execute("insert into EAFFECTEDROW (id) select 11 from RDB$DATABASE union all select 12 from RDB$DATABASE", ())?;
        assert_eq!(2, affected);

        let affected = conn.execute("update EAFFECTEDROW set id = 50", ())?;
        assert_eq!(3, affected);

        let affected = conn.execute("delete from EAFFECTEDROW", ())?;
        assert_eq!(3, affected);

        Ok(())
    }

    #[test]
    fn execute_procedure() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        if conn.server_engine()? <= EngineVersion::V2 {
            return Ok(());
        }

        let ddl_procedure = "create or alter procedure get_value()
                                returns (val int not null)
                                as
                                begin
                                    val = 150;
                                    suspend;
                                end;";
        conn.execute(ddl_procedure, ())?;

        // Using select
        let (val,): (i32,) = conn.query_first("select p.val from get_value p", ())?
            .unwrap();
        assert_eq!(150, val);

        // Using exec proc
        let (val,): (i32,) = conn.execute_returnable("execute procedure get_value", ())?;
        assert_eq!(150, val);

        Ok(())
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn execute_block() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let sql = "execute block (x double precision = ?, y double precision = ?)
                    returns (gmean double precision)
                    as
                    begin
                        gmean = sqrt(x*y);
                        suspend;
                    end";

        // with execute_returnable
        let (sqrt,): (f64,) = conn.execute_returnable(sql, (10, 20))?;
        assert_eq!(14.142135623730951, sqrt);

        // with query
        let (sqrt,): (f64,) = conn.query_first(sql, (10, 20))?
            .unwrap();
        assert_eq!(14.142135623730951, sqrt);

        Ok(())
    }

    #[test]
    fn insert_returning() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE RINSERT_RETURNING", ()).ok();
        conn.execute("CREATE TABLE RINSERT_RETURNING (id int, name varchar(10))", ())?;

        let returning: (i32, String,) = conn.execute_returnable("insert into rinsert_returning (id, name) values (10, 'abc 132') returning id, name", ())?;

        assert_eq!((10, "abc 132".to_string(),), returning);

        conn.with_transaction(|tr| {
            let id: (i32,) = tr.execute_returnable("insert into rinsert_returning (id) values (11) returning id", ())?;

            assert_eq!((11,), id);

            Ok(())
        })?;

        Ok(())
    }

    #[test]
    #[allow(clippy::bool_assert_comparison)]
    fn boolean() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        if conn.server_engine()? <= EngineVersion::V2 {
            return Ok(());
        }

        let (a, b,): (bool, bool,) = conn.query_first("select false, true from rdb$database;", ())?
            .unwrap();

        assert_eq!(false, a);
        assert_eq!(true, b);

        Ok(())
    }

    #[test]
    fn blob_binary_subtype() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a,): (Vec<u8>,) = conn.query_first("select cast(x'61626320c3a462c3a720313233' as blob SUB_TYPE 0) from rdb$database;", ())?
            .unwrap();

        assert_eq!(13, a.len());
        assert_eq!("abc äbç 123", str::from_utf8(&a).expect("Invalid UTF-8 sequence"));

        Ok(())
    }

    #[test]
    fn blob_text_subtype() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a,): (String,) = conn.query_first("select cast('abc äbç 123' as BLOB sub_type 1) from rdb$database", ())?
            .unwrap();

        assert_eq!("abc äbç 123", a);

        // With a big string....

        let (b,): (String,) = conn.query_first("select cast('Mussum Ipsum, cacilds vidis litro abertis. Admodum accumsan disputationi eu sit. Vide electram sadipscing et per. Delegadis gente finis, bibendum egestas augue arcu ut est. Paisis, filhis, espiritis santis. Quem manda na minha terra sou euzis!' as BLOB sub_type 1) from rdb$database", ())?
            .unwrap();

        assert_eq!("Mussum Ipsum, cacilds vidis litro abertis. Admodum accumsan disputationi eu sit. Vide electram sadipscing et per. Delegadis gente finis, bibendum egestas augue arcu ut est. Paisis, filhis, espiritis santis. Quem manda na minha terra sou euzis!", b);


        Ok(())
    }

    #[test]
    fn big_blob_binary() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let rvec: Vec<u8> = rand::thread_rng()
            .sample_iter(Standard)
            .take(1024 * 1024)
            .collect();

        conn.execute("DROP TABLE RBIGBLOBBIN", ()).ok();
        conn.execute("CREATE TABLE RBIGBLOBBIN (content blob sub_type 0)", ())?;

        conn.execute("insert into rbigblobbin (content) values (?)", (&rvec,))?;

        let (s,): (Vec<u8>,) = conn.query_first("select content from rbigblobbin", ())?.unwrap();

        assert_eq!(rvec, s);

        Ok(())
    }

    #[test]
    fn big_blob_text() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let rstr: String = rand::thread_rng()
            .sample_iter::<char, _>(Standard)
            .take(1024 * 1024)
            .collect();

        conn.execute("DROP TABLE RBIGBLOBTEXT", ()).ok();
        conn.execute("CREATE TABLE RBIGBLOBTEXT (content blob sub_type 1 character set utf8)", ())?;

        conn.execute("insert into rbigblobtext (content) values (?)", (&rstr,))?;

        let (s,): (String,) = conn.query_first("select content from rbigblobtext", ())?.unwrap();

        assert_eq!(rstr, s);

        Ok(())
    }

    #[test]
    fn blob_custom_subtype() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a,): (Option<Vec<u8>>,) = conn.query_first("select cast(null as blob SUB_TYPE -1) from rdb$database;", ())?
            .unwrap();

        assert_eq!(None, a);

        Ok(())
    }

    #[test]
    fn dates() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a, b, c): (NaiveDate, NaiveDateTime, NaiveTime) = conn
                .query_first(
                    "select cast('2010-10-10' as date), cast('2010-10-10 10:10:10' as TIMESTAMP), cast('10:10:10' as TIME) from rdb$database",
                    (),
                )?
                .unwrap();
        assert_eq!(NaiveDate::from_ymd(2010, 10, 10), a);
        assert_eq!(NaiveDate::from_ymd(2010, 10, 10).and_hms(10, 10, 10), b);
        assert_eq!(NaiveTime::from_hms(10, 10, 10), c);

        Ok(())
    }

    #[test]
    fn strings() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a, b): (String, String) = conn
            .query_first(
                "select cast('firebird' as varchar(8)), cast('firebird' as char(8)) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!("firebird".to_string(), a);
        assert_eq!("firebird".to_string(), b);

        let (a, b): (String, String) = conn
                .query_first(
                    "select cast('firebird' as varchar(10)), cast('firebird' as char(10)) from rdb$database",
                    (),
                )?
                .unwrap();
        assert_eq!("firebird".to_string(), a);
        assert_eq!("firebird  ".to_string(), b);

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "native_client", not(feature = "pure_rust")))]
    fn unsupported_column_drops_prepared_statement_handle() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        if conn.server_engine()? < EngineVersion::V4 {
            return Ok(());
        }

        conn.execute("set bind of decfloat to native", ())?;

        for attempt in 0..1_100 {
            let result: Result<Option<(f64,)>, FbError> = conn.query_first(
                "select cast(1 as decfloat(34)) from rdb$database",
                (),
            );
            let error = result.expect_err("DECFLOAT(34) result must remain unsupported");
            let message = error.to_string();
            assert!(
                message.contains("Unsupported column type (32762"),
                "unexpected error at attempt {attempt}: {message}"
            );
        }

        let (value,): (i64,) = conn
            .query_first("select 1 from rdb$database", ())?
            .expect("query after prepare errors must return one row");
        assert_eq!(value, 1);

        Ok(())
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::excessive_precision)]
    fn fixed_points() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a, b): (f32, f32) = conn
            .query_first(
                "select cast(100 as numeric(3, 2)), cast(100 as decimal(3, 2)) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(100.0, a);
        assert_eq!(100.0, b);

        let (a, b): (f32, f32) = conn
                .query_first(
                    "select cast(2358.35321 as numeric(5, 5)), cast(2358.35321 as decimal(5, 5)) from rdb$database",
                    ()
                )?
                .unwrap();
        assert_eq!(2358.35321, a);
        assert_eq!(2358.35321, b);

        let (a, b): (f64, f64) = conn
                .query_first(
                    "select cast(2358.78353211234 as numeric(11, 11)), cast(2358.78353211234 as decimal(11, 11)) from rdb$database",
                    ()
                )?
                .unwrap();
        assert_eq!(2358.78353211234, a);
        assert_eq!(2358.78353211234, b);

        Ok(())
    }

    #[test]
    #[allow(clippy::float_cmp)]
    #[cfg(all(feature = "native_client", not(feature = "pure_rust")))]
    fn int128_and_decimal_28_mapping() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        if conn.server_engine()? < EngineVersion::V4 {
            return Ok(());
        }

        conn.execute("set bind of int128 to native", ())?;

        let expected_int128 = 123456789012345678901234567890_i128;
        let (positive, negative): (i128, i128) = conn
            .query_first(
                "select cast(123456789012345678901234567890 as int128), cast(-123456789012345678901234567890 as int128) from rdb$database",
                (),
            )?
            .expect("INT128 query must return one row");
        assert_eq!(positive, expected_int128);
        assert_eq!(negative, -expected_int128);

        let (parameter,): (i128,) = conn
            .query_first("select cast(? as int128) from rdb$database", (expected_int128,))?
            .expect("INT128 parameter query must return one row");
        assert_eq!(parameter, expected_int128);

        let (decimal_28, expected_decimal_28): (f64, f64) = conn
            .query_first(
                "select cast(1.2345 as decimal(28,4)), cast(cast(1.2345 as decimal(28,4)) as double precision) from rdb$database",
                (),
            )?
            .expect("DECIMAL(28,4) query must return one row");
        assert_eq!(decimal_28, expected_decimal_28);

        Ok(())
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn float_points() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a, b): (f32, f64) = conn
            .query_first(
                "select cast(100 as float), cast(100 as double precision) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(100.0, a);
        assert_eq!(100.0, b);

        let (a, b): (f32, f64) = conn
            .query_first(
                "select cast(2358.35 as float), cast(2358.35 as double precision) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(2358.35, a);
        assert_eq!(2358.35, b);

        // We use fixed values instead of f64::MAX/MIN, because the supported ranges in rust and firebird aren't the same.
        let (min, max): (f64, f64) = conn.query_first("select cast(2.225E-300 as double precision), cast(1.797e300 as double precision) from RDB$DATABASE", ())?
                .unwrap();
        assert_eq!(2.225e-300, min);
        assert_eq!(1.797e300, max);

        // We use fixed values instead of f32::MAX/MIN, because the supported ranges in rust and firebird aren't the same.
        let (min, max): (f32, f32) = conn
            .query_first(
                "select cast(1.175E-38 as float), cast(3.402E38 as float) from RDB$DATABASE",
                (),
            )?
            .unwrap();
        assert_eq!(1.175E-38, min);
        assert_eq!(3.402E38, max);

        Ok(())
    }

    #[test]
    fn ints() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (a, b, c): (i32, i16, i64) = conn
            .query_first(
                "select cast(100 as int), cast(100 as smallint), cast(100 as bigint) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(100, a);
        assert_eq!(100, b);
        assert_eq!(100, c);

        let (a, b, c): (i32, i16, i64) = conn
                .query_first(
                    "select cast(2358 as int), cast(2358 as smallint), cast(2358 as bigint) from rdb$database",
                    ()
                )?
                .unwrap();
        assert_eq!(2358, a);
        assert_eq!(2358, b);
        assert_eq!(2358, c);

        let (min, max): (i64, i64) = conn.query_first("select cast(-9223372036854775808 as bigint), cast(9223372036854775807 as bigint) from RDB$DATABASE", ())?
                .unwrap();
        assert_eq!(i64::MIN, min);
        assert_eq!(i64::MAX, max);

        let (min, max): (i32, i32) = conn
            .query_first(
                "select cast(-2147483648 as int), cast(2147483647 as int) from RDB$DATABASE",
                (),
            )?
            .unwrap();
        assert_eq!(i32::MIN, min);
        assert_eq!(i32::MAX, max);

        let (min, max): (i16, i16) = conn
            .query_first(
                "select cast(-32768 as bigint), cast(32767 as bigint) from RDB$DATABASE",
                (),
            )?
            .unwrap();
        assert_eq!(i16::MIN, min);
        assert_eq!(i16::MAX, max);

        Ok(())
    }

    #[test]
    fn lots_of_columns() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let vals = -250..250;

        let sql = format!(
            "select {} from rdb$database",
            vals.clone().fold(String::new(), |mut acc, v| {
                if acc.is_empty() {
                    acc += &format!("{}", v);
                } else {
                    acc += &format!(", {}", v);
                }
                acc
            })
        );

        let resp: Row = conn.query_first(&sql, ())?.expect("No row returned");

        assert_eq!(vals.clone().count(), resp.cols.len());

        for (res, col) in vals.zip(resp.cols) {
            let col: i64 = col.to_val()?;

            assert_eq!(res, col);
        }

        Ok(())
    }
    #[test]
    fn raw_type () -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let row: Row = conn
            .query_first(
                "select cast('firebird' as varchar(8)), cast('firebird' as char(8)) from rdb$database",
                (),
            )?
            .unwrap();
        assert_eq!(2, row.cols.len());

        assert_eq!(448, row.cols.first().unwrap().raw_type);

        Ok(())
    }
}

/// The zero-copy streaming path only exists on the pure-rust backend, so it
/// cannot be generated for every client like the tests above.
#[cfg(feature = "pure_rust")]
mod raw_stream {
    use crate::{prelude::*, FbError, RawValue, Row, SqlType};

    /// 250 rows (crossing the default fetch batch of 200) with one column of each
    /// type the raw path supports, and every fifth row all null.
    const ROWS: &str = "EXECUTE BLOCK RETURNS (n int, s varchar(20), d double precision,
                                               b boolean, ts timestamp) AS
                        DECLARE i int = 0;
                        BEGIN
                          WHILE (i < 250) DO BEGIN
                            i = i + 1;
                            IF (MOD(i, 5) = 0) THEN BEGIN
                              n = NULL; s = NULL; d = NULL; b = NULL; ts = NULL;
                            END ELSE BEGIN
                              n = i;
                              s = 'ação ' || i;
                              d = CAST(i AS DOUBLE PRECISION) / 4;
                              b = MOD(i, 2) = 0;
                              ts = DATEADD(i SECOND TO TIMESTAMP '2020-01-01 12:00:00');
                            END
                            SUSPEND;
                          END
                        END";

    fn render_column(col: &crate::Column) -> Result<String, FbError> {
        Ok(match &col.value {
            SqlType::Null => "null".to_string(),
            SqlType::Text(t) => t.clone(),
            SqlType::Integer(i) => i.to_string(),
            SqlType::Floating(f) => f.to_string(),
            SqlType::Boolean(b) => b.to_string(),
            SqlType::Timestamp(ts) => ts.to_string(),
            other => return Err(format!("unexpected column {:?}", other).into()),
        })
    }

    fn render_raw(value: &RawValue) -> Result<String, FbError> {
        Ok(match value {
            RawValue::Null => "null".to_string(),
            RawValue::Text(bytes) => std::str::from_utf8(bytes)
                .map_err(|e| FbError::from(e.to_string()))?
                .to_string(),
            RawValue::Integer(i) => i.to_string(),
            RawValue::Int128(i) => i.to_string(),
            RawValue::Floating(f) => f.to_string(),
            RawValue::Boolean(b) => b.to_string(),
            RawValue::Timestamp(ts) => ts.to_string(),
        })
    }

    #[test]
    fn stream_raw_matches_the_column_api() -> Result<(), FbError> {
        let mut conn = crate::builder_pure_rust().connect()?;

        let mut expected: Vec<Vec<String>> = Vec::new();
        for row in conn.query_iter::<(), Row>(ROWS, ())? {
            expected.push(
                row?.cols
                    .iter()
                    .map(render_column)
                    .collect::<Result<_, FbError>>()?,
            );
        }

        let mut streamed: Vec<Vec<String>> = Vec::new();
        conn.stream_raw(ROWS, |row| {
            streamed.push(row.iter().map(render_raw).collect::<Result<_, FbError>>()?);

            Ok(())
        })?;

        assert_eq!(250, streamed.len());
        assert_eq!(expected, streamed);

        // The rows above only prove the two paths agree, so make sure they are not
        // agreeing on nothing: every kind must have shown up.
        assert_eq!(
            vec!["1", "ação 1", "0.25", "false", "2020-01-01 12:00:01"],
            streamed[0]
        );
        assert_eq!(vec!["null"; 5], streamed[4]);

        Ok(())
    }

    #[test]
    fn stream_raw_rejects_blob_columns() -> Result<(), FbError> {
        let mut conn = crate::builder_pure_rust().connect()?;

        let res = conn.stream_raw(
            "select cast('abc' as blob sub_type 1) from rdb$database",
            |_| Ok(()),
        );

        assert!(res.is_err(), "a blob column must not be streamed raw");

        Ok(())
    }
}
