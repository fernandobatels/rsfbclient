//!
//! Rust Firebird Client
//!
//! Parameter tests
//!

mk_tests_default! {
    use crate::{prelude::*, FbError, SqlType, EngineVersion, SystemInfos};
    use chrono::{NaiveDate, NaiveTime};
    use rand::{distributions::Standard, Rng};

    #[test]
    fn optional_named_support() -> Result<(), FbError> {
        let exec_block_select : &str = "
            EXECUTE BLOCK RETURNS (outval bigint) as
            declare loopvar int = 0;
            begin
                while (loopvar < 100) do begin
                    for select
                        :loopvar
                    from
                        rdb$database
                    into
                        :outval
                    do begin
                        loopvar = loopvar + 1;
                        suspend;
                    end
                end
            end;";

        let mut conn = cbuilder().connect()?;

        let rows = conn.query::<(), (i64,)>(exec_block_select,())?;

        assert_eq!(100, rows.len());

        Ok(())
    }

    #[test]
    fn struct_namedparams_optional() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PNAMED_TEST", ()).ok();
        conn.execute("CREATE TABLE PNAMED_TEST (id int, num1 int, str1 varchar(50))", ())?;

        #[derive(Clone, IntoParams)]
        struct ParamTest {
            pub num1: Option<i32>,
            pub str1: Option<String>
        };

        let ptest = ParamTest {
            num1: Some(10),
            str1: None
        };

        let res1: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 10 = :num1",
            ptest.clone(),
        )?;
        assert!(res1.is_some());

        conn.execute("insert into pnamed_test (id, str1) values (1, :str1)", ptest.clone())?;
        conn.execute("insert into pnamed_test (id, num1) values (2, :num1)", ptest)?;

        let res2: Option<(i32,)> = conn.query_first("select 1 from pnamed_test where id = 1 and str1 is null", ())?;
        assert!(res2.is_some());

        let res3: Option<(i32,)> = conn.query_first("select 1 from pnamed_test where id = 2 and num1 is not null", ())?;
        assert!(res3.is_some());

        Ok(())
    }

    #[test]
    fn struct_namedparams_insert() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        conn.execute("DROP TABLE PNAMED_USER", ()).ok();
        conn.execute("CREATE TABLE PNAMED_USER (name varchar(50), age int)", ())?;

        #[derive(Clone, IntoParams)]
        struct User {
            pub name: String,
            pub age: i32
        };

        let user1 = User {
            name: "Pedro".to_string(),
            age: 20
        };

        conn.execute("insert into pnamed_user (name, age) values (:name, :age)", user1.clone())?;

        let suser1: Option<(String,i32,)> = conn.query_first(
            "select name, age from pnamed_user where age >= :age",
            user1,
        )?;
        assert!(suser1.is_some());
        assert_eq!("Pedro", suser1.unwrap().0);

        Ok(())
    }

    #[test]
    fn struct_namedparams() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        #[derive(Clone, IntoParams)]
        struct ParamTest {
            pub num: i32,
            pub num2: f64,
            pub str1: String
        };

        let ptest = ParamTest {
            num: 10,
            num2: 11.11,
            str1: "olá mundo".to_string()
        };

        let res1: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 10 = :num ",
            ptest.clone(),
        )?;
        assert!(res1.is_some());

        let res2: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 11.11 = :num2 ",
            ptest.clone(),
        )?;
        assert!(res2.is_some());

        let res3: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 10 = :num and 11.11 = :num2 ",
            ptest.clone(),
        )?;
        assert!(res3.is_some());

        let res4: Option<(i32,)> = conn.query_first(
            "select 1 from rdb$database where 'olá mundo' = :str1 ",
            ptest,
        )?;
        assert!(res4.is_some());

        Ok(())
    }

    #[test]
    fn boolean() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        if conn.server_engine()? <= EngineVersion::V2 {
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

        let params: Vec<SqlType> = vals.clone().map(|v| v.into()).collect();

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
