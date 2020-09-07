//! Charset tests

#[cfg(test)]
/// Counter to allow the tests to be run in parallel without interfering in each other
static TABLE_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

mk_tests_default! {
    use crate::{prelude::*, FbError};

    #[cfg(not(feature = "pure_rust"))] // TODO: fix the pure_rust locking
    #[test]
    fn params() -> Result<(), FbError> {
        use crate::charset::ISO_8859_1;

        let mut conn = cbuilder().charset(ISO_8859_1)
            .connect()?;
        let table_num = super::TABLE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let table = format!("pcharsets{}", table_num);

        conn.execute(
            &format!("RECREATE TABLE {} (a Varchar(30) CHARACTER SET none)", table),
            (),
        )?;

        conn.execute(&format!("insert into {} (a) values (?)", table), ("Pão de queijo",))?;

        let (pao,): (String,) = conn.query_first(&format!("select * from {}", table), ())?
            .unwrap();
        assert_eq!("Pão de queijo", pao);

        let mut conn = cbuilder().connect()?; // utf8

        let err: Result<Option<(String,)>, FbError> = conn.query_first(&format!("select * from {}", table), ());
        assert!(err.is_err());
        assert_eq!("error: Found column with an invalid UTF-8 string: invalid utf-8 sequence of 1 bytes from index 1", err.err().unwrap().to_string());

        Ok(())
    }

    #[test]
    fn column_with_charset_none() -> Result<(), FbError> {
        use crate::charset::ISO_8859_1;

        // This test reproduce the of using a diferent
        // charset of column in insert.
        // When we read an ISO8859_1(or others) data using
        // the UTF8, we will get an "invalid utf-8" error.
        // In this cases, we must use the same charset
        // of data inserted

        let mut conn = cbuilder().connect()?; // utf-8, but is not used
        let table_num = super::TABLE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let table = format!("rcharsets{}", table_num);

        conn.execute(
            &format!("RECREATE TABLE {} (a Varchar(30) CHARACTER SET none)", table),
            (),
        )?;

        conn.execute(&format!("insert into {} (a) values (cast('pão de queijo' as Varchar(30) CHARACTER SET ISO8859_1))", table), ())?;

        let err: Result<Option<(String,)>, FbError> = conn.query_first(&format!("select * from {}", table), ());
        assert!(err.is_err());
        assert_eq!("error: Found column with an invalid UTF-8 string: invalid utf-8 sequence of 1 bytes from index 1", err.err().unwrap().to_string());

        // Hmm, I need use the same charset of inserted content
        let mut conn = cbuilder().charset(ISO_8859_1)
            .connect()?;

        let (pao,): (String,) = conn.query_first(&format!("select * from {}", table), ())?
            .unwrap();

        assert_eq!("pão de queijo", pao);

        Ok(())
    }

    #[test]
    fn stmt_charset() -> Result<(), FbError> {
        use crate::charset::ISO_8859_1;

        let mut conn = cbuilder().charset(ISO_8859_1)
            .connect()?;

        let (pao,): (String,) = conn.query_first("SELECT cast('pão de queijo' as Varchar(30)) FROM RDB$DATABASE;", ())?.unwrap();
        assert_eq!("pão de queijo", pao);

        Ok(())
    }

    #[test]
    fn cast_charsets() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let qsql = "select
                            CAST('olá abc ë' AS VARCHAR(10) CHARACTER SET WIN_1252),
                            CAST('olá abc ë' AS VARCHAR(10) CHARACTER SET ISO8859_1),
                            CAST('olá abc ë' AS VARCHAR(10) CHARACTER SET utf8),
                            CAST('olá abc ë' AS VARCHAR(11) CHARACTER SET none)
                       from RDB$DATABASE;";

        let (win, iso, utf, none,): (String, String, String, String,) = conn.query_first(qsql, ())?
            .unwrap();

        assert_eq!("olá abc ë", win);
        assert_eq!("olá abc ë", iso);
        assert_eq!("olá abc ë", utf);
        assert_eq!("olá abc ë", none);
        assert_eq!(win, iso);
        assert_eq!(win, utf);
        assert_eq!(win, none);

        Ok(())
    }
}
