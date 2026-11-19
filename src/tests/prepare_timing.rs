//! Preparation timing is opt-in and counts prepared statements only.
mk_tests_default! {
    use crate::{prelude::*, FbError, SimpleConnection};

    #[test]
    fn prepare_timing_tracks_misses_errors_and_cache_hits() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        let sql = "SELECT 24 FROM RDB$DATABASE";
        assert_eq!(conn.query_first::<_, (i32,)>(sql, ())?, Some((24,)));
        assert!(conn.last_prepare_duration().is_zero());

        conn.set_prepare_timing_enabled(true);
        let sql = "SELECT 42 FROM RDB$DATABASE";
        assert_eq!(conn.query_first::<_, (i32,)>(sql, ())?, Some((42,)));
        assert!(!conn.last_prepare_duration().is_zero());
        assert_eq!(conn.query_first::<_, (i32,)>(sql, ())?, Some((42,)));
        assert!(conn.last_prepare_duration().is_zero());

        assert!(conn.execute("SELECT FROM", ()).is_err());
        assert!(!conn.last_prepare_duration().is_zero());

        conn.set_prepare_timing_enabled(false);
        assert_eq!(conn.query_first::<_, (i32,)>("SELECT 43 FROM RDB$DATABASE", ())?, Some((43,)));
        assert!(conn.last_prepare_duration().is_zero());
        Ok(())
    }
}
