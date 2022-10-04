//!
//! Rust Firebird Client
//!
//! Transaction struct tests
//!

mk_tests_default! {
    use crate::{FbError, Connection, Transaction, query::Queryable};
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
        let mut e = qr.err().unwrap().to_string();
        e.truncate(95);
        assert!(e.contains("deadlock\nread conflicts with concurrent update\nconcurrent transaction number is"));

        drop(transaction2);
        drop(transaction1);
        conn2.close()?;
        teardown(conn, TABLE_NAME)
    }

    #[test]
    fn select_readcommited_with_wait_for_some_seconds() -> Result<(), FbError> {
        const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS5";

        let mut conn = cbuilder().connect()?;
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
        let mut e = qr.err().unwrap().to_string();
        e.truncate(95);
        assert!(e.contains("deadlock\nread conflicts with concurrent update\nconcurrent transaction number is"));

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
