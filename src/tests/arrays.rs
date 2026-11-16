//! ARRAY values must be used in the attachment and transaction that own them.
mk_tests_default! {
    use crate::{EngineVersion, FbError, Row, SimpleConnection, SystemInfos, prelude::*};
    use rsfbclient_core::{ArrayElement, ArrayId};

    #[test]
    fn array_slices_round_trip_through_sql_and_the_owning_cursor() -> Result<(), FbError> {
        let mut conn: SimpleConnection = cbuilder().connect()?.into();
        // Check that native SQL-info preserves UTF-8 origins beyond XSQLVAR's limit.
        let relation = if conn.server_engine()? >= EngineVersion::V4 {
            " Array Ж data beyond the old metadata buffer"
        } else {
            "Array round trip"
        };
        let table = format!("\"{relation}\"");
        let bounds = [(-2, 0), (4, 6)];
        let integers = [i32::MIN, -72, 0, 10, 25, 31, 37, -86, i32::MAX];
        let shorts = [-10i16, 123, i16::MAX];
        let slots: Vec<_> = shorts.into_iter().flat_map(i16::to_le_bytes).collect();
        let element = ArrayElement { blr: 7, scale: 0, length: 2, charset: 0 };

        conn.execute(&format!("CREATE TABLE {table}(ID INTEGER PRIMARY KEY, A INTEGER[-2:0,4:6], B SMALLINT[1:3])"), ())?;
        conn.execute(&format!("INSERT INTO {table}(ID) VALUES (1)"), ())?;
        assert!(conn.arrays().create_integer_array(relation, "A", &bounds, &integers).is_err());

        conn.begin_transaction()?;
        let null_row: Row = conn.query_first(&format!("SELECT A FROM {table}"), ())?.unwrap();
        assert!(null_row.cols[0].value.is_null());
        assert_eq!(null_row.cols[0].array_origin.as_ref().unwrap().relation, relation);
        let id = conn.arrays().create_integer_array(relation, "A", &bounds, &integers)?;
        let small = conn.arrays().put_array(ArrayId::default(), relation, "B", &[(1, 3)], element, &slots)?;
        assert_eq!(conn.execute(&format!("UPDATE {table} SET A=?, B=? WHERE ID=1"), (id, small))?, 1);
        conn.commit()?;

        let got = conn.with_query_cursor(
            &format!("SELECT A AS WIDE_ARRAY,B AS TYPED_ARRAY FROM {table}"),
            |names, cursor| {
                assert_eq!(names, ["WIDE_ARRAY", "TYPED_ARRAY"]);
                let row = cursor.fetch()?.unwrap();
                let origin = row.cols[0].array_origin.as_ref().unwrap();
                assert_eq!((&*origin.relation, &*origin.field), (relation, "A"));
                let id: ArrayId = row.get(0)?;
                let small: ArrayId = row.get(1)?;
                let reader = cursor.array_reader();
                assert_eq!(reader.read_integer_array(id, relation, "A", &bounds)?, integers);
                assert_eq!(reader.read_integer_array(id, relation, "A", &[(-1, 0), (5, 6)])?, [25,31,-86,i32::MAX]);
                let slice = reader.read_array(small, relation, "B", &[(1, 3)], element)?;
                assert!(cursor.fetch()?.is_none());
                Ok(slice)
            },
        )?;
        assert_eq!(got, slots);
        assert_eq!(
            conn.query_first::<_, (i32,i16)>(&format!("SELECT A[-1,5],B[3] FROM {table}"), ())?,
            Some((25,i16::MAX)),
        );
        conn.with_query_cursor(&format!("SELECT A AS EMPTY_ARRAY FROM {table} WHERE 1=0"), |columns,cursor| {
            assert_eq!(columns, ["EMPTY_ARRAY"]);
            assert!(cursor.fetch()?.is_none());
            Ok(())
        })?;
        conn.close()?;
        cbuilder().connect()?.execute(&format!("DROP TABLE {table}"), ())?;
        Ok(())
    }
}
