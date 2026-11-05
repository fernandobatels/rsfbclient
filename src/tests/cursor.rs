//! Queries do not consume affected-row counts; execute callers still need them.
mk_tests_default! {
    use crate::{Execute, Queryable, Row, SqlType};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn cursor_queries_preserve_counts_results_and_rollback() -> Result<(), crate::FbError> {
        let mut conn = cbuilder().connect()?;
        let name = format!("CURSOR_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        conn.execute(&format!("CREATE TABLE {name}(ID INTEGER PRIMARY KEY, N INTEGER)"), ())?;
        conn.begin_transaction()?;
        let identity = "SELECT CURRENT_CONNECTION, CURRENT_TRANSACTION FROM RDB$DATABASE";
        let before: Option<(i64, i64)> = conn.query_first(identity, ())?;
        let insert = format!("INSERT INTO {name} VALUES(?, ?)");
        assert_eq!(conn.execute(&insert, (1, 10))?, 1);
        assert_eq!(conn.execute(&insert, (2, 20))?, 1);
        let select = format!("SELECT N FROM {name} WHERE ID=?");
        for id in [1, 2, 1] {
            assert_eq!(conn.query_first::<_, (i32,)>(&select, (id,))?, Some((id * 10,)));
        }
        assert_eq!(conn.execute(&format!("UPDATE {name} SET N=N+1"), ())?, 2);
        assert_eq!(conn.execute(&format!("UPDATE {name} SET N=0 WHERE ID=9"), ())?, 0);
        // Re-execute one cached cursor with NULL and non-NULL bindings.
        for value in [SqlType::Null, SqlType::Integer(3), SqlType::Null] {
            let expected_null = value.is_null();
            let row: Row = conn.query_first("SELECT CAST(? AS INTEGER) FROM RDB$DATABASE", vec![value])?.unwrap();
            assert_eq!(row.cols[0].value.is_null(), expected_null);
        }
        let all = format!("SELECT ID FROM {name} ORDER BY ID");
        {
            let mut rows = conn.query_iter::<_, (i32,)>(&all, ())?;
            assert_eq!(rows.next().unwrap()?, (1,));
        }
        assert_eq!(conn.query::<_, (i32,)>(&all, ())?, [(1,), (2,)]);
        let returned: (i32,) = conn.execute_returnable(&format!("UPDATE {name} SET N=N+1 WHERE ID=2 RETURNING N"), ())?;
        assert_eq!(returned, (22,));
        assert_eq!(conn.execute(&format!("DELETE FROM {name} WHERE ID=?"), (1,))?, 1);
        assert_eq!(conn.execute(&format!("DELETE FROM {name} WHERE ID=?"), (1,))?, 0);
        // An execution/fetch error must not poison the connection or its cache.
        let invalid = "SELECT CAST(? AS INTEGER) FROM RDB$DATABASE";
        assert!(conn.query_first::<_, (i32,)>(invalid, ("invalid",)).is_err());
        assert_eq!(conn.query_first::<_, (i32,)>(invalid, (4,))?, Some((4,)));
        let after: Option<(i64, i64)> = conn.query_first(identity, ())?;
        assert_eq!(before, after, "Query execution must keep the explicit transaction");
        conn.rollback()?;
        assert_eq!(conn.query_first::<_, (i64,)>(&format!("SELECT COUNT(*) FROM {name}"), ())?, Some((0,)));
        // Release cached prepared statements before dropping their table.
        conn.close()?;
        let mut cleanup = cbuilder().connect()?;
        cleanup.execute(&format!("DROP TABLE {name}"), ())?;
        cleanup.close()
    }
}
