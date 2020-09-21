//!
//! Rust Firebird Client
//!
//! Transaction struct tests
//!

mk_tests_default! {
  use crate::{prelude::*, FbError, Connection, Transaction};
  use rsfbclient_core::FirebirdClient;


  const RECREATE_TBL_STMT : &str = "recreate table RSFBCLIENT_TEST_TRANSACTION ( id INT NOT NULL PRIMARY KEY, description VARCHAR(20) );";
  const DROP_TBL_STMT     : &str = "drop table RSFBCLIENT_TEST_TRANSACTION;";
  const INSERT_STMT       : &str = "insert into RSFBCLIENT_TEST_TRANSACTION (id, description) values (543210, 'testing');";


  fn setup<C: FirebirdClient>( conn: &mut Connection<C> ) ->  Result<(), FbError>{
     conn.execute( RECREATE_TBL_STMT, ())
  }

  fn teardown<C: FirebirdClient>( conn: &mut Connection<C> ) -> Result<(), FbError> {
     conn.execute( DROP_TBL_STMT, ())
  }

  #[test]
  fn recreate_insert_drop_with_commit() -> Result<(), FbError> {
      let mut conn = cbuilder().connect()?;

      setup(&mut conn)?;

      let mut transaction = Transaction::new(&conn)?;
      let _insert_result = transaction.execute( INSERT_STMT, () );
      let commit_result = transaction.commit();

      teardown(&mut conn)?;

      commit_result
  }

  #[test]
  fn recreate_insert_drop_with_commit_retaining() -> Result<(), FbError> {
      let mut conn = cbuilder().connect()?;

      setup(&mut conn)?;

      let mut transaction = Transaction::new(&conn)?;
      let _insert_result = transaction.execute( INSERT_STMT, () );
      let commit_result = transaction.commit_retaining();
      drop(transaction);

      teardown(&mut conn)?;

      commit_result
  }

}
