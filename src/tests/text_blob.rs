//! UTF-8 text BLOB parameters are independent of the attachment encoding.
mk_tests_default! {
    use crate::{prelude::*, FbError, SqlType};

    #[test]
    fn text_blob_parameters_keep_text_and_binary_distinct() -> Result<(), FbError> {
        let mut conn = cbuilder().charset(rsfbclient_core::charset::WIN_1251).connect()?;
        conn.execute(
            "RECREATE TABLE RSFB_TEXT_BLOB (
                ID INTEGER PRIMARY KEY,
                CONTENT BLOB SUB_TYPE TEXT CHARACTER SET UTF8,
                NOTE VARCHAR(12000) CHARACTER SET ASCII,
                BIN BLOB SUB_TYPE BINARY
            )",
            (),
        )?;
        let text = "Жёлтый текст ".repeat(1000);
        let note = "abc".repeat(3000);
        let binary = vec![0, 255, 128, 0, 64];
        let sql = "INSERT INTO RSFB_TEXT_BLOB VALUES
            (?, ?, CAST(? AS BLOB SUB_TYPE TEXT CHARACTER SET UTF8), ?)";
        conn.execute(sql, vec![
            SqlType::Integer(1),
            SqlType::TextBlob(text.clone()),
            SqlType::TextBlob(note.clone()),
            SqlType::Binary(binary.clone()),
        ])?;
        let first: (String, String, Vec<u8>) = conn.query_first(
            "SELECT CONTENT, NOTE, BIN FROM RSFB_TEXT_BLOB WHERE ID=1", (),
        )?.expect("inserted row");
        assert_eq!(first, (text, note, binary));

        // The existing automatic fallback for oversized strings uses the same
        // UTF-8 text descriptor in both clients.
        let long = "Длинный текст ".repeat(2000);
        conn.execute("UPDATE RSFB_TEXT_BLOB SET CONTENT=?", (long.clone(),))?;
        assert_eq!(
            conn.query_first::<_, (String,)>("SELECT CONTENT FROM RSFB_TEXT_BLOB", ())?,
            Some((long,)),
        );
        conn.close()?;
        cbuilder().connect()?.execute("DROP TABLE RSFB_TEXT_BLOB", ())?;
        Ok(())
    }
}
