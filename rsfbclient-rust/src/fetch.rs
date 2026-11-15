use super::*;
use rsfbclient_core::charset::UTF_8;
use std::net::TcpListener;

fn response(handle: u32) -> Vec<u8> {
    let mut b = BytesMut::new();
    b.put_u32(WireOp::Response as u32);
    b.put_u32(handle);
    b.put_u64(0);
    b.put_wire_bytes(&[]);
    b.put_u32(ibase::isc_arg_end);
    b.to_vec()
}
fn row(value: i64) -> Vec<u8> {
    let mut b = BytesMut::new();
    b.put_u32(WireOp::FetchResponse as u32);
    b.put_u32(0);
    b.put_u32(1);
    b.put_u32(0);
    b.put_i64(value);
    b.to_vec()
}
#[test]
fn completed_rows_precede_the_error_and_preserve_the_following_packet() {
    check_completed_rows(false, false);
    check_completed_rows(true, false);
    check_completed_rows(false, true);
}

fn check_completed_rows(deferred: bool, raw: bool) {
    for buffer in [1, 5, 4096] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let server = std::thread::spawn(move || {
            let mut request = [0u8; 20];
            peer.read_exact(&mut request).unwrap();
            assert_eq!(&request[..4], &(WireOp::Fetch as u32).to_be_bytes());
            assert_eq!(
                &request[16..],
                &(if raw { fetch_batch_size() } else { 2 }).to_be_bytes(),
                "Batching must remain active"
            );
            let mut b = BytesMut::new();
            b.extend_from_slice(&row(7));
            b.extend_from_slice(&row(8));
            b.put_u32(WireOp::Response as u32);
            b.put_u32(0);
            b.put_u64(0);
            b.put_wire_bytes(&[]);
            b.put_u32(ibase::isc_arg_gds);
            b.put_u32(ibase::isc_except);
            b.put_u32(ibase::isc_arg_number);
            b.put_u32(1);
            b.put_u32(ibase::isc_arg_end);
            b.extend_from_slice(&response(99));
            peer.write_all(&b).unwrap();
        });
        let mut conn = FirebirdWireConnection {
            socket: FbStream::Plain(socket),
            version: ProtocolVersion::V13,
            buff: vec![0; buffer].into_boxed_slice(),
            pending: Bytes::new(),
            lazy_count: 0,
            charset: UTF_8,
            inline_blob_size: 65535,
            auth_plugin: None,
            srp_key: [0; 32],
            event_channel: None,
            next_event_id: 1,
        };
        let mut stmt = StmtHandleData {
            handle: StmtHandle(23),
            xsqlda: vec![XSqlVar {
                sqltype: ibase::SQL_INT64 as i16,
                data_length: 8,
                alias_name: "ID".into(),
                ..Default::default()
            }],
            blr: Bytes::new(),
            param_count: 0,
            prefetched: VecDeque::new(),
            fetch_error: None,
            cursor_eof: false,
            has_blob: deferred,
        };
        let mut tr = TrHandle(7);
        if raw {
            let mut values = Vec::new();
            let error = conn
                .stream_raw(&mut stmt, |row| {
                    if let RawValue::Integer(value) = row[0] {
                        values.push(value);
                    }
                    Ok(())
                })
                .unwrap_err();
            server.join().unwrap();
            assert_eq!(values, vec![7, 8]);
            assert!(matches!(error, FbError::Sql { .. }));
            assert!(stmt.cursor_eof);
        } else {
            let batch = conn.fetch_batch(&mut tr, &mut stmt, 2);
            server.join().unwrap();
            assert!(
                batch.is_ok(),
                "A terminal SQL error discarded its completed rows: {batch:?}"
            );
            for n in [7, 8] {
                let columns = conn.fetch(&mut tr, &mut stmt).unwrap().unwrap();
                assert!(matches!(columns[0].value,SqlType::Integer(value) if value==n));
            }
            let error = conn.fetch(&mut tr, &mut stmt).unwrap_err().to_string();
            assert!(error.contains("exception 1"), "{error}");
            assert!(conn.fetch(&mut tr, &mut stmt).unwrap().is_none());
        }
        assert_eq!(
            conn.read_response().unwrap().handle,
            99,
            "The completed error must not poison the next response"
        );
    }
}
