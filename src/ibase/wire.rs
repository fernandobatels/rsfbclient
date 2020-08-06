//!
//! Rust Firebird Client
//!
//! Wire protocol implementation
//!

#![allow(non_upper_case_globals)]

use bytes::{Buf, BufMut, Bytes, BytesMut};
use lazy_static::lazy_static;
use num_bigint::BigUint;
use std::{
    convert::TryFrom,
    env,
    io::{Read, Write},
    net::TcpStream,
};

use super::{srp::*, *};
use crate::{
    params::Params,
    row::{ColumnBuffer, ColumnType},
    status::SqlError,
    xsqlda::{parse_xsqlda, XSqlVar, XSQLDA_DESCRIBE_VARS},
    Dialect, FbError,
};

/// Buffer length to use in the connection
const BUFFER_LENGTH: u32 = 1024;

lazy_static! {
    /// Srp Group used by the firebird server
    static ref SRP_GROUP: SrpGroup = SrpGroup {
        n: BigUint::from_bytes_be(&[
            230, 125, 46, 153, 75, 47, 144, 12, 63, 65, 240, 143, 91, 178, 98, 126, 208, 212, 158,
            225, 254, 118, 122, 82, 239, 205, 86, 92, 214, 231, 104, 129, 44, 62, 30, 156, 232,
            240, 168, 190, 166, 203, 19, 205, 41, 221, 235, 247, 169, 109, 74, 147, 181, 93, 72,
            141, 240, 153, 161, 92, 137, 220, 176, 100, 7, 56, 235, 44, 189, 217, 168, 247, 186,
            181, 97, 171, 27, 13, 193, 198, 205, 171, 243, 3, 38, 74, 8, 209, 188, 169, 50, 209,
            241, 238, 66, 139, 97, 157, 151, 15, 52, 42, 186, 154, 101, 121, 59, 139, 47, 4, 26,
            229, 54, 67, 80, 193, 111, 115, 95, 86, 236, 188, 168, 123, 213, 123, 41, 231,
        ]),
        g: BigUint::from_bytes_be(&[2]),
    };
}

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
    pub fn connect(
        host: &str,
        port: u16,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<Self, FbError> {
        let mut socket = TcpStream::connect((host, port))?;

        // System username
        let username =
            env::var("USER").unwrap_or_else(|_| env::var("USERNAME").unwrap_or_default());
        let hostname = socket
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_default();

        let (req, srp) = connect(db_name, false, user, &username, &hostname);
        socket.write_all(&req)?;

        // May be a bit too much
        let mut buff = vec![0; BUFFER_LENGTH as usize * 2];

        let len = socket.read(&mut buff)?;
        let mut resp = Bytes::copy_from_slice(&buff[..len]);

        let ConnectionResponse {
            version,
            auth_plugin,
        } = parse_accept(&mut resp)?;

        if let Some(AuthPlugin { kind, data, .. }) = auth_plugin {
            match kind {
                plugin @ AuthPluginType::Srp256 | plugin @ AuthPluginType::Srp => {
                    if let Some(data) = data {
                        // Generate a private key with the salt received from the server
                        let private_key = srp_private_key::<sha1::Sha1>(
                            user.as_bytes(),
                            pass.as_bytes(),
                            &data.salt,
                        );

                        // Generate a verified with the private key above and the server public key received
                        let verifier = srp
                            .process_reply(user.as_bytes(), &data.salt, &private_key, &data.pub_key)
                            .map_err(|e| FbError::from(format!("Srp error: {}", e)))?;

                        // Generate a proof to send to the server so it can verify the password
                        let proof = hex::encode_upper(verifier.get_proof());

                        // Send proof data
                        socket.write_all(&cont_auth(
                            &proof.as_bytes(),
                            plugin,
                            AuthPluginType::plugin_list(),
                            &[],
                        ))?;

                        read_response(&mut socket, &mut buff)?;

                        // Enable wire encryption
                        socket.write_all(&crypt("Arc4", "Symmetric"))?;

                        read_response(&mut socket, &mut buff)?;
                    } else {
                        todo!("Not sure what to do")
                    }
                }
                AuthPluginType::Legacy => {}
            }
        }

        Ok(Self {
            socket,
            version,
            buff,
        })
    }

    /// Connect to a database, returning a database handle
    pub fn attach_database(
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

    /// Disconnect from the database
    pub fn detach_database(&mut self, db_handle: DbHandle) -> Result<(), FbError> {
        self.socket.write_all(&detach(db_handle.0))?;

        self.read_response()?;

        Ok(())
    }

    /// Drop the database
    pub fn drop_database(&mut self, db_handle: DbHandle) -> Result<(), FbError> {
        self.socket.write_all(&drop_database(db_handle.0))?;

        self.read_response()?;

        Ok(())
    }

    /// Start a new transaction, with the specified transaction parameter buffer
    pub fn begin_transaction(
        &mut self,
        db_handle: DbHandle,
        tpb: &[u8],
    ) -> Result<TrHandle, FbError> {
        self.socket
            .write_all(&transaction(db_handle.0, tpb))
            .unwrap();

        let resp = self.read_response()?;

        Ok(TrHandle(resp.handle))
    }

    /// Commit / Rollback a transaction
    pub fn transaction_operation(&mut self, tr_handle: TrHandle, op: TrOp) -> Result<(), FbError> {
        self.socket
            .write_all(&transaction_operation(tr_handle.0, op))?;

        self.read_response()?;

        Ok(())
    }

    /// Execute a sql immediately, without returning rows
    pub fn exec_immediate(
        &mut self,
        tr_handle: TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError> {
        self.socket
            .write_all(&exec_immediate(tr_handle.0, dialect as u32, sql))
            .unwrap();

        self.read_response()?;

        Ok(())
    }

    /// Alloc and prepare a statement
    ///
    /// Returns the statement type, handle and xsqlda describing the columns
    pub fn prepare_statement(
        &mut self,
        db_handle: DbHandle,
        tr_handle: TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, StmtHandle, Vec<XSqlVar>), FbError> {
        // Alloc statement
        self.socket.write_all(&allocate_statement(db_handle.0))?;

        // Prepare statement
        self.socket.write_all(&prepare_statement(
            tr_handle.0,
            u32::MAX,
            dialect as u32,
            sql,
        ))?;

        let (op_code, mut resp) = self.read_packet()?;

        // Alloc resp
        if op_code != WireOp::Response as u32 {
            return err_conn_rejected(op_code);
        }

        let stmt_handle = StmtHandle(parse_response(&mut resp)?.handle);

        // Prepare resp
        if resp.remaining() < 4 {
            return err_invalid_response();
        }
        let op_code = resp.get_u32();

        if op_code != WireOp::Response as u32 {
            return err_conn_rejected(op_code);
        }

        let resp = parse_response(&mut resp)?;
        let (stmt_type, xsqlda, _truncated) = parse_xsqlda(&resp.data)?;
        // TODO: handle truncated

        Ok((stmt_type, stmt_handle, xsqlda))
    }

    /// Closes or drops a statement
    pub fn free_statement(
        &mut self,
        stmt_handle: StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        self.socket.write_all(&free_statement(stmt_handle.0, op))?;
        // Obs.: Lazy response
        Ok(())
    }

    /// Execute the prepared statement with parameters
    pub fn execute(
        &mut self,
        tr_handle: TrHandle,
        stmt_handle: StmtHandle,
        params: &Params,
    ) -> Result<(), FbError> {
        self.socket
            .write_all(&execute(
                tr_handle.0,
                stmt_handle.0,
                &params.blr,
                &params.values,
            ))
            .unwrap();

        self.read_response()?;

        Ok(())
    }

    /// Fetch rows from the executed statement, coercing the types
    /// according to the provided blr
    pub fn fetch(
        &mut self,
        stmt_handle: StmtHandle,
        xsqlda: &[XSqlVar],
        blr: &[u8],
    ) -> Result<Option<Vec<Option<ColumnBuffer>>>, FbError> {
        self.socket.write_all(&fetch(stmt_handle.0, &blr))?;

        let (op_code, mut resp) = self.read_packet()?;

        if op_code != WireOp::FetchResponse as u32 {
            return err_conn_rejected(op_code);
        }

        parse_fetch_response(&mut resp, xsqlda)
    }

    /// Read a server response
    fn read_response(&mut self) -> Result<Response, FbError> {
        read_response(&mut self.socket, &mut self.buff)
    }

    /// Reads a packet from the socket
    fn read_packet(&mut self) -> Result<(u32, Bytes), FbError> {
        read_packet(&mut self.socket, &mut self.buff)
    }
}

/// Read a server response
fn read_response(socket: &mut TcpStream, buff: &mut [u8]) -> Result<Response, FbError> {
    let (op_code, mut resp) = read_packet(socket, buff)?;

    if op_code != WireOp::Response as u32 {
        return err_conn_rejected(op_code);
    }

    parse_response(&mut resp)
}

/// Reads a packet from the socket
fn read_packet(socket: &mut TcpStream, buff: &mut [u8]) -> Result<(u32, Bytes), FbError> {
    let len = socket.read(buff)?;
    let mut resp = Bytes::copy_from_slice(&buff[..len]);

    let op_code = loop {
        if resp.remaining() < 4 {
            return err_invalid_response();
        }
        let op_code = resp.get_u32();
        if op_code != WireOp::Dummy as u32 {
            break op_code;
        }
    };

    Ok((op_code, resp))
}

#[derive(Debug, Clone, Copy)]
/// A database handle
pub struct DbHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A transaction handle
pub struct TrHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A statement handle
/// (pub on field to help in testing the `StmtCache`)
pub struct StmtHandle(pub(crate) u32);

fn err_conn_rejected<T>(op_code: u32) -> Result<T, FbError> {
    Err(format!(
        "Connection rejected with code {}{}",
        op_code,
        WireOp::try_from(op_code as u8)
            .map(|op| format!(" ({:?})", op))
            .unwrap_or_default()
    )
    .into())
}

#[test]
fn connection_test() {
    let db_name = "test.fdb";
    let user = "SYSDBA";
    let pass = "masterkey";

    let mut conn = WireConnection::connect("127.0.0.1", 3050, db_name, user, pass).unwrap();

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
            Dialect::D3,
            // "SELECT cast(1 as bigint), cast('abcdefghij' as varchar(10)) as tst FROM RDB$DATABASE where 1 = ?",
            "
            SELECT cast(1 as bigint), cast('abcdefghij' as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            SELECT cast(2 as bigint), cast('abcdefgh' as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            SELECT cast(3 as bigint), cast('abcdef' as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            SELECT cast(4 as bigint), cast(null as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            SELECT cast(null as bigint), cast('abcd' as varchar(10)) as tst FROM RDB$DATABASE
            ",
        )
        .unwrap();

    println!("{:#?}", xsqlda);
    println!("StmtType: {:?}", stmt_type);

    let params =
        crate::params::params_to_blr(&crate::params::IntoParams::to_params(("1",))).unwrap();
    let output_blr = crate::xsqlda::xsqlda_to_blr(&xsqlda).unwrap();

    conn.execute(tr_handle, stmt_handle, &params).unwrap();

    loop {
        let resp = conn.fetch(stmt_handle, &xsqlda, &output_blr).unwrap();

        if resp.is_none() {
            break;
        }
        println!("Fetch Resp: {:?}", resp);
    }

    std::thread::sleep(std::time::Duration::from_millis(100));
}

/// Connection request
fn connect(
    db_name: &str,
    create_db: bool,
    user: &str,
    username: &str,
    hostname: &str,
) -> (Bytes, SrpClient<'static, sha1::Sha1>) {
    let protocols = [
        // PROTOCOL_VERSION, Arch type (Generic=1), min, max, weight
        [ProtocolVersion::V10 as u32, 1, 0, 5, 2],
        [ProtocolVersion::V11 as u32, 1, 0, 5, 4],
        [ProtocolVersion::V12 as u32, 1, 0, 5, 6],
        [ProtocolVersion::V13 as u32, 1, 0, 5, 8],
    ];

    let mut connect = BytesMut::with_capacity(256);

    connect.put_u32(WireOp::Connect as u32);
    connect.put_u32(if create_db {
        WireOp::Create
    } else {
        WireOp::Attach
    } as u32);
    connect.put_u32(3); // CONNECT_VERSION
    connect.put_u32(1); // arch_generic

    // Db file path / name
    connect.put_wire_bytes(db_name.as_bytes());

    // Protocol versions understood
    connect.put_u32(protocols.len() as u32);

    // Random seed for the srp
    // let seed: [u8; 32] = rand::random();
    let seed = [
        104, 168, 26, 157, 227, 194, 41, 70, 204, 234, 48, 50, 217, 147, 39, 186, 223, 61, 125,
        154, 223, 9, 54, 220, 163, 109, 222, 183, 78, 242, 217, 218,
    ];
    let srp = SrpClient::<sha1::Sha1>::new(&seed, &SRP_GROUP);

    let uid = {
        let mut uid = BytesMut::new();

        let pubkey = hex::encode_upper(srp.get_a_pub());

        // Database username
        uid.put_u8(Cnct::Login as u8);
        uid.put_u8(user.len() as u8);
        uid.put(user.as_bytes());

        let plugin = AuthPluginType::Srp.name();

        uid.put_u8(Cnct::PluginName as u8);
        uid.put_u8(plugin.len() as u8);
        uid.put(plugin.as_bytes());

        let plugin_list = AuthPluginType::plugin_list();

        uid.put_u8(Cnct::PluginList as u8);
        uid.put_u8(plugin_list.len() as u8);
        uid.put(plugin_list.as_bytes());

        for (i, pk_chunk) in pubkey.as_bytes().chunks(254).enumerate() {
            uid.put_u8(Cnct::SpecificData as u8);
            uid.put_u8(pk_chunk.len() as u8 + 1);
            uid.put_u8(i as u8);
            uid.put(pk_chunk);
        }

        let wire_crypt = "\x01\x00\x00\x00";

        uid.put_u8(Cnct::ClientCrypt as u8);
        uid.put_u8(wire_crypt.len() as u8);
        uid.put(wire_crypt.as_bytes());

        // System username
        uid.put_u8(Cnct::User as u8);
        uid.put_u8(username.len() as u8);
        uid.put(username.as_bytes());

        uid.put_u8(Cnct::Host as u8);
        uid.put_u8(hostname.len() as u8);
        uid.put(hostname.as_bytes());

        uid.put_u8(Cnct::UserVerification as u8);
        uid.put_u8(0);

        uid.freeze()
    };
    connect.put_wire_bytes(&uid);

    // Protocols
    for i in protocols.iter().flatten() {
        connect.put_u32(*i);
    }

    (connect.freeze(), srp)
}

/// Continue authentication request
fn cont_auth(data: &[u8], plugin: AuthPluginType, plugin_list: String, keys: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(
        20 + data.len() + plugin.name().len() + plugin_list.len() + keys.len(),
    );

    req.put_u32(WireOp::ContAuth as u32);
    req.put_wire_bytes(data);
    req.put_wire_bytes(plugin.name().as_bytes());
    req.put_wire_bytes(plugin_list.as_bytes());
    req.put_wire_bytes(keys);

    req.freeze()
}

/// Wire encryption request
fn crypt(algo: &str, kind: &str) -> Bytes {
    let mut req = BytesMut::with_capacity(12 + algo.len() + kind.len());

    req.put_u32(WireOp::Crypt as u32);
    // Encryption algorithm
    req.put_wire_bytes(algo.as_bytes());
    // Encryption type
    req.put_wire_bytes(kind.as_bytes());

    req.freeze()
}

/// Attach request
fn attach(db_name: &str, user: &str, pass: &str, protocol: u32) -> Bytes {
    let dpb = {
        let mut dpb = BytesMut::with_capacity(64);

        dpb.put_u8(1); //Version

        let charset = b"UTF8";

        dpb.put_slice(&[isc_dpb_lc_ctype as u8, charset.len() as u8]);
        dpb.put_slice(charset);

        dpb.put_slice(&[isc_dpb_user_name as u8, user.len() as u8]);
        dpb.put_slice(user.as_bytes());

        if protocol < ProtocolVersion::V11 as u32 {
            dpb.put_slice(&[isc_dpb_password as u8, pass.len() as u8]);
            dpb.put_slice(pass.as_bytes());
        } else {
            #[allow(deprecated)]
            let enc_pass = pwhash::unix_crypt::hash_with("9z", pass).unwrap();
            let enc_pass = &enc_pass[2..];

            dpb.put_slice(&[isc_dpb_password_enc as u8, enc_pass.len() as u8]);
            dpb.put_slice(enc_pass.as_bytes());
        }

        dpb.freeze()
    };

    let mut attach = BytesMut::with_capacity(16 + db_name.len() + dpb.len());

    attach.put_u32(WireOp::Attach as u32);
    attach.put_u32(0); // Database Object ID

    attach.put_wire_bytes(db_name.as_bytes());

    attach.put_wire_bytes(&dpb);

    attach.freeze()
}

/// Detach from the database request
fn detach(db_handle: u32) -> Bytes {
    let mut tr = BytesMut::with_capacity(8);

    tr.put_u32(WireOp::Detach as u32);
    tr.put_u32(db_handle);

    tr.freeze()
}

/// Drop database request
fn drop_database(db_handle: u32) -> Bytes {
    let mut tr = BytesMut::with_capacity(8);

    tr.put_u32(WireOp::DropDatabase as u32);
    tr.put_u32(db_handle);

    tr.freeze()
}

/// Begin transaction request
fn transaction(db_handle: u32, tpb: &[u8]) -> Bytes {
    let mut tr = BytesMut::with_capacity(12 + tpb.len());

    tr.put_u32(WireOp::Transaction as u32);
    tr.put_u32(db_handle);
    tr.put_wire_bytes(tpb);

    tr.freeze()
}

/// Commit / Rollback transaction request
fn transaction_operation(tr_handle: u32, op: TrOp) -> Bytes {
    let mut tr = BytesMut::with_capacity(8);

    tr.put_u32(op as u32);
    tr.put_u32(tr_handle);

    tr.freeze()
}

/// Execute immediate request
fn exec_immediate(tr_handle: u32, dialect: u32, sql: &str) -> Bytes {
    let mut req = BytesMut::with_capacity(28 + sql.len());

    req.put_u32(WireOp::ExecImmediate as u32);
    req.put_u32(tr_handle);
    req.put_u32(0); // Statement handle, apparently unused
    req.put_u32(dialect);
    req.put_wire_bytes(sql.as_bytes());
    req.put_u32(0); // TODO: parameters
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

/// Prepare statement request. Use u32::MAX as `stmt_handle` if the statement was allocated
/// in the previous request
fn prepare_statement(tr_handle: u32, stmt_handle: u32, dialect: u32, query: &str) -> Bytes {
    let mut req = BytesMut::with_capacity(28 + query.len() + XSQLDA_DESCRIBE_VARS.len());

    req.put_u32(WireOp::PrepareStatement as u32);
    req.put_u32(tr_handle);
    req.put_u32(stmt_handle);
    req.put_u32(dialect);
    req.put_wire_bytes(query.as_bytes());
    req.put_wire_bytes(&XSQLDA_DESCRIBE_VARS);

    req.put_u32(BUFFER_LENGTH);

    req.freeze()
}

/// Close or drop statement request
fn free_statement(stmt_handle: u32, op: FreeStmtOp) -> Bytes {
    let mut req = BytesMut::with_capacity(12);

    req.put_u32(WireOp::FreeStatement as u32);
    req.put_u32(stmt_handle);
    req.put_u32(op as u32);

    req.freeze()
}

/// Execute prepared statement request.
fn execute(tr_handle: u32, stmt_handle: u32, input_blr: &[u8], input_data: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(36 + input_blr.len() + input_data.len());

    req.put_u32(WireOp::Execute as u32);
    req.put_u32(stmt_handle);
    req.put_u32(tr_handle);

    req.put_wire_bytes(input_blr);
    req.put_u32(0);
    req.put_u32(if input_blr.is_empty() { 0 } else { 1 });

    req.put_slice(input_data);

    req.freeze()
}

/// Fetch row request
fn fetch(stmt_handle: u32, blr: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(20 + blr.len());

    req.put_u32(WireOp::Fetch as u32);
    req.put_u32(stmt_handle);
    req.put_wire_bytes(blr);
    req.put_u32(0); // Message number
    req.put_u32(1); // Message count TODO: increase to return more rows in one fetch request

    req.freeze()
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
    if resp.remaining() < 12 {
        return err_invalid_response();
    }
    let handle = resp.get_u32();
    let object_id = resp.get_u64();

    let data = resp.get_wire_bytes()?;

    parse_status_vector(resp)?;

    Ok(Response {
        handle,
        object_id,
        data,
    })
}

/// Parse a server sql response (`WireOp::FetchResponse`)
fn parse_fetch_response(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
) -> Result<Option<Vec<Option<ColumnBuffer>>>, FbError> {
    const STATUS_EOS: u32 = 100;

    if resp.remaining() < 8 {
        return err_invalid_response();
    }

    let status = resp.get_u32();

    let has_row = resp.get_u32() != 0;
    if !has_row && status != STATUS_EOS {
        return Err("Fetch returned no columns".into());
    }

    if status == STATUS_EOS {
        return Ok(None);
    }

    let mut data = Vec::with_capacity(xsqlda.len());

    for var in xsqlda {
        let column_type = var.to_column_type()?;

        match column_type {
            t @ ColumnType::Text => {
                let d = resp.get_wire_bytes()?;

                if resp.remaining() < 4 {
                    return err_invalid_response();
                }

                let null = resp.get_u32() != 0;
                if null {
                    data.push(None)
                } else {
                    data.push(Some(ColumnBuffer { kind: t, buffer: d }))
                }
            }
            t @ ColumnType::Integer | t @ ColumnType::Float | t @ ColumnType::Timestamp => {
                let len = 8;

                if resp.remaining() < len {
                    return err_invalid_response();
                }

                let mut d = resp.clone();
                d.truncate(len);
                resp.advance(len);

                let null = resp.get_u32() != 0;
                if null {
                    data.push(None)
                } else {
                    data.push(Some(ColumnBuffer { kind: t, buffer: d }))
                }
            }
        }
    }

    Ok(Some(data))
}

/// Parses the error messages from the response
fn parse_status_vector(resp: &mut Bytes) -> Result<(), FbError> {
    // Sql error code (default to -1)
    let mut sql_code = -1;
    // Error messages
    let mut message = String::new();

    // Code of the last error message
    let mut gds_code = 0;
    // Error message argument index
    let mut num_arg = 0;

    loop {
        if resp.remaining() < 4 {
            return err_invalid_response();
        }

        match resp.get_u32() {
            // New error message
            isc_arg_gds => {
                gds_code = resp.get_u32();

                if gds_code != 0 {
                    message += gds_to_msg(gds_code);
                    num_arg = 0;
                }
            }

            // Error message arg number
            isc_arg_number => {
                let num = resp.get_i32();
                // Sql error code
                if gds_code == 335544436 {
                    sql_code = num
                }

                num_arg += 1;
                message = message.replace(&format!("@{}", num_arg), &format!("{}", num));
            }

            // Error message arg string
            isc_arg_string => {
                let msg = resp.get_wire_bytes()?;
                let msg = std::str::from_utf8(&msg[..]).unwrap_or("**Invalid message**");

                num_arg += 1;
                message = message.replace(&format!("@{}", num_arg), &msg);
            }

            // Aditional error message string
            isc_arg_interpreted => {
                let msg = resp.get_wire_bytes()?;
                let msg = std::str::from_utf8(&msg[..]).unwrap_or("**Invalid message**");

                message += msg;
            }

            isc_arg_sql_state => {
                resp.get_wire_bytes()?;
            }

            // End of error messages
            isc_arg_end => break,

            cod => {
                return Err(format!("Invalid / Unknown status vector item: {}", cod).into());
            }
        }
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

#[derive(Debug)]
/// Data from the response of a connection request
struct ConnectionResponse {
    version: ProtocolVersion,
    auth_plugin: Option<AuthPlugin>,
}

#[derive(Debug)]
struct AuthPlugin {
    kind: AuthPluginType,
    data: Option<SrpAuthData>,
    keys: Bytes,
}

/// Parse the connect response response (`WireOp::Accept`, `WireOp::AcceptData`, `WireOp::CondAccept` )
fn parse_accept(resp: &mut Bytes) -> Result<ConnectionResponse, FbError> {
    if resp.remaining() < 4 {
        return err_invalid_response();
    }
    let op_code = resp.get_u32();

    if op_code == WireOp::Response as u32 {
        // Returned an error
        parse_response(resp)?;
    }

    if op_code != WireOp::Accept as u32
        && op_code != WireOp::AcceptData as u32
        && op_code != WireOp::CondAccept as u32
    {
        return err_conn_rejected(op_code);
    }

    if resp.remaining() < 12 {
        return err_invalid_response();
    }

    let version =
        ProtocolVersion::try_from(resp.get_u32()).map_err(|e| FbError::Other(e.to_string()))?;
    resp.get_u32(); // Arch
    resp.get_u32(); // Type

    let auth_plugin =
        if op_code == WireOp::AcceptData as u32 || op_code == WireOp::CondAccept as u32 {
            let auth_data = parse_srp_auth_data(&mut resp.get_wire_bytes()?)?;

            let plugin = AuthPluginType::parse(&resp.get_wire_bytes()?)?;

            if resp.remaining() < 4 {
                return err_invalid_response();
            }
            let authenticated = resp.get_u32() != 0;

            let keys = resp.get_wire_bytes()?;

            if authenticated {
                None
            } else {
                Some(AuthPlugin {
                    kind: plugin,
                    data: auth_data,
                    keys,
                })
            }
        } else {
            None
        };

    Ok(ConnectionResponse {
        version,
        auth_plugin,
    })
}

#[derive(Debug)]
struct SrpAuthData {
    salt: Box<[u8]>,
    pub_key: Box<[u8]>,
}

/// Parse the auth data from the Srp / Srp256 plugin
fn parse_srp_auth_data(resp: &mut Bytes) -> Result<Option<SrpAuthData>, FbError> {
    if resp.is_empty() {
        return Ok(None);
    }

    if resp.remaining() < 2 {
        return err_invalid_response();
    }
    let len = resp.get_u16_le() as usize;
    if resp.remaining() < len {
        return err_invalid_response();
    }
    let mut salt = resp.clone();
    salt.truncate(len);
    // * DO NOT PARSE AS HEXADECIMAL *
    let salt = salt.to_vec();
    resp.advance(len);

    if resp.remaining() < 2 {
        return err_invalid_response();
    }
    let len = resp.get_u16_le() as usize;
    if resp.remaining() < len {
        return err_invalid_response();
    }
    let mut pub_key = resp.clone();
    pub_key.truncate(len);
    let pub_key =
        hex::decode(&pub_key).map_err(|_| FbError::from("Invalid hex pub_key in srp data"))?;
    resp.advance(len);

    Ok(Some(SrpAuthData {
        salt: salt.into_boxed_slice(),
        pub_key: pub_key.into_boxed_slice(),
    }))
}

trait BufMutWireExt: BufMut {
    /// Put a u32 with the bytes length and the byte data
    /// with padding to align for 4 bytes
    fn put_wire_bytes(&mut self, bytes: &[u8])
    where
        Self: Sized,
    {
        let len = bytes.len() as usize;

        self.put_u32(len as u32);
        self.put(bytes);
        if len % 4 != 0 {
            self.put_slice(&[0; 4][..4 - (len % 4)]);
        }
    }
}

impl<T> BufMutWireExt for T where T: BufMut {}

trait BytesWireExt {
    /// Get the length of the bytes from the first u32
    /// and return the bytes read, advancing the cursor
    /// to align to 4 bytes
    fn get_wire_bytes(&mut self) -> Result<Bytes, FbError>;
}

impl BytesWireExt for Bytes {
    fn get_wire_bytes(&mut self) -> Result<Bytes, FbError> {
        if self.remaining() < 4 {
            return err_invalid_response();
        }
        let len = self.get_u32() as usize;

        if self.remaining() < len {
            return err_invalid_response();
        }
        let mut bytes = self.clone();
        bytes.truncate(len);

        self.advance(len);
        if len % 4 != 0 {
            self.advance(4 - (len % 4));
        }

        Ok(bytes)
    }
}

fn err_invalid_response<T>() -> Result<T, FbError> {
    Err("Invalid server response, missing bytes".into())
}

#[cfg(test)]
mod test {
    use super::SRP_GROUP;
    use crate::ibase::srp::{srp_private_key, SrpClient};
    use num_bigint::BigUint;
    use sha1::Sha1;

    #[test]
    fn srp_group_k_test() {
        use sha1::Digest;

        let k = {
            let n = SRP_GROUP.n.to_bytes_be();
            let g_bytes = SRP_GROUP.g.to_bytes_be();
            let mut buf = vec![0u8; n.len()];
            let l = n.len() - g_bytes.len();
            buf[l..].copy_from_slice(&g_bytes);

            BigUint::from_bytes_be(&sha1::Sha1::new().chain(&n).chain(&buf).finalize())
        };

        assert_eq!(
            "1277432915985975349439481660349303019122249719989",
            &k.to_string()
        );
    }

    #[test]
    fn srp_vals_test() {
        let user = b"sysdba";
        let password = b"masterkey";

        // Real one randomly generated
        let seed = [
            104, 168, 26, 157, 227, 194, 41, 70, 204, 234, 48, 50, 217, 147, 39, 186, 223, 61, 125,
            154, 223, 9, 54, 220, 163, 109, 222, 183, 78, 242, 217, 218,
        ];

        let cli = SrpClient::<Sha1>::new(&seed, &SRP_GROUP);

        assert_eq!(
            cli.get_a_pub(),
            BigUint::parse_bytes(
                b"140881421499567234926370707691929201584335514055692587180102084646282810733160001237892692305806957785292091467614922078328787082920091583399296847456481914076730273969778307678896596634071762017513173403243965936903761580099023780256639030075360658492420403842461445358536578442895018174380364815053686107255"
                , 10
            ).unwrap().to_bytes_be()
        );

        // Real ones are received from server
        let salt = b"9\xe0\xee\x06\xa9]\xbe\xa7\xe4V\x08\xb1g\xa1\x93\x19\xf6\x11\xcb@\t\xeb\x9c\xf8\xe5K_;\xd1\xeb\x0f\xde";
        let serv_pub = BigUint::parse_bytes(
            b"9664511961170061978805668776377548609867359536792555459451373100540811860853826881772164535593386333263225393199902079347793807335504376938377762257920751005873533468177562614066508611409115917792525726727162676806787115902775303095022305576987173568527110065130456437265884455358297687922316181717357090556", 
            10
        ).unwrap().to_bytes_be();

        let cli_priv = srp_private_key::<Sha1>(user, password, salt);

        assert_eq!(
            b"\xe7\xd1>*\xaag\x9a\xa9\"w\x17&>\xca\xff\x86+ '\xdc",
            &cli_priv[..]
        );

        let verifier = cli.process_reply(user, salt, &cli_priv, &serv_pub).unwrap();

        assert_eq!(
            b"C~\xe6\xad\xe1\x97d\xed\xbf\x16D7\xb1C\xbf\xb1\xc9\x92\xc4@",
            &verifier.get_proof()[..]
        );

        assert_eq!(
            b"\xd5,\xe6(\xf6\x04\xec\xdb\xf2\xa2J\xc8zw\xb0\x9a\x87O\xe8\xf7",
            &verifier.get_key()[..]
        );
    }
}
