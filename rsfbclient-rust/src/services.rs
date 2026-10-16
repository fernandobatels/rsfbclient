//! Service manager attachment over the wire protocol.
//!
//! The Services API rides on four opcodes the SQL path never uses:
//! `op_service_attach` (after the usual connect + auth handshake, against
//! `service_mgr` instead of a database), `op_service_start` to launch an
//! action such as a backup, `op_service_info` to read the action's output
//! and the server's properties, and `op_service_detach`. The actions run on
//! the server; every path they take is a server path.
//!
//! The byte formats of the requests and replies are shared with the native
//! client and live in `rsfbclient_core::services`; this module only moves
//! them over the socket.

use rsfbclient_core::{charset::UTF_8, ibase, services::parse_svc_string, FbError};

use crate::{
    client::{FirebirdWireConnection, SvcHandle},
    consts::WireOp,
    wire::SERVICE_MGR,
};

/// Bytes offered for one information reply. Lines of a verbose backup are
/// short; the server version is shorter still.
const INFO_BUFFER: u32 = 8192;

/// A pure-rust attachment to a Firebird server's service manager.
///
/// Mirrors the native `NativeServiceManager`: [`Self::start`] launches an
/// action, [`Self::query`] reads information back into a caller buffer.
/// Detaches on drop.
pub struct RustServiceManager {
    conn: FirebirdWireConnection,
    handle: Option<SvcHandle>,
}

impl RustServiceManager {
    /// Connect to `host:port`, authenticate as `user` and attach to the
    /// service manager. `expected_db` names the database the coming actions
    /// concern, for servers whose security database depends on it.
    pub fn attach(
        host: &str,
        port: u16,
        user: &str,
        pass: &str,
        expected_db: Option<&str>,
    ) -> Result<Self, FbError> {
        let mut conn = FirebirdWireConnection::connect_for(
            host,
            port,
            WireOp::ServiceAttach,
            SERVICE_MGR,
            user,
            pass,
            UTF_8,
        )?;
        let handle = conn.service_attach(user, pass, expected_db)?;
        Ok(Self {
            conn,
            handle: Some(handle),
        })
    }

    fn handle(&self) -> Result<SvcHandle, FbError> {
        self.handle
            .ok_or_else(|| FbError::from("Service manager already detached"))
    }

    /// Start a service action (`request`: the action byte and its
    /// arguments, see `rsfbclient_core::services`).
    pub fn start(&mut self, request: &[u8]) -> Result<(), FbError> {
        let handle = self.handle()?;
        self.conn.service_start(handle, request)
    }

    /// Query the service manager, writing the reply into `buffer` (the
    /// native `isc_service_query` contract). A reply longer than the buffer
    /// is cut; the buffer is zero-filled past the reply.
    pub fn query(&mut self, send: &[u8], receive: &[u8], buffer: &mut [u8]) -> Result<(), FbError> {
        let handle = self.handle()?;
        let data = self
            .conn
            .service_query(handle, send, receive, (buffer.len() as u32).min(INFO_BUFFER))?;
        let n = data.len().min(buffer.len());
        buffer[..n].copy_from_slice(&data[..n]);
        for b in &mut buffer[n..] {
            *b = 0;
        }
        Ok(())
    }

    /// The server version string, e.g. `LI-V4.0.2.2816 Firebird 4.0`.
    pub fn server_version(&mut self) -> Result<String, FbError> {
        let receive = [ibase::isc_info_svc_server_version as u8];
        let mut buffer = [0u8; 1024];
        self.query(&[], &receive, &mut buffer)?;
        parse_svc_string(&buffer, ibase::isc_info_svc_server_version as u8)
            .ok_or_else(|| FbError::from("Unexpected service version reply"))
    }

    /// Detach from the service manager. Also called on drop.
    pub fn detach(&mut self) -> Result<(), FbError> {
        if let Some(handle) = self.handle.take() {
            self.conn.service_detach(handle)?;
        }
        Ok(())
    }
}

impl Drop for RustServiceManager {
    fn drop(&mut self) {
        let _ = self.detach();
    }
}

#[cfg(test)]
mod tests {
    //! A scripted service manager on a loopback socket: checks the exact
    //! bytes the client sends for attach / start / info / detach, and that
    //! the info replies come back as the caller buffer.
    use super::*;
    use crate::{consts::ProtocolVersion, util::BufMutWireExt};
    use bytes::{BufMut, BytesMut};
    use rsfbclient_core::services::{backup_request, parse_svc_line, SvcBackupOptions};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    fn read_u32(r: &mut impl Read) -> u32 {
        let mut b = [0u8; 4];
        r.read_exact(&mut b).expect("peer closed");
        u32::from_be_bytes(b)
    }

    fn read_cstring(r: &mut impl Read) -> Vec<u8> {
        let len = read_u32(r) as usize;
        let padded = (len + 3) & !3;
        let mut b = vec![0u8; padded];
        r.read_exact(&mut b).unwrap();
        b.truncate(len);
        b
    }

    fn op_response(handle: u32, data: &[u8]) -> Vec<u8> {
        let mut b = BytesMut::new();
        b.put_u32(WireOp::Response as u32);
        b.put_u32(handle);
        b.put_u64(0);
        b.put_wire_bytes(data);
        b.put_u32(ibase::isc_arg_end);
        b.to_vec()
    }

    fn svc_line(text: &str) -> Vec<u8> {
        let mut b = vec![ibase::isc_info_svc_line as u8];
        b.extend_from_slice(&(text.len() as u16).to_le_bytes());
        b.extend_from_slice(text.as_bytes());
        b.push(ibase::isc_info_end as u8);
        b
    }

