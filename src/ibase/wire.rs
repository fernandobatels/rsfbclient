//!
//! Rust Firebird Client
//!
//! Wire protocol implementation
//!

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::{
    convert::TryFrom,
    io::{Read, Write},
    net::TcpStream,
};

use super::*;
use crate::{status::SqlError, FbError};

/// Buffer length to use in the connection
const BUFFER_LENGTH: u32 = 1024;

pub struct WireConnection {
    /// Connection socket
    socket: TcpStream,

    /// Wire protocol version
    version: ProtocolVersion,

    /// Buffer to read the network data
    buff: Vec<u8>,
}

impl WireConnection {
    /// Start a connection to the firebird server
    fn connect(host: &str, port: u16, db_name: &str) -> Result<Self, FbError> {
        let mut socket = TcpStream::connect((host, port))?;

        socket.write_all(&connect(db_name))?;

        // May be a bit too much
        let mut buff = vec![0; BUFFER_LENGTH as usize * 2];

        let len = socket.read(&mut buff)?;
        let mut resp = Bytes::copy_from_slice(&buff[..len]);

        let op_code = resp.get_u32();
        if op_code != WireOp::Accept as u32 {
            return Err(FbError::Other(format!(
                "Connection rejected with code {}",
                op_code
            )));
        }

        let version =
            ProtocolVersion::try_from(resp.get_u32()).map_err(|e| FbError::Other(e.to_string()))?;
        // println!("Arch: {:X}", resp.get_u32());
        // println!("Type: {:X}", resp.get_u32());

        Ok(Self {
            socket,
            version,
            buff,
        })
    }

