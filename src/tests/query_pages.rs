//! Paged reads keep the same server cursor and transaction across calls.
mk_tests_default! {
    use crate::{FbError, Row, SimpleConnection, SqlType, prelude::*};

    #[test]
    fn retained_pages_keep_order_metadata_and_transaction() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        conn.execute("CREATE SEQUENCE RSFB_PAGE_VISITS", ())?;
        assert!(conn.query_paged("SELECT 1 FROM RDB$DATABASE", Vec::new()).is_err());
        conn.begin_transaction()?;
        let context: Option<(i64, i64)> = conn.query_first("SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION FROM RDB$DATABASE", ())?;
        let mut cursor = conn.query_paged(
            "SELECT N AS ID, GEN_ID(RSFB_PAGE_VISITS, 1) AS VISIT FROM (SELECT 1 N FROM RDB$DATABASE UNION ALL SELECT 2 FROM RDB$DATABASE UNION ALL SELECT 3 FROM RDB$DATABASE UNION ALL SELECT 4 FROM RDB$DATABASE) X WHERE N >= CAST(? AS INTEGER) ORDER BY N",
            vec![SqlType::Integer(1)],
        )?;
        assert_eq!(cursor.columns(), ["ID", "VISIT"]);
        assert!(cursor.fetch(0).is_err());
        let (first, more) = cursor.fetch(2)?;
        assert!(more);
        assert_eq!(first.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [1, 2]);
        cursor.check_error()?;
        let paused = cursor.pause();
        assert_eq!(conn.query_first::<_, (i64, i64)>("SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION FROM RDB$DATABASE", ())?, context);
        let mut cursor = conn.resume_paged_query(paused);
        let (last, more) = cursor.fetch(2)?;
        assert!(!more);
        assert_eq!(last.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [3, 4]);
        assert!(cursor.fetch(1).is_err());
        cursor.close()?;
        assert_eq!(conn.query_first::<_, (i64,)>("SELECT GEN_ID(RSFB_PAGE_VISITS, 0) FROM RDB$DATABASE", ())?, Some((4,)));
        let mut cursor = conn.query_paged("SELECT 1 AS EMPTY_VALUE FROM RDB$DATABASE WHERE 1=0", Vec::new())?;
        assert_eq!(cursor.columns(), ["EMPTY_VALUE"]);
        let (empty, more) = cursor.fetch(3)?;
        assert!(empty.is_empty() && !more);
        let paused = cursor.pause();
        conn.rollback()?;
        assert!(conn.resume_paged_query(paused).fetch(1).is_err());
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
        let mut first = conn.query_paged(
            "SELECT 1 AS FIRST_RESULT FROM RDB$DATABASE UNION ALL SELECT 2 FROM RDB$DATABASE ORDER BY 1",
            Vec::new(),
        )?;
        assert_eq!(first.columns(), ["FIRST_RESULT"]);
        let (rows, more) = first.fetch(1)?;
        assert_eq!((rows[0].get::<i32>(0)?, more), (1, true));
        let first = first.pause();

        let mut second = conn.query_paged(
            "SELECT 20 AS SECOND_RESULT FROM RDB$DATABASE UNION ALL SELECT 21 FROM RDB$DATABASE ORDER BY 1",
            Vec::new(),
        )?;
        assert_eq!(second.columns(), ["SECOND_RESULT"]);
        let (rows, more) = second.fetch(1)?;
        assert_eq!((rows[0].get::<i32>(0)?, more), (20, true));
        let second = second.pause();

        let mut wrong_connection = other.resume_paged_query(first);
        assert!(wrong_connection.fetch(1).is_err());
        let first = wrong_connection.pause();
        conn.resume_paged_query(first).close()?;
        let mut second = conn.resume_paged_query(second);
        let (rows, more) = second.fetch(1)?;
        assert_eq!((rows[0].get::<i32>(0)?, more), (21, false));
        let stale = second.pause();
        conn.commit()?;
        conn.begin_transaction()?;
        assert!(conn.resume_paged_query(stale).fetch(1).is_err());

        let mut fresh = conn.query_paged("SELECT 99 FROM RDB$DATABASE", Vec::new())?;
        assert_eq!(fresh.fetch(1)?.0[0].get::<i32>(0)?, 99);
        drop(fresh);
        conn.rollback()?;
        Ok(())
    }

    #[test]
    fn completed_rows_are_delivered_before_a_page_fetch_error() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        conn.begin_transaction()?;
        let mut cursor = conn.query_paged(
            "EXECUTE BLOCK RETURNS (N INT) AS BEGIN N=7;SUSPEND;N=8;SUSPEND;N=1/0;SUSPEND;END",
            Vec::new(),
        )?;
        let (rows, more) = cursor.fetch(2)?;
        assert_eq!(rows.into_iter().map(|row| row.get::<i32>(0)).collect::<Result<Vec<_>, _>>()?, [7, 8]);
        assert!(more);
        assert!(matches!(cursor.fetch(2), Err(FbError::Sql { .. })));
        assert!(cursor.fetch(2).is_err());
        drop(cursor);
        assert_eq!(conn.query_first::<_, (i32,)>("SELECT 42 FROM RDB$DATABASE", ())?, Some((42,)));
        conn.rollback()?;

        conn.begin_transaction()?;
        let mut cursor = conn.query_paged(
            "EXECUTE BLOCK RETURNS (N INT) AS BEGIN N=7;SUSPEND;N=1/0;SUSPEND;END",
            Vec::new(),
        )?;
        let (rows, more) = cursor.fetch(2)?;
        assert_eq!(rows[0].get::<i32>(0)?, 7);
        assert!(more);
        let paused = cursor.pause();
        let mut other = conn.query_paged("SELECT 43 FROM RDB$DATABASE", Vec::new())?;
        other.check_error()?;
        let other = other.pause();
        let mut cursor = conn.resume_paged_query(paused);
        assert!(matches!(cursor.check_error(), Err(FbError::Sql { .. })));
        assert!(cursor.fetch(1).is_err());
        drop(cursor);
        let mut other = conn.resume_paged_query(other);
        assert_eq!(other.fetch(1)?.0[0].get::<i32>(0)?, 43);
        other.close()?;
        conn.rollback()?;

        let mut columns = Vec::new();
        let mut iter = conn.query_iter_with_columns::<_, Row>("SELECT 1 AS EMPTY_VALUE FROM RDB$DATABASE WHERE 1=0", (), &mut columns)?;
        assert!(iter.next().is_none());
        assert_eq!(columns, ["EMPTY_VALUE"]);
        Ok(())
    }
}
