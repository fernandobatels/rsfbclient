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
        let mut cursor = conn.open_paged_query(
            "SELECT N AS ID, GEN_ID(RSFB_PAGE_VISITS, 1) AS VISIT FROM (SELECT 1 N FROM RDB$DATABASE UNION ALL SELECT 2 FROM RDB$DATABASE UNION ALL SELECT 3 FROM RDB$DATABASE UNION ALL SELECT 4 FROM RDB$DATABASE) X WHERE N >= CAST(? AS INTEGER) ORDER BY N",
            vec![SqlType::Integer(1)],
        )?;
        assert_eq!(cursor.columns(), ["ID", "VISIT"]);
        assert!(conn.fetch_query_page(&mut cursor, 0).is_err());
        let (first, more) = conn.fetch_query_page(&mut cursor, 2)?;
        assert!(more);
        assert_eq!(first.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [1, 2]);
        assert_eq!(conn.query_first::<_, (i64, i64)>("SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION FROM RDB$DATABASE", ())?, context);
        let (last, more) = conn.fetch_query_page(&mut cursor, 2)?;
        assert!(!more);
        assert_eq!(last.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [3, 4]);
        assert!(conn.fetch_query_page(&mut cursor, 1).is_err());
        assert_eq!(conn.query_first::<_, (i64,)>("SELECT GEN_ID(RSFB_PAGE_VISITS, 0) FROM RDB$DATABASE", ())?, Some((4,)));
        conn.close_paged_query(&mut cursor)?;
        let mut cursor = conn.open_paged_query("SELECT 1 AS EMPTY_VALUE FROM RDB$DATABASE WHERE 1=0", Vec::new())?;
        assert_eq!(cursor.columns(), ["EMPTY_VALUE"]);
        let (empty, more) = conn.fetch_query_page(&mut cursor, 3)?;
        assert!(empty.is_empty() && !more);
        conn.rollback()?;
        assert!(conn.fetch_query_page(&mut cursor, 1).is_err());
        conn.close()?;
        cbuilder().connect()?.execute("DROP SEQUENCE RSFB_PAGE_VISITS", ())?;
        Ok(())
    }

    #[test]
    fn paged_cursors_are_independent_and_bound_to_their_connection_and_transaction() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;
        let mut other = cbuilder().connect()?;
        conn.begin_transaction()?;
        other.begin_transaction()?;
        let mut first = conn.open_paged_query(
            "SELECT 1 AS FIRST_RESULT FROM RDB$DATABASE UNION ALL SELECT 2 FROM RDB$DATABASE ORDER BY 1",
            Vec::new(),
        )?;
        let mut second = conn.open_paged_query(
            "SELECT 20 AS SECOND_RESULT FROM RDB$DATABASE UNION ALL SELECT 21 FROM RDB$DATABASE ORDER BY 1",
            Vec::new(),
        )?;
        assert_eq!(first.columns(), ["FIRST_RESULT"]);
        assert_eq!(second.columns(), ["SECOND_RESULT"]);
        assert!(other.fetch_query_page(&mut first, 1).is_err());
        assert!(other.close_paged_query(&mut second).is_err());
        let (rows, more) = conn.fetch_query_page(&mut first, 1)?;
        assert_eq!((rows[0].get::<i32>(0)?, more), (1, true));
        let (rows, more) = conn.fetch_query_page(&mut second, 1)?;
        assert_eq!((rows[0].get::<i32>(0)?, more), (20, true));
        conn.close_paged_query(&mut first)?;
        assert!(conn.fetch_query_page(&mut first, 1).is_err());
        let (rows, more) = conn.fetch_query_page(&mut second, 1)?;
        assert_eq!((rows[0].get::<i32>(0)?, more), (21, false));

        let mut stale = conn.open_paged_query("SELECT 42 FROM RDB$DATABASE", Vec::new())?;
        conn.commit()?;
        conn.begin_transaction()?;
        assert!(conn.fetch_query_page(&mut stale, 1).is_err());
        assert!(conn.close_paged_query(&mut second).is_err());
        let mut fresh = conn.open_paged_query("SELECT 99 FROM RDB$DATABASE", Vec::new())?;
        assert_eq!(conn.fetch_query_page(&mut fresh, 1)?.0[0].get::<i32>(0)?, 99);
        conn.rollback()?;
        assert!(conn.close_paged_query(&mut fresh).is_err());
        Ok(())
    }

    #[test]
    fn completed_rows_are_delivered_before_a_page_fetch_error() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        conn.begin_transaction()?;
        let mut cursor = conn.open_paged_query(
            "EXECUTE BLOCK RETURNS (N INT) AS BEGIN N=7;SUSPEND;N=8;SUSPEND;N=1/0;SUSPEND;END",
            Vec::new(),
        )?;
        let (rows, more) = conn.fetch_query_page(&mut cursor, 2)?;
        assert_eq!(rows.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [7, 8]);
        assert!(more);
        assert!(matches!(conn.fetch_query_page(&mut cursor, 2), Err(FbError::Sql { .. })));
        assert!(conn.fetch_query_page(&mut cursor, 2).is_err());
        assert_eq!(conn.query_first::<_, (i32,)>("SELECT 42 FROM RDB$DATABASE", ())?, Some((42,)));
        conn.rollback()?;

        let mut columns = Vec::new();
        let mut iter = conn.query_iter_with_columns::<_, Row>("SELECT 1 AS EMPTY_VALUE FROM RDB$DATABASE WHERE 1=0", (), &mut columns)?;
        assert!(iter.next().is_none());
        assert_eq!(columns, ["EMPTY_VALUE"]);
        Ok(())
    }
}
