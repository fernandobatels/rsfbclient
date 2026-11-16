//!
//! Rust Firebird Client
//!
//! Transaction struct tests
//!

mk_tests_default! {
    use crate::{FbError, Connection, Transaction, query::Queryable, EngineVersion, SystemInfos};
    use rsfbclient_core::*;

    macro_rules! recreate_tbl_fmtstring{
        () => {"recreate table {} ( id INT NOT NULL PRIMARY KEY, description VARCHAR(20) );"};
    }
    macro_rules! drop_tbl_fmtstring{
        () => {"drop table {};"};
    }
    macro_rules! insert_stmt_fmtstring{
        () => {"insert into {} (id, description) values (543210, 'testing');"};
    }
    macro_rules! select_stmt_fmtstring{
        () => {"select * from {};"};
    }

    fn setup<C: FirebirdClient>( conn: &mut Connection<C>, table_name: &str ) ->  Result<(), FbError>{
        let mut setup_transaction = Transaction::new(conn, TransactionConfiguration::default())?;
        setup_transaction.execute_immediate( format!(recreate_tbl_fmtstring!(), table_name).as_str() )?;
        setup_transaction.commit()
    }

    fn teardown<C: FirebirdClient>( conn: Connection<C>, table_name: &str ) -> Result<(), FbError> {
        let mut conn = conn;
        let mut setup_transaction = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        setup_transaction.execute_immediate( format!(drop_tbl_fmtstring!(), table_name ).as_str() )?;
        setup_transaction.commit()?;

        conn.close()
    }

    #[test]
    fn automatic_queries_release_their_transactions() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;

        let (first_transaction,): (i64,) = conn
            .query_first("select current_transaction from rdb$database", ())?
            .unwrap();
        let (second_transaction,): (i64,) = conn
            .query_first("select current_transaction from rdb$database", ())?
            .unwrap();

        assert_ne!(first_transaction, second_transaction);
        Ok(())
    }

    #[test]
    fn explicit_connection_transaction_is_reused_until_commit() -> Result<(), FbError> {
        let mut conn = cbuilder().connect()?;
        conn.begin_transaction()?;

        let (first_transaction,): (i64,) = conn
            .query_first("select current_transaction from rdb$database", ())?
            .unwrap();
        let (second_transaction,): (i64,) = conn
            .query_first("select current_transaction from rdb$database", ())?
            .unwrap();

        assert_eq!(first_transaction, second_transaction);
        conn.commit()?;

        let (next_transaction,): (i64,) = conn
            .query_first("select current_transaction from rdb$database", ())?
            .unwrap();
        assert_ne!(first_transaction, next_transaction);
        Ok(())
    }

    #[test]
    fn default_transaction_can_commit_and_roll_back_with_or_without_retaining() -> Result<(), FbError> {
        use crate::{Execute, SimpleConnection};
        let mut driver = cbuilder().connect()?;
        setup(&mut driver, "RSFB_TX_LIFECYCLE")?;
        let mut writer: SimpleConnection = driver.into();
        let mut observer = cbuilder().connect()?;
        let id_sql = "SELECT CURRENT_TRANSACTION FROM RDB$DATABASE";
        let context_sql = "SELECT RDB$GET_CONTEXT('USER_TRANSACTION','RSFB_TX_TEST') FROM RDB$DATABASE";
        let ids_sql = "SELECT ID FROM RSFB_TX_LIFECYCLE ORDER BY ID";

        writer.commit_transaction()?;
        writer.rollback_retaining()?;
        writer.begin_transaction()?;
        writer.query_first::<_, (i32,)>(
            "SELECT RDB$SET_CONTEXT('USER_TRANSACTION','RSFB_TX_TEST','kept') FROM RDB$DATABASE",
            (),
        )?;
        writer.execute("INSERT INTO RSFB_TX_LIFECYCLE (ID) VALUES (1)", ())?;
        assert!(observer.query::<_, (i32,)>(ids_sql, ())?.is_empty());
        writer.commit_retaining()?;
        assert_eq!(writer.query_first::<_, (String,)>(context_sql, ())?, Some(("kept".into(),)));
        assert_eq!(observer.query::<_, (i32,)>(ids_sql, ())?, [(1,)]);

        writer.execute("INSERT INTO RSFB_TX_LIFECYCLE (ID) VALUES (2)", ())?;
        writer.rollback_retaining()?;
        assert_eq!(writer.query_first::<_, (String,)>(context_sql, ())?, Some(("kept".into(),)));
        assert_eq!(writer.query::<_, (i32,)>(ids_sql, ())?, [(1,)]);

        let before_commit: Option<(i64,)> = writer.query_first(id_sql, ())?;
        writer.execute("INSERT INTO RSFB_TX_LIFECYCLE (ID) VALUES (3)", ())?;
        writer.commit_transaction()?;
        assert_eq!(observer.query::<_, (i32,)>(ids_sql, ())?, [(1,), (3,)]);
        writer.begin_transaction()?;
        let next: Option<(i64,)> = writer.query_first(id_sql, ())?;
        assert_ne!(next, before_commit);
        writer.execute("INSERT INTO RSFB_TX_LIFECYCLE (ID) VALUES (4)", ())?;
        writer.rollback_transaction()?;
        assert_ne!(writer.query_first::<_, (i64,)>(id_sql, ())?, next);
        assert_eq!(observer.query::<_, (i32,)>(ids_sql, ())?, [(1,), (3,)]);

        writer.close()?;
        observer.close()?;
        teardown(cbuilder().connect()?, "RSFB_TX_LIFECYCLE")
    }

    #[test]
    fn declining_a_transaction_configuration_change_keeps_pending_work() -> Result<(), FbError> {
        use crate::Execute;
        let mut conn = cbuilder().connect()?;
        setup(&mut conn, "RSFB_TX_CONFIG")?;
        let original = TransactionConfiguration {
            isolation: TrIsolationLevel::Concurrency,
            ..TransactionConfiguration::default()
        };
        let different = TransactionConfiguration {
            data_access: TrDataAccessMode::ReadOnly,
            ..original
        };
        conn.begin_transaction_config(original)?;
        let id_sql = "SELECT CURRENT_TRANSACTION FROM RDB$DATABASE";
        let first: Option<(i64,)> = conn.query_first(id_sql, ())?;
        conn.execute("INSERT INTO RSFB_TX_CONFIG (ID) VALUES (1)", ())?;
        assert!(conn.begin_transaction_config(different).is_err());
        assert!(conn.with_transaction_config(different, |_| -> Result<(), FbError> {
            panic!("an incompatible transaction must reject the closure before it runs");
        }).is_err());
        conn.begin_transaction_config(original)?;
        assert_eq!(conn.query_first::<_, (i64,)>(id_sql, ())?, first);
        assert_eq!(conn.query_first::<_, (i32,)>("SELECT ID FROM RSFB_TX_CONFIG", ())?, Some((1,)));
        conn.rollback_transaction()?;

        conn.begin_transaction_config(different)?;
        assert_ne!(conn.query_first::<_, (i64,)>(id_sql, ())?, first);
        assert!(conn.execute("INSERT INTO RSFB_TX_CONFIG (ID) VALUES (2)", ()).is_err());
        assert!(conn.query::<_, (i32,)>("SELECT ID FROM RSFB_TX_CONFIG", ())?.is_empty());
        conn.rollback_transaction()?;
        conn.close()?;
        teardown(cbuilder().connect()?, "RSFB_TX_CONFIG")
    }

    #[test]
    fn failed_default_commit_allows_an_explicit_rollback() -> Result<(), FbError> {
        use crate::Execute;
        let mut writer = cbuilder().connect()?;
        setup(&mut writer, "RSFB_TX_FAIL_COMMIT")?;
        writer.execute("CREATE EXCEPTION RSFB_TX_ABORT 'Commit rejected by the fixture'", ())?;
        writer.execute(
            "CREATE TRIGGER RSFB_TX_CHECK_COMMIT ACTIVE ON TRANSACTION COMMIT AS
             BEGIN
               IF (RDB$GET_CONTEXT('USER_TRANSACTION','RSFB_TX_REJECT') = 'YES') THEN
                 EXCEPTION RSFB_TX_ABORT;
             END",
            (),
        )?;
        writer.begin_transaction()?;
        writer.execute("INSERT INTO RSFB_TX_FAIL_COMMIT (ID) VALUES (1)", ())?;
        writer.query_first::<_, (i32,)>(
            "SELECT RDB$SET_CONTEXT('USER_TRANSACTION','RSFB_TX_REJECT','YES') FROM RDB$DATABASE",
            (),
        )?;
        assert!(writer.commit_transaction().unwrap_err().to_string().contains("Commit rejected by the fixture"));
        assert_eq!(writer.query_first::<_, (i32,)>("SELECT ID FROM RSFB_TX_FAIL_COMMIT", ())?, Some((1,)));
        writer.rollback_transaction()?;
        writer.close()?;

        let mut observer = cbuilder().connect()?;
        assert!(observer.query::<_, (i32,)>("SELECT ID FROM RSFB_TX_FAIL_COMMIT", ())?.is_empty());
        observer.close()?;
        let mut cleanup = cbuilder().connect()?;
        cleanup.execute("DROP TRIGGER RSFB_TX_CHECK_COMMIT", ())?;
        cleanup.execute("DROP EXCEPTION RSFB_TX_ABORT", ())?;
        teardown(cleanup, "RSFB_TX_FAIL_COMMIT")
    }

    #[test]
    fn recreate_insert_drop_with_commit() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS0";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _insert_result  = transaction.execute_immediate( format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str() );
        let commit_result   = transaction.commit();

        teardown(conn, TABLE_NAME)?;
        commit_result
    }

    #[test]
    fn recreate_insert_drop_with_commit_retaining() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS1";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _insert_result  = transaction.execute_immediate( format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str() );
        let commit_result   = transaction.commit_retaining();
        drop(transaction);

        teardown(conn, TABLE_NAME)?;
        commit_result
    }

    #[test]
    fn recreate_insert_drop_with_rollback() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS2";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _insert_result  = transaction.execute_immediate( format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str() );
        let rollback_result = transaction.rollback();

        teardown(conn, TABLE_NAME)?;
        rollback_result
    }

    #[test]
    fn recreate_insert_drop_with_rollback_retaining() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS3";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _insert_result  = transaction.execute_immediate( format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str() );
        let rollback_result = transaction.rollback_retaining();
        drop(transaction);

        teardown(conn, TABLE_NAME)?;

        rollback_result
    }

    #[test]
    fn select_readcommited_with_nowait() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS4";

        let mut conn = cbuilder().connect()?;
        if conn.server_engine()? != EngineVersion::V3 {
            return Ok(());
        }
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction1 = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _ = transaction1.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str())?;

        let mut conn2 = cbuilder().connect()?;
        let mut transaction2 = Transaction::new(&mut conn2, TransactionConfiguration {
            lock_resolution: TrLockResolution::NoWait,
            ..Default::default()
        })?;
        let qr: Result<Vec<(i32,)>, FbError> = transaction2.query(format!(select_stmt_fmtstring!(), TABLE_NAME).as_str(), ());

        assert!(qr.is_err());
        let e = qr.err().unwrap().to_string();
        assert!(e.contains("deadlock") && e.contains("read conflicts with concurrent update"), "received: {}", e);

        drop(transaction2);
        drop(transaction1);
        conn2.close()?;
        teardown(conn, TABLE_NAME)
    }

    #[test]
    #[ignore]
    fn select_readcommited_with_wait_for_some_seconds() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS5";

        let mut conn = cbuilder().connect()?;
        if conn.server_engine()? != EngineVersion::V3 {
            return Ok(());
        }
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction1 = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _ = transaction1.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str())?;

        let mut conn2 = cbuilder().connect()?;
        let mut transaction2 = Transaction::new(&mut conn2, TransactionConfiguration {
            lock_resolution: TrLockResolution::Wait(Some(2)),
            ..Default::default()
        })?;
        let qr: Result<Vec<(i32,)>, FbError> = transaction2.query(format!(select_stmt_fmtstring!(), TABLE_NAME).as_str(), ());

        assert!(qr.is_err());
        let e = qr.err().unwrap().to_string();
        assert!(e.contains("deadlock") && e.contains("read conflicts with concurrent update"), "received: {}", e);

        drop(transaction2);
        drop(transaction1);
        conn2.close()?;
        teardown(conn, TABLE_NAME)
    }

    #[test]
    fn insert_with_readonly_transaction() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS5";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction1 = Transaction::new(&mut conn, TransactionConfiguration {
            data_access: TrDataAccessMode::ReadOnly,
            ..Default::default()
        })?;
        let qr = transaction1.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str());

        assert!(qr.is_err());
        assert!(qr.err().unwrap().to_string().contains("attempted update during read-only transaction"));

        drop(transaction1);
        teardown(conn, TABLE_NAME)
    }

    #[test]
    fn select_concurrency_isolation() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS6";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut conn2 = cbuilder().connect()?;
        let mut transaction2 = Transaction::new(&mut conn2, TransactionConfiguration {
            isolation: TrIsolationLevel::Concurrency,
            ..Default::default()
        })?;

        let mut transaction1 = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _ = transaction1.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str())?;

        let rows: Vec<(i32,)> = transaction2.query(format!(select_stmt_fmtstring!(), TABLE_NAME).as_str(), ())?;
        assert_eq!(0, rows.len());

        drop(transaction2);
        drop(transaction1);
        conn2.close()?;
        teardown(conn, TABLE_NAME)
    }

    #[test]
    fn select_consistency_isolation() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS7";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction1 = Transaction::new(&mut conn, TransactionConfiguration {
            isolation: TrIsolationLevel::Consistency,
            lock_resolution: TrLockResolution::NoWait,
            ..Default::default()
        })?;

        let _ = transaction1.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str())?;

        let mut conn2 = cbuilder().connect()?;
        let mut transaction2 = Transaction::new(&mut conn2, TransactionConfiguration {
            isolation: TrIsolationLevel::Concurrency,
            lock_resolution: TrLockResolution::NoWait,
            ..Default::default()
        })?;

        let rows: Vec<(i32,)> = transaction2.query(format!(select_stmt_fmtstring!(), TABLE_NAME).as_str(), ())?;
        assert_eq!(0, rows.len());

        let qr = transaction2.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str());
        assert!(qr.is_err());
        let mut e = qr.err().unwrap().to_string();
        e.truncate(52);
        assert!(e.contains("lock conflict on no wait transaction"));

        drop(transaction2);
        drop(transaction1);
        conn2.close()?;
        teardown(conn, TABLE_NAME)
    }

    #[test]
    fn select_readcommited_with_record_version() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS8";

        let mut conn = cbuilder().connect()?;
        setup(&mut conn, TABLE_NAME)?;

        let mut transaction1 = Transaction::new(&mut conn, TransactionConfiguration::default())?;
        let _ = transaction1.execute_immediate(format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str())?;

        let mut conn2 = cbuilder().connect()?;
        let mut transaction2 = Transaction::new(&mut conn2, TransactionConfiguration {
            isolation: TrIsolationLevel::ReadCommited(TrRecordVersion::RecordVersion),
            lock_resolution: TrLockResolution::NoWait,
            ..Default::default()
        })?;
        let rows: Vec<(i32,)> = transaction2.query(format!(select_stmt_fmtstring!(), TABLE_NAME).as_str(), ())?;
        assert_eq!(0, rows.len());

        drop(transaction2);
        drop(transaction1);
        conn2.close()?;
        teardown(conn, TABLE_NAME)
    }
}
