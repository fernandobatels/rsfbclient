//!
//! Rust Firebird Client
//!
//! Parameter tests
//!

mk_tests_default! {
    use crate::{prelude::*, FbError, Param};
    use chrono::{NaiveDate, NaiveTime};
    use rand::{distributions::Standard, Rng};

    #[test]
    fn struct_namedparams() -> Result<(), FbError> {

        let mut conn = cbuilder().connect()?;

        struct ParamTest {
            pub num: i32
        };

        // TODO: remove this!
        impl rsfbclient_core::IntoParams for ParamTest {
            fn to_params(self) -> Vec<rsfbclient_core::Param> {
                vec![rsfbclient_core::Param::Integer(self.num.into(), Some("num".to_string()))]
            }
        };

        let ptest = ParamTest {
            num: 10
        };

        let res: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 10 = :num ",
            ptest,
        )?;

        assert!(res.is_some());

        Ok(())
    }

    #[test]
    fn boolean() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (engine_version,): (String,) = conn.query_first(
            "SELECT rdb$get_context('SYSTEM', 'ENGINE_VERSION') from rdb$database;",
            (),
        )?.unwrap();
        if engine_version.starts_with("2.") {
            return Ok(());
        }

        let res: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where true = ? ",
            (true,),
        )?;
        assert!(res.is_some());


        let res: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where true = ? ",
            (false,),
        )?;
        assert!(res.is_none());

        Ok(())
    }

    #[test]
    fn blob_binary_subtype() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PBLOBBIN", ()).ok();
        conn.execute("CREATE TABLE PBLOBBIN (content blob sub_type 0)", ())?;

        let bin: Vec<u8> = Vec::from("abc äbç 123".as_bytes());
        conn.execute("insert into pblobbin (content) values (?)", (bin,))?;
        let val_exists: Option<(i16,)> = conn.query_first("select 1 from pblobbin where content = x'61626320c3a462c3a720313233'", ())?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn blob_text_subtype() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PBLOBTEXT", ()).ok();
        conn.execute("CREATE TABLE PBLOBTEXT (content blob sub_type 1)", ())?;

        conn.execute("insert into pblobtext (content) values (?)", ("abc äbç 123",))?;
        let val_exists: Option<(i16,)> = conn.query_first("select 1 from pblobtext where content = 'abc äbç 123'", ())?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn big_blob_binary() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let rstr: Vec<u8> = rand::thread_rng()
            .sample_iter::<u8, _>(Standard)
            .take(10000)
            .collect();

        conn.execute("DROP TABLE PBIGBLOBBIN", ()).ok();
        conn.execute("CREATE TABLE PBIGBLOBBIN (content blob sub_type 0)", ())?;

        conn.execute("insert into pbigblobbin (content) values (?)", (rstr,))?;

        Ok(())
    }

    #[test]
    fn big_blob_text() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let rstr: String = rand::thread_rng()
            .sample_iter::<char, _>(Standard)
            .take(10000)
            .collect();

        conn.execute("DROP TABLE PBIGBLOBTEXT", ()).ok();
        conn.execute("CREATE TABLE PBIGBLOBTEXT (content blob sub_type 1)", ())?;

        conn.execute("insert into pbigblobtext (content) values (?)", (rstr,))?;

        Ok(())
    }

    #[test]
    fn dates() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PDATES", ()).ok();
        conn.execute(
            "CREATE TABLE PDATES (ref char(1), a date, b timestamp, c time)",
            (),
        )?;

        conn.execute(
            "insert into pdates (ref, a) values ('a', ?)",
            (NaiveDate::from_ymd(2009, 8, 7),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'a' and a = '2009-08-07'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pdates (ref, b) values ('b', ?)",
            (NaiveDate::from_ymd(2009, 8, 7).and_hms(11, 32, 25),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'b' and b = '2009-08-07 11:32:25'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pdates (ref, c) values ('c', ?)",
            (NaiveTime::from_hms(11, 22, 33),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'c' and c = '11:22:33'",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn strings() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PSTRINGS", ()).ok();
        conn.execute(
            "CREATE TABLE PSTRINGS (ref char(1), a varchar(10), b varchar(10))",
            (),
        )?;

        conn.execute(
            "insert into pstrings (ref, a) values ('a', ?)",
            ("firebird",),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pstrings where ref = 'a' and a = 'firebird'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pstrings (ref, b) values ('b', ?)",
            ("firebird",),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pstrings where ref = 'b' and b = 'firebird  '",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn fixed_points() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PFIXEDS", ()).ok();
        conn.execute(
            "CREATE TABLE PFIXEDS (ref char(1), a numeric(2, 2), b decimal(2, 2))",
            (),
        )?;

        conn.execute("insert into pfixeds (ref, a) values ('a', ?)", (22.33,))?;
        let val_exists: Option<(i16,)> =
            conn.query_first("select 1 from pfixeds where ref = 'a' and a = 22.33", ())?;
        assert!(val_exists.is_some());

        conn.execute("insert into pfixeds (ref, b) values ('b', ?)", (22.33,))?;
        let val_exists: Option<(i16,)> =
            conn.query_first("select 1 from pfixeds where ref = 'b' and b = 22.33", ())?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn float_points() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PFLOATS", ()).ok();
        conn.execute(
            "CREATE TABLE PFLOATS (ref char(1), a float, b double precision)",
            (),
        )?;

        conn.execute("insert into pfloats (ref, a) values ('a', ?)", (3.402E38,))?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pfloats where ref = 'a' and a = cast(3.402E38 as float)",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pfloats (ref, b) values ('b', ?)",
            (2.225e-300,),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pfloats where ref = 'b' and b = 2.225E-300",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn ints() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PINTEGERS", ()).ok();
        conn.execute(
            "CREATE TABLE PINTEGERS (ref char(1), a smallint, b int, c bigint)",
            (),
        )?;

        conn.execute(
            "insert into pintegers (ref, a) values ('a', ?)",
            (i16::MIN,),
        )?;
        let val_exists: Option<(i16,)> =
            conn.query_first("select 1 from pintegers where ref = 'a' and a = -32768", ())?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pintegers (ref, b) values ('b', ?)",
            (i32::MIN,),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pintegers where ref = 'b' and b = -2147483648",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pintegers (ref, c) values ('c', ?)",
            (i64::MIN,),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pintegers where ref = 'c' and c = -9223372036854775808",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn null() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let res: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 1 = ? ",
            (Option::<i32>::None,),
        )?;

        assert!(res.is_none());

        Ok(())
    }

    #[test]
    fn lots_of_params() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let vals = -250..250;

        let params: Vec<Param> = vals.clone().map(|v| v.into()).collect();

        let sql = format!("select 1 from rdb$database where {}", vals.fold(String::new(), |mut acc, v| {
            if acc.is_empty() {
                acc += &format!("{} = ?", v);
            }else{
                acc += &format!(" and {} = ?", v);
            }
            acc
        }));

        let resp = conn.query_first(&sql, params)?;

        assert_eq!(resp, Some((1,)));

        Ok(())
    }
}
