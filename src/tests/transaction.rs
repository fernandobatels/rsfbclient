//!
//! Rust Firebird Client
//!
//! Transaction struct tests
//!

mk_tests_default! {
  use crate::{FbError, Connection, Transaction};
  use rsfbclient_core::FirebirdClient;


  macro_rules! recreate_tbl_fmtstring{
    () => {"recreate table {} ( id INT NOT NULL PRIMARY KEY, description VARCHAR(20) );"};
  }
  macro_rules! drop_tbl_fmtstring{
    () => {"drop table {};"};
  }
  macro_rules! insert_stmt_fmtstring{
    () => {"insert into {} (id, description) values (543210, 'testing');"};
  }

  fn setup<C: FirebirdClient>( conn: &mut Connection<C>, table_name: &str ) ->  Result<(), FbError>{
      let mut setup_transaction = Transaction::new(conn)?;
      setup_transaction.execute_immediate( format!(recreate_tbl_fmtstring!(), table_name).as_str() )?;
      setup_transaction.commit()
  }

  fn teardown<C: FirebirdClient>( conn: Connection<C>, table_name: &str ) -> Result<(), FbError> {
      let mut conn = conn;
      let mut setup_transaction = Transaction::new(&mut conn)?;
      setup_transaction.execute_immediate( format!(drop_tbl_fmtstring!(), table_name ).as_str() )?;
      setup_transaction.commit()?;

      conn.close()
  }

  #[test]
  fn recreate_insert_drop_with_commit() -> Result<(), FbError> {
      const TABLE_NAME: &str = "RSFBCLIENT_TEST_TRANS0";

      let mut conn = cbuilder().connect()?;
      setup(&mut conn, TABLE_NAME)?;

      let mut transaction = Transaction::new(&mut conn)?;
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

      let mut transaction = Transaction::new(&mut conn)?;
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

      let mut transaction = Transaction::new(&mut conn)?;
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

      let mut transaction = Transaction::new(&mut conn)?;
      let _insert_result  = transaction.execute_immediate( format!(insert_stmt_fmtstring!(), TABLE_NAME).as_str() );
      let rollback_result = transaction.rollback_retaining();
      drop(transaction);

      teardown(conn, TABLE_NAME)?;

      rollback_result
  }

}
