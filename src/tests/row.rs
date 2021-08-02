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
            .take(10000)
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
            .take(10000)
            .collect();

        conn.execute("DROP TABLE RBIGBLOBTEXT", ()).ok();
        conn.execute("CREATE TABLE RBIGBLOBTEXT (content blob sub_type 1 character set utf8)", ())?;

        conn.execute("insert into rbigblobtext (content) values (?)", (&rstr,))?;

        let (s,): (String,) = conn.query_first("select content from rbigblobtext", ())?.unwrap();

        assert_eq!(rstr, s);

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
}
