//! Exercise cancellation while Firebird is waiting for a conflicting update.
#![cfg(not(feature = "embedded_tests"))]

mk_tests_default! {
    use crate::{EngineVersion, FbError, SimpleConnection, SystemInfos, prelude::*};
    use std::{sync::mpsc, time::{Duration, Instant}};

    #[test]
    fn cancelling_a_busy_statement_preserves_the_attachment_and_revokes_old_handles() -> Result<(), FbError> {
        let mut blocker: SimpleConnection = cbuilder().connect()?.into();
        if blocker.server_engine()? < EngineVersion::V3 { return Ok(()); }
        let mut target: SimpleConnection = cbuilder().connect()?.into();
        let mut observer = cbuilder().connect()?;
        blocker.execute("CREATE TABLE RSFB_CANCEL_TEST(ID INTEGER PRIMARY KEY, N INTEGER)", ())?;
        blocker.execute("INSERT INTO RSFB_CANCEL_TEST VALUES(1,0)", ())?;
        blocker.begin_transaction()?;
        blocker.execute("UPDATE RSFB_CANCEL_TEST SET N=1 WHERE ID=1", ())?;
        let (original,): (i64,) = target.query_first("SELECT CURRENT_CONNECTION FROM RDB$DATABASE", ())?.unwrap();
        let stale = target.statement_cancel_handle()?;
        let cancel = target.statement_cancel_handle()?;
        assert!(!stale.request()?);
        assert!(!stale.abort()?);

        let mut result = None;
        let mut requested = false;
        std::thread::scope(|scope| -> Result<(), FbError> {
            let (send, received) = mpsc::channel();
            let target = &mut target;
            scope.spawn(move || {
                let _ = send.send(target.execute("UPDATE RSFB_CANCEL_TEST SET N=99 WHERE ID=1", ()));
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            let action = (|| -> Result<(), FbError> {
                while Instant::now() < deadline {
                    let (active,): (i32,) = observer.query_first(
                        "SELECT COUNT(*) FROM MON$STATEMENTS WHERE MON$ATTACHMENT_ID=? AND MON$STATE=1",
                        (original,),
                    )?.unwrap();
                    if active > 0 {
                        requested = cancel.request()?;
                        result = received.recv_timeout(Duration::from_secs(5)).ok();
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(())
            })();
            // Release the lock even if monitoring or cancellation failed.
            blocker.rollback()?;
            action
        })?;
        assert!(requested, "the test must cancel a running statement");
        let error = result.expect("cancel must complete before the conflicting lock is released").unwrap_err();
        assert!(error.to_string().to_ascii_lowercase().contains("cancel"), "{error}");
        assert_eq!(target.query_first::<_,(i64,)>("SELECT CURRENT_CONNECTION FROM RDB$DATABASE", ())?, Some((original,)));
        assert_eq!(target.query_first::<_,(i32,)>("SELECT N FROM RSFB_CANCEL_TEST WHERE ID=1", ())?, Some((0,)));

        target.close()?;
        assert!(!cancel.request()?);
        assert!(!cancel.abort()?);
        blocker.close()?;
        observer.close()?;
        cbuilder().connect()?.execute("DROP TABLE RSFB_CANCEL_TEST", ())?;
        Ok(())
    }

    #[test]
    fn aborting_an_attachment_rolls_back_its_uncommitted_changes() -> Result<(), FbError> {
        let mut observer: SimpleConnection = cbuilder().connect()?.into();
        if observer.server_engine()? < EngineVersion::V3 { return Ok(()); }
        let mut target: SimpleConnection = cbuilder().connect()?.into();
        observer.execute("CREATE TABLE RSFB_ABORT_TEST(N INTEGER)", ())?;
        observer.execute("INSERT INTO RSFB_ABORT_TEST VALUES (0)", ())?;
        target.begin_transaction()?;
        target.execute("UPDATE RSFB_ABORT_TEST SET N=99", ())?;
        let cancel = target.statement_cancel_handle()?;
        let aborter = cancel.clone();
        let aborted = std::thread::spawn(move || aborter.abort()).join().unwrap()?;
        assert!(aborted);
        assert!(cancel.was_aborted());
        assert!(!cancel.request()?);
        assert!(!cancel.abort()?);
        drop(target);

        // An update from a different attachment must be able to take the lock.
        assert_eq!(observer.execute("UPDATE RSFB_ABORT_TEST SET N=N+1", ())?, 1);
        assert_eq!(observer.query_first::<_, (i32,)>("SELECT N FROM RSFB_ABORT_TEST", ())?, Some((1,)));
        observer.close()?;
        cbuilder().connect()?.execute("DROP TABLE RSFB_ABORT_TEST", ())?;
        Ok(())
    }

}
