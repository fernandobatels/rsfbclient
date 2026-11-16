//! Paged reads keep the same server cursor and transaction across calls.
mk_tests_default! {
    use crate::{FbError, Row, SimpleConnection, SqlType, prelude::*};

    #[test]
    fn retained_pages_keep_order_metadata_and_transaction() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        conn.execute("CREATE SEQUENCE RSFB_PAGE_VISITS", ())?;
        assert!(conn.open_paged_query("SELECT 1 FROM RDB$DATABASE", Vec::new()).is_err());
        conn.begin_transaction()?;
        let context: Option<(i64, i64)> = conn.query_first("SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION FROM RDB$DATABASE", ())?;
        let names = conn.open_paged_query(
            "SELECT N AS ID, GEN_ID(RSFB_PAGE_VISITS, 1) AS VISIT FROM (SELECT 1 N FROM RDB$DATABASE UNION ALL SELECT 2 FROM RDB$DATABASE UNION ALL SELECT 3 FROM RDB$DATABASE UNION ALL SELECT 4 FROM RDB$DATABASE) X WHERE N >= CAST(? AS INTEGER) ORDER BY N",
            vec![SqlType::Integer(1)],
        )?;
        assert_eq!(names, ["ID", "VISIT"]);
        assert!(conn.fetch_query_page(0).is_err());
        let (first, more) = conn.fetch_query_page(2)?;
        assert!(more);
        assert_eq!(first.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [1, 2]);
        assert_eq!(conn.query_first::<_, (i64, i64)>("SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION FROM RDB$DATABASE", ())?, context);
        let (last, more) = conn.fetch_query_page(2)?;
        assert!(!more);
        assert_eq!(last.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [3, 4]);
        assert!(conn.fetch_query_page(1).is_err());
        assert_eq!(conn.query_first::<_, (i64,)>("SELECT GEN_ID(RSFB_PAGE_VISITS, 0) FROM RDB$DATABASE", ())?, Some((4,)));
        assert_eq!(conn.open_paged_query("SELECT 1 AS EMPTY_VALUE FROM RDB$DATABASE WHERE 1=0", Vec::new())?, ["EMPTY_VALUE"]);
        let (empty, more) = conn.fetch_query_page(3)?;
        assert!(empty.is_empty() && !more);
        conn.rollback()?;
        assert!(conn.fetch_query_page(1).is_err());
        conn.close()?;
        cbuilder().connect()?.execute("DROP SEQUENCE RSFB_PAGE_VISITS", ())?;
        Ok(())
    }

    #[test]
    fn completed_rows_are_delivered_before_a_page_fetch_error() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        conn.begin_transaction()?;
        conn.open_paged_query(
            "EXECUTE BLOCK RETURNS (N INT) AS BEGIN N=7;SUSPEND;N=8;SUSPEND;N=1/0;SUSPEND;END",
            Vec::new(),
        )?;
        let (rows, more) = conn.fetch_query_page(2)?;
        assert_eq!(rows.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [7, 8]);
        assert!(more);
        assert!(matches!(conn.fetch_query_page(2), Err(FbError::Sql { .. })));
        assert!(conn.fetch_query_page(2).is_err());
        assert_eq!(conn.query_first::<_, (i32,)>("SELECT 42 FROM RDB$DATABASE", ())?, Some((42,)));
        conn.rollback()?;

        let mut columns = Vec::new();
        let mut iter = conn.query_iter_with_columns::<_, Row>("SELECT 1 AS EMPTY_VALUE FROM RDB$DATABASE WHERE 1=0", (), &mut columns)?;
        assert!(iter.next().is_none());
        assert_eq!(columns, ["EMPTY_VALUE"]);
        Ok(())
    }
}
