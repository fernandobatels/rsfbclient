# Native managed API acceptance tests

These opt-in tests create two databases with unique names in an existing,
server-writable directory. They never attach to a supplied existing database.
Each fixture drops only databases it created. The event disconnect test terminates
attachments only inside its own newly created database.

Set these environment variables without putting credentials into shell history:

- `RSFB_TEST_DIR`: existing directory on the Firebird server (not a database file).
- `RSFB_TEST_CLIENT`: native client library matching the test executable's architecture.
- `RSFB_TEST_HOST` and `RSFB_TEST_PORT`: test server.
- `ISC_USER` and `ISC_PASSWORD`: account allowed to create/drop the test databases
  and terminate its test event attachment (SYSDBA or equivalent privileges).

Run, adding `--target i686-pc-windows-msvc` for a Win32 client:

```text
cargo test -p rsfbclient-native --features dynamic_loading --test managed -- --ignored --test-threads=1
cargo test -p rsfbclient-native --features dynamic_loading --lib
```

Coverage:

- Existing typed conversion, lossless large/negative NUMERIC values, NULL versus
  empty strings, Turkish SQL text and database filenames, timestamp/date/time,
  multi-segment binary/text BLOBs, INSERT RETURNING.
- Read-only writes, snapshot isolation, NO WAIT conflicts, implicit Drop rollback,
  invalid participant/count/state, and cleanup after failed second attachment.
- Repeated prepare/bind/execute/fetch errors, with MON$STATEMENTS checks proving
  no accumulated statement handles on the attachment.
- Common two-database rollback, prepare/commit, prepare/rollback and detached
  limbo recovery for commit and rollback, including duplicate recovery rejection.
- Initial event baseline, idle timeout, rollback suppression, committed bursts,
  re-arming, bounded normal cancellation, forced attachment loss and resubscription.

The APIs deliberately leave an ambiguous prepare/commit for explicit recovery.
The application must persist participant IDs, transaction identity, and the commit
decision durably before commit; the library does not replace that journal.
The recovery methods currently accept Firebird's 32-bit transaction IDs and reject
truncated/longer database-info responses rather than silently truncating them.
If cleanup fails, the remaining generated database name is included in the native
error and can be inspected on the test server.
