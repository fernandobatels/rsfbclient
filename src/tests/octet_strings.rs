//! OCTETS parameters and results retain bytes independently of attachment encoding.
mk_tests_default! {
    use crate::{FbError, Row, SimpleConnection, SqlType, prelude::*};

    #[test]
    fn binary_strings_round_trip_without_turning_into_blobs() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().charset(crate::charset::WIN_1251).connect()?.into();
        conn.execute(
            "CREATE TABLE RSFB_OCTET_TEST (ID INT PRIMARY KEY, K CHAR(5) CHARACTER SET OCTETS, V VARCHAR(12000) CHARACTER SET OCTETS, B BLOB SUB_TYPE BINARY)",
            (),
        )?;
        let key = vec![0, 255, 128, 65, 0];
        let bytes: Vec<u8> = (0..12_000).map(|n| (n % 256) as u8).collect();
        let blob = vec![255, 0, 128];
        conn.execute("INSERT INTO RSFB_OCTET_TEST VALUES (?, ?, ?, ?)",
            vec![SqlType::Integer(1), SqlType::Octets(key.clone()), SqlType::Octets(bytes.clone()), SqlType::Binary(blob.clone())],
        )?;
        conn.execute("INSERT INTO RSFB_OCTET_TEST (ID,V) VALUES (?,?)",
            vec![SqlType::Integer(2), SqlType::Octets(Vec::new())],
        )?;
        conn.execute("INSERT INTO RSFB_OCTET_TEST (ID) VALUES (3)", ())?;
        let row: Row = conn.query_first("SELECT K,V,B FROM RSFB_OCTET_TEST WHERE K=? AND ID=1",
            vec![SqlType::Octets(key.clone())],
        )?.unwrap();
        assert!(matches!(&row.cols[0].value, SqlType::Octets(value) if value == &key));
        assert!(matches!(&row.cols[1].value, SqlType::Octets(value) if value == &bytes));
        assert!(matches!(&row.cols[2].value, SqlType::Binary(value) if value == &blob));
        assert_eq!(row.get::<Vec<u8>>(1)?, bytes);
        assert!(row.get::<String>(1).is_err());
        assert_eq!(conn.query_first::<_, (Option<Vec<u8>>,)>("SELECT V FROM RSFB_OCTET_TEST WHERE ID=2", ())?, Some((Some(Vec::new()),)));
        assert_eq!(conn.query_first::<_, (Option<Vec<u8>>,)>("SELECT V FROM RSFB_OCTET_TEST WHERE ID=3", ())?, Some((None,)));
        assert!(conn.execute("UPDATE RSFB_OCTET_TEST SET V=? WHERE ID=1",
            vec![SqlType::Octets(vec![0; 32768])],
        ).is_err());
        assert_eq!(conn.query_first::<_, (i32,)>("SELECT OCTET_LENGTH(V) FROM RSFB_OCTET_TEST WHERE ID=1", ())?, Some((12000,)));
        #[cfg(feature = "pure_rust")]
        if let Some(wire) = conn.as_pure_rust_mut() {
            let mut visited = 0;
            wire.stream_raw("SELECT K,V FROM RSFB_OCTET_TEST WHERE ID=1", |values| {
                assert!(matches!(&values[0], crate::RawValue::Octets(value) if value.as_ref() == key));
                assert!(matches!(&values[1], crate::RawValue::Octets(value) if value.as_ref() == bytes));
                visited += 1;
                Ok(())
            })?;
            assert_eq!(visited, 1);
        }
        conn.close()?;
        cbuilder().connect()?.execute("DROP TABLE RSFB_OCTET_TEST", ())?;
        Ok(())
    }
}