    #[derive(Default, Debug)]
    struct Seen {
        attach_name: Vec<u8>,
        spb: Vec<u8>,
        start: Vec<u8>,
        info_recv: Vec<Vec<u8>>,
        info_buffer_len: Vec<u32>,
        detached: bool,
    }

    /// Serves one client: attach → start → three line queries (two lines,
    /// then the empty one that ends the stream) → detach.
    fn serve(stream: TcpStream, seen: Arc<Mutex<Seen>>) {
        let mut rd = stream.try_clone().unwrap();
        let mut wr = stream;
        let lines = ["gbak:readied database for backup", "gbak:closing file, committing, and finishing", ""];
        let mut next_line = 0;
        loop {
            let mut op = [0u8; 4];
            if rd.read_exact(&mut op).is_err() {
                return;
            }
            let op = u32::from_be_bytes(op);
            if op == WireOp::ServiceAttach as u32 {
                let _object = read_u32(&mut rd);
                let name = read_cstring(&mut rd);
                let spb = read_cstring(&mut rd);
                let mut s = seen.lock().unwrap();
                s.attach_name = name;
                s.spb = spb;
                wr.write_all(&op_response(42, &[])).unwrap();
            } else if op == WireOp::ServiceStart as u32 {
                assert_eq!(read_u32(&mut rd), 42, "service handle");
                assert_eq!(read_u32(&mut rd), 0, "incarnation");
                seen.lock().unwrap().start = read_cstring(&mut rd);
                wr.write_all(&op_response(0, &[])).unwrap();
            } else if op == WireOp::ServiceInfo as u32 {
                assert_eq!(read_u32(&mut rd), 42, "service handle");
                assert_eq!(read_u32(&mut rd), 0, "incarnation");
                let send = read_cstring(&mut rd);
                let recv = read_cstring(&mut rd);
                let len = read_u32(&mut rd);
                assert!(send.is_empty());
                {
                    let mut s = seen.lock().unwrap();
                    s.info_recv.push(recv.clone());
                    s.info_buffer_len.push(len);
                }
                let reply = if recv == [ibase::isc_info_svc_server_version as u8] {
                    let mut b = vec![ibase::isc_info_svc_server_version as u8];
                    let v = b"LI-V5.0.1.1469 Firebird 5.0";
                    b.extend_from_slice(&(v.len() as u16).to_le_bytes());
                    b.extend_from_slice(v);
                    b.push(ibase::isc_info_end as u8);
                    b
                } else {
                    let l = svc_line(lines[next_line.min(lines.len() - 1)]);
                    next_line += 1;
                    l
                };
                wr.write_all(&op_response(0, &reply)).unwrap();
            } else if op == WireOp::ServiceDetach as u32 {
                assert_eq!(read_u32(&mut rd), 42);
                seen.lock().unwrap().detached = true;
                wr.write_all(&op_response(0, &[])).unwrap();
            } else {
                panic!("fake service manager: unexpected op {op}");
            }
        }
    }

    fn manager() -> (RustServiceManager, Arc<Mutex<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let server_seen = seen.clone();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, server_seen);
        });
        let socket = TcpStream::connect(addr).unwrap();
        // Protocol 10: the SPB carries the plaintext password, which the test
        // can then read back.
        let mut conn = FirebirdWireConnection::test_over_socket(socket, ProtocolVersion::V10);
        let handle = conn.service_attach("SYSDBA", "masterkey", Some("/d/a.fdb")).unwrap();
        (
            RustServiceManager {
                conn,
                handle: Some(handle),
            },
            seen,
        )
    }

    #[test]
    fn attach_sends_service_mgr_and_a_v2_spb() {
        let (mut svc, seen) = manager();
        svc.detach().unwrap();
        let s = seen.lock().unwrap();
        assert_eq!(s.attach_name, b"service_mgr");
        let mut spb = vec![2u8, 2, 28, 6];
        spb.extend_from_slice(b"SYSDBA");
        spb.extend_from_slice(&[124, 8]);
        spb.extend_from_slice(b"/d/a.fdb");
        spb.extend_from_slice(&[29, 9]);
        spb.extend_from_slice(b"masterkey");
        assert_eq!(s.spb, spb);
        assert!(s.detached);
    }

    #[test]
    fn start_then_drain_lines_until_the_empty_one() {
        let (mut svc, seen) = manager();
        let req = backup_request("/d/a.fdb", "/d/a.fbk", SvcBackupOptions::default());
        svc.start(&req).unwrap();

        let mut got = Vec::new();
        loop {
            let mut buffer = [0u8; 4096];
            svc.query(&[], &[ibase::isc_info_svc_line as u8], &mut buffer).unwrap();
            match parse_svc_line(&buffer) {
                Some(line) => got.push(line),
                None => break,
            }
        }
        assert_eq!(
            got,
            vec![
                "gbak:readied database for backup".to_string(),
                "gbak:closing file, committing, and finishing".to_string()
            ]
        );
        drop(svc); // detaches

        let s = seen.lock().unwrap();
        assert_eq!(s.start, req, "the start request goes out byte for byte");
        assert_eq!(s.info_recv.len(), 3);
        assert!(s.info_recv.iter().all(|r| r == &[ibase::isc_info_svc_line as u8]));
        assert!(s.info_buffer_len.iter().all(|l| *l == 4096));
        assert!(s.detached);
    }

    #[test]
    fn server_version_reply() {
        let (mut svc, _seen) = manager();
        assert_eq!(svc.server_version().unwrap(), "LI-V5.0.1.1469 Firebird 5.0");
    }

    #[test]
    fn detach_twice_is_harmless() {
        let (mut svc, _seen) = manager();
        svc.detach().unwrap();
        svc.detach().unwrap();
        assert!(svc.start(&[1]).is_err());
    }
}