    /// Connect to a database, returning a database handle
    fn attach_database(
        &mut self,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<DbHandle, FbError> {
        self.socket
            .write_all(&attach(db_name, user, pass, self.version as u32))?;

        let resp = self.read_response()?;

        Ok(DbHandle(resp.handle))
    }

    /// Start a new transaction, with the specified transaction parameter buffer
    fn begin_transaction(&mut self, db_handle: DbHandle, tpb: &[u8]) -> Result<TrHandle, FbError> {
        self.socket
            .write_all(&transaction(db_handle.0, tpb))
            .unwrap();

        let resp = self.read_response()?;

        Ok(TrHandle(resp.handle))
    }

    fn prepare_statement(
        &mut self,
        db_handle: DbHandle,
        tr_handle: TrHandle,
        sql: &str,
    ) -> Result<(StmtType, StmtHandle, XSqlDa), FbError> {
        // Alloc statement
        self.socket.write_all(&allocate_statement(db_handle.0))?;

        // Prepare statement
        self.socket
            .write_all(&prepare_statement(tr_handle.0, u32::MAX, 3, sql))?;

        let len = self.socket.read(&mut self.buff)?;
        let mut resp = Bytes::copy_from_slice(&self.buff[..len]);

        // Alloc resp
        let op_code = resp.get_u32();
        if op_code != WireOp::Response as u32 {
            return Err(FbError::Other(format!(
                "Connection rejected with code {}",
                op_code
            )));
        }

        let stmt_handle = StmtHandle(parse_response(&mut resp)?.handle);

        // Prepare resp
        let op_code = resp.get_u32();
        if op_code != WireOp::Response as u32 {
            return Err(FbError::Other(format!(
                "Connection rejected with code {}",
                op_code
            )));
        }

        let resp = parse_response(&mut resp)?;
        let (stmt_type, xsqlda, truncated) = parse_xsqlda(&resp.data)?;
        // TODO: handle truncated

        Ok((stmt_type, stmt_handle, xsqlda))
    }

    /// Read a server response
    fn read_response(&mut self) -> Result<Response, FbError> {
        let len = self.socket.read(&mut self.buff).unwrap();
        let mut resp = Bytes::copy_from_slice(&self.buff[..len]);

        let op_code = resp.get_u32();
        if op_code != WireOp::Response as u32 {
            return Err(FbError::Other(format!(
                "Connection rejected with code {}",
                op_code
            )));
        }

        parse_response(&mut resp)
    }
}

#[derive(Debug, Clone, Copy)]
/// A database handle
struct DbHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A transaction handle
struct TrHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A statement handle
struct StmtHandle(u32);

#[test]
fn connection_test() {
    let db_name = "test.fdb";
    let user = "SYSDBA";
    let pass = "masterkey";

    let mut conn = WireConnection::connect("127.0.0.1", 3050, db_name).unwrap();

    let db_handle = conn.attach_database(db_name, user, pass).unwrap();

    let tr_handle = conn
        .begin_transaction(
            db_handle,
            &[isc_tpb_version3 as u8, isc_tpb_read_committed as u8],
        )
        .unwrap();

    let (stmt_type, stmt_handle, xsqlda) = conn
        .prepare_statement(
            db_handle,
            tr_handle,
            "SELECT 1, cast('abcdefghijk' as varchar(10)) as tst FROM RDB$DATABASE",
        )
        .unwrap();

    println!("{:#?}", xsqlda);
    println!("StmtType: {:?}", stmt_type);

    std::thread::sleep(std::time::Duration::from_millis(100));
}

/// Prepare statement request. Use u32::MAX as `stmt_handle` if the statement was allocated
/// in the previous request
fn prepare_statement(tr_handle: u32, stmt_handle: u32, dialect: u32, query: &str) -> Bytes {
    let mut req = BytesMut::with_capacity(256);

    req.put_u32(WireOp::PrepareStatement as u32);
    req.put_u32(tr_handle);
    req.put_u32(stmt_handle);
    req.put_u32(dialect);
    put_wire_bytes(&mut req, query.as_bytes());
    put_wire_bytes(&mut req, &XSQLDA_DESCRIBE_VARS);

    req.put_u32(BUFFER_LENGTH);

    req.freeze()
}

/// Statement allocation request (lazy response)
fn allocate_statement(db_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(8);

    req.put_u32(WireOp::AllocateStatement as u32);
    req.put_u32(db_handle);

    req.freeze()
}

/// Begin transaction request
fn transaction(db_handle: u32, tpb: &[u8]) -> Bytes {
    let mut tr = BytesMut::with_capacity(tpb.len() + 8);

    tr.put_u32(WireOp::Transaction as u32);
    tr.put_u32(db_handle);
    put_wire_bytes(&mut tr, tpb);

    tr.freeze()
}

#[derive(Debug)]
/// `WireOp::Response` response
struct Response {
    handle: u32,
    object_id: u64,
    data: Bytes,
}

/// Parse a server response (`WireOp::Response`)
fn parse_response(resp: &mut Bytes) -> Result<Response, FbError> {
    let handle = resp.get_u32();
    let object_id = resp.get_u64();

    let data = get_wire_bytes(resp);

    parse_status_vector(resp)?;

    Ok(Response {
        handle,
        object_id,
        data,
    })
}

/// Connection request
fn connect(db_name: &str) -> Bytes {
    let protocols = [
        // PROTOCOL_VERSION, Arch type (Generic=1), min, max, weight
        [ProtocolVersion::V10 as u32, 1, 0, 5, 2],
        [ProtocolVersion::V11 as u32, 1, 0, 5, 4],
        [ProtocolVersion::V12 as u32, 1, 0, 5, 6],
        // [ProtocolVersion::V13 as u32, 1, 0, 5, 8],
    ];

    let mut connect = BytesMut::new();
    connect.put_u32(WireOp::Connect as u32);
    connect.put_u32(WireOp::Attach as u32);
    connect.put_u32(3); // CONNECT_VERSION
    connect.put_u32(1); // arch_generic

    put_wire_bytes(&mut connect, db_name.as_bytes());

    connect.put_u32(protocols.len() as u32);

    // User identification, TODO: Wire protocol 13
    let uid = {
        let uid = BytesMut::new();

        // let key: [u8; 16] = rand::random();
        // let srp = SrpClient::<sha1::Sha1>::new(&key, &groups::G_1024);
        // let pubkey = srp
        //     .get_a_pub()
        //     .into_iter()
        //     .map(|b| format!("{:02X}", b))
        //     .fold(String::new(), |acc, b| acc + &b);

        // uid.put_u8(Cnct::Login as u8);
        // uid.put_u8(user.len() as u8);
        // uid.put(user.as_bytes());

        // let plugin = "Srp";

        // uid.put_u8(Cnct::PluginName as u8);
        // uid.put_u8(plugin.len() as u8);
        // uid.put(plugin.as_bytes());

        // let plugin_list = "Srp, Srp256, Legacy_Auth";

        // uid.put_u8(Cnct::PluginList as u8);
        // uid.put_u8(plugin_list.len() as u8);
        // uid.put(plugin_list.as_bytes());

        // for (i, pk_chunk) in pubkey.as_bytes().chunks(254).enumerate() {
        //     uid.put_u8(Cnct::SpecificData as u8);
        //     uid.put_u8(pk_chunk.len() as u8 + 1);
        //     uid.put_u8(i as u8);
        //     uid.put(pk_chunk);
        // }

        // let wire_crypt = "\x01\x00\x00\x00";

        // uid.put_u8(Cnct::ClientCrypt as u8);
        // uid.put_u8(wire_crypt.len() as u8);
        // uid.put(wire_crypt.as_bytes());

        // let usr = "username";

        // uid.put_u8(Cnct::User as u8);
        // uid.put_u8(usr.len() as u8);
        // uid.put(usr.as_bytes());

        // let host = "localhost";

        // uid.put_u8(Cnct::Host as u8);
        // uid.put_u8(host.len() as u8);
        // uid.put(host.as_bytes());

        // uid.put_u8(Cnct::UserVerification as u8);
        // uid.put_u8(0);

        uid.freeze()
    };

    put_wire_bytes(&mut connect, &uid);

    // Protocols
    for i in protocols.iter().flatten() {
        connect.put_u32(*i);
    }

    connect.freeze()
}

fn attach(db_name: &str, user: &str, pass: &str, protocol: u32) -> Bytes {
    let dpb = {
        let mut dpb = BytesMut::with_capacity(256);

        dpb.put_u8(1); //Version

        let charset = b"UTF-8";

        dpb.extend_from_slice(&[isc_dpb_lc_ctype as u8, charset.len() as u8]);
        dpb.extend_from_slice(charset);

        dpb.extend_from_slice(&[isc_dpb_user_name as u8, user.len() as u8]);
        dpb.extend_from_slice(user.as_bytes());

        if protocol < ProtocolVersion::V11 as u32 {
            dpb.extend_from_slice(&[isc_dpb_password as u8, pass.len() as u8]);
            dpb.extend_from_slice(pass.as_bytes());
        } else {
            #[allow(deprecated)]
            let enc_pass = pwhash::unix_crypt::hash_with("9z", pass).unwrap();
            let enc_pass = &enc_pass[2..];

            dpb.extend_from_slice(&[isc_dpb_password_enc as u8, enc_pass.len() as u8]);
            dpb.extend_from_slice(enc_pass.as_bytes());
        }

        dpb.freeze()
    };

    let mut attach = BytesMut::with_capacity(256);

    attach.put_u32(WireOp::Attach as u32);
    attach.put_u32(0); // Database Object ID

    put_wire_bytes(&mut attach, db_name.as_bytes());

    put_wire_bytes(&mut attach, &dpb);

    attach.freeze()
}

/// Parses the error messages from the response
fn parse_status_vector(resp: &mut Bytes) -> Result<(), FbError> {
    let mut sql_code = 0;
    let mut message = String::new();

    let mut gds_code = 0;
    let mut num_arg = 0;

    let mut n = resp.get_u32();

    while n != isc_arg_end {
        if n == isc_arg_gds {
            gds_code = resp.get_u32();

            if gds_code != 0 {
                message += gds_to_msg(gds_code);
                num_arg = 0;
            }
        } else if n == isc_arg_number {
            let num = resp.get_i32();
            if gds_code == 335544436 {
                sql_code = num
            }
            num_arg += 1;
            message = message.replace(&format!("@{}", num_arg), &format!("{}", num));
        } else if n == isc_arg_string {
            let msg = get_wire_bytes(resp);
            let msg = std::str::from_utf8(&msg[..]).unwrap_or("**Invalid message**");

            num_arg += 1;
            message = message.replace(&format!("@{}", num_arg), &msg);
        } else if n == isc_arg_interpreted {
            let msg = get_wire_bytes(resp);
            let msg = std::str::from_utf8(&msg[..]).unwrap_or("**Invalid message**");

            message += msg;
        } else if n == isc_arg_sql_state {
            let len = resp.get_u32() as usize;

            resp.advance(len as usize);
            if len % 4 != 0 {
                resp.advance(4 - (len % 4));
            }
        }

        n = resp.get_u32();
    }

    if message.ends_with('\n') {
        message.pop();
    }

    if !message.is_empty() {
        Err(FbError::Sql(SqlError {
            code: sql_code,
            msg: message,
        }))
    } else {
        Ok(())
    }
}

/// Put a u32 with the bytes length and the byte data
/// with padding to align for 4 bytes
fn put_wire_bytes(into: &mut impl BufMut, bytes: &[u8]) {
    let len = bytes.len() as usize;

    into.put_u32(len as u32);
    into.put(bytes);
    if len % 4 != 0 {
        into.put_slice(&[0; 4][..4 - (len % 4)]);
    }
}

/// Get the length of the bytes from the first u32
/// and return the bytes read, advancing the cursor
/// to align to 4 bytes
fn get_wire_bytes(from: &mut Bytes) -> Bytes {
    let len = from.get_u32() as usize;

    let mut bytes = from.clone();
    bytes.truncate(len);

    from.advance(len);
    if len % 4 != 0 {
        from.advance(4 - (len % 4));
    }

    bytes
}
