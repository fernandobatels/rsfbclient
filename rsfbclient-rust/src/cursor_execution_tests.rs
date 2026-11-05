use super::*;
use rsfbclient_core::charset::UTF_8;
use std::net::TcpListener;

#[test]
fn cursor_execute_consumes_its_status_without_requesting_record_counts() {
    for (version, buffer_size, fail) in [
        (ProtocolVersion::V13, 1, false),
        (ProtocolVersion::V13, 1, true),
        (ProtocolVersion::V19, 4096, false),
        (ProtocolVersion::V19, 4096, true),
    ] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let server = std::thread::spawn(move || {
            let mut header = [0u8; 16];
            peer.read_exact(&mut header).unwrap();
            assert_eq!(&header[..4], &(WireOp::Execute as u32).to_be_bytes());
            let blr_length = u32::from_be_bytes(header[12..16].try_into().unwrap()) as usize;
            let tail = if version >= ProtocolVersion::V19 {
                12
            } else {
                0
            };
            let mut rest = vec![0; (blr_length + 3) / 4 * 4 + 8 + tail];
            peer.read_exact(&mut rest).unwrap();
            let mut status = BytesMut::new();
            status.put_u32(WireOp::Response as u32);
            status.put_u32(0);
            status.put_u64(0);
            status.put_wire_bytes(&[]);
            if fail {
                status.put_u32(ibase::isc_arg_gds);
                status.put_u32(335544321); // arithmetic error
            }
            status.put_u32(ibase::isc_arg_end);
            peer.write_all(&status).unwrap();
            let mut next = [0u8; 12];
            peer.read_exact(&mut next).unwrap();
            assert_eq!(
                &next[..4],
                &(WireOp::FreeStatement as u32).to_be_bytes(),
                "Opening a cursor must not send an intervening info_sql request"
            );
        });
        let mut conn = FirebirdWireConnection {
            socket: FbStream::Plain(socket),
            version,
            buff: vec![0; buffer_size].into_boxed_slice(),
            pending: Bytes::new(),
            lazy_count: 0,
            inline_blob_size: 0,
            charset: UTF_8,
            auth_plugin: None,
            srp_key: [0; 32],
            event_channel: None,
            next_event_id: 1,
        };
        let mut stmt = StmtHandleData {
            handle: StmtHandle(23),
            xsqlda: vec![],
            blr: Bytes::new(),
            param_count: 0,
            prefetched: VecDeque::from([vec![]]),
            cursor_eof: true,
            has_blob: false,
        };
        let result = conn.execute_cursor(&mut TrHandle(7), &mut stmt, &[]);
        assert_eq!(result.is_err(), fail);
        if fail {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("arithmetic exception"));
        }
        assert!(stmt.prefetched.is_empty());
        assert!(!stmt.cursor_eof);
        conn.free_statement(&mut stmt, FreeStmtOp::Close).unwrap();
        server.join().unwrap();
    }
}
