//!
//! Rust Firebird Client
//!
//! Wire protocol implementation
//!

#![allow(non_upper_case_globals)]

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::{
    convert::TryFrom,
    env,
    io::{Read, Write},
    net::TcpStream,
};

use super::{arc4::*, srp::*, *};
use crate::{
    params::ParamInfo,
    row::{ColumnBuffer, ColumnType},
    status::SqlError,
    xsqlda::{parse_xsqlda, PrepareInfo, XSqlVar, XSQLDA_DESCRIBE_VARS},
    Dialect, FbError,
};

/// Buffer length to use in the connection
const BUFFER_LENGTH: u32 = 1024;

/// Firebird tcp stream, may be encrypted
enum FbStream {
    /// Plaintext stream
    Plain(TcpStream),

    /// Arc4 ecrypted stream
    Arc4(Arc4Stream<TcpStream>),
}

impl Read for FbStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            FbStream::Plain(s) => s.read(buf),
            FbStream::Arc4(s) => s.read(buf),
        }
    }
}

impl Write for FbStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            FbStream::Plain(s) => s.write(buf),
            FbStream::Arc4(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            FbStream::Plain(s) => s.flush(),
            FbStream::Arc4(s) => s.flush(),
        }
    }
}

pub struct WireConnection {
    /// Connection socket
    socket: FbStream,

    /// Wire protocol version
    version: ProtocolVersion,

    /// Buffer to read the network data
    buff: Box<[u8]>,
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
        let socket = TcpStream::connect((host, port))?;

        // System username
        let username =
            env::var("USER").unwrap_or_else(|_| env::var("USERNAME").unwrap_or_default());
        let hostname = socket
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_default();

        let mut socket = FbStream::Plain(socket);

        let (req, srp) = connect(db_name, false, user, &username, &hostname);
        socket.write_all(&req)?;

        // May be a bit too much
        let mut buff = vec![0; BUFFER_LENGTH as usize * 2].into_boxed_slice();

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
                        let proof = hex::encode(verifier.get_proof());

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

                        socket = FbStream::Arc4(Arc4Stream::new(
                            match socket {
                                FbStream::Plain(s) => s,
                                _ => unreachable!("Stream was already encrypted!"),
                            },
                            &verifier.get_key(),
                            buff.len(),
                        ));

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
            .write_all(&attach(db_name, user, pass, self.version))?;

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
    ) -> Result<(StmtType, StmtHandle, Vec<XSqlVar>, usize), FbError> {
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

        let mut xsqlda = Vec::new();

        let mut resp = parse_response(&mut resp)?;
        let PrepareInfo {
            stmt_type,
            mut param_count,
            mut truncated,
        } = parse_xsqlda(&mut resp.data, &mut xsqlda)?;

        while truncated {
            // Get more info on the types
            self.socket
                .write_all(&info_sql(stmt_handle.0, xsqlda.len()))?;

            let mut data = self.read_response()?.data;

            let parse_resp = parse_xsqlda(&mut data, &mut xsqlda)?;
            truncated = parse_resp.truncated;
            param_count = parse_resp.param_count;
        }

        Ok((stmt_type, stmt_handle, xsqlda, param_count))
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
        params: &[ParamInfo],
    ) -> Result<(), FbError> {
        // TODO: Verify if parameter length match the sql
        let params = blr::params_to_blr(params, self.version)?;

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

        if op_code == WireOp::Response as u32 {
            // An error ocurred
            parse_response(&mut resp)?;
        }

        if op_code != WireOp::FetchResponse as u32 {
            return err_conn_rejected(op_code);
        }

        parse_fetch_response(&mut resp, xsqlda, self.version)
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
fn read_response(socket: &mut impl Read, buff: &mut [u8]) -> Result<Response, FbError> {
    let (op_code, mut resp) = read_packet(socket, buff)?;

    if op_code != WireOp::Response as u32 {
        return err_conn_rejected(op_code);
    }

    parse_response(&mut resp)
}

/// Reads a packet from the socket
fn read_packet(socket: &mut impl Read, buff: &mut [u8]) -> Result<(u32, Bytes), FbError> {
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
    let seed: [u8; 32] = rand::random();
    let srp = SrpClient::<sha1::Sha1>::new(&seed, &SRP_GROUP);

    let uid = {
        let mut uid = BytesMut::new();

        let pubkey = hex::encode(srp.get_a_pub());

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
fn attach(db_name: &str, user: &str, pass: &str, protocol: ProtocolVersion) -> Bytes {
    let dpb = {
        let mut dpb = BytesMut::with_capacity(64);

        dpb.put_u8(1); //Version

        let charset = b"UTF8";

        dpb.put_slice(&[isc_dpb_lc_ctype as u8, charset.len() as u8]);
        dpb.put_slice(charset);

        dpb.put_slice(&[isc_dpb_user_name as u8, user.len() as u8]);
        dpb.put_slice(user.as_bytes());

        match protocol {
            // Plaintext password
            ProtocolVersion::V10 => {
                dpb.put_slice(&[isc_dpb_password as u8, pass.len() as u8]);
                dpb.put_slice(pass.as_bytes());
            }

            // Hashed password
            ProtocolVersion::V11 | ProtocolVersion::V12 => {
                #[allow(deprecated)]
                let enc_pass = pwhash::unix_crypt::hash_with("9z", pass).unwrap();
                let enc_pass = &enc_pass[2..];

                dpb.put_slice(&[isc_dpb_password_enc as u8, enc_pass.len() as u8]);
                dpb.put_slice(enc_pass.as_bytes());
            }

            // Password already verified
            ProtocolVersion::V13 => {}
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
    req.put_wire_bytes(&XSQLDA_DESCRIBE_VARS); // Data to be returned

    req.put_u32(BUFFER_LENGTH);

    req.freeze()
}

/// Statement information request, to continue a truncated prepare statement xsqlda response
fn info_sql(stmt_handle: u32, next_index: usize) -> Bytes {
    let mut req = BytesMut::with_capacity(24 + XSQLDA_DESCRIBE_VARS.len());

    let next_index = (next_index as u16).to_le_bytes();

    req.put_u32(WireOp::InfoSql as u32);
    req.put_u32(stmt_handle);
    req.put_u32(0); // Incarnation of object
    req.put_wire_bytes(
        &[
            &[
                isc_info_sql_sqlda_start as u8, // Describe a xsqlda
                2,
                next_index[0], // Index, first byte
                next_index[1], // Index, second byte
            ],
            &XSQLDA_DESCRIBE_VARS[..], // Data to be returned
        ]
        .concat(),
    );
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
    version: ProtocolVersion,
) -> Result<Option<Vec<Option<ColumnBuffer>>>, FbError> {
    const END_OF_STREAM: u32 = 100;

    if resp.remaining() < 8 {
        return err_invalid_response();
    }

    let status = resp.get_u32();

    let has_row = resp.get_u32() != 0;
    if !has_row && status != END_OF_STREAM {
        return Err("Fetch returned no columns".into());
    }

    if status == END_OF_STREAM {
        return Ok(None);
    }

    let null_map = if version >= ProtocolVersion::V13 {
        // Read the null bitmap, 8 columns per byte
        let mut len = xsqlda.len() / 8;
        len += if xsqlda.len() % 8 == 0 { 0 } else { 1 };
        if len % 4 != 0 {
            // Align to 4 bytes
            len += 4 - (len % 4);
        }

        if resp.remaining() < len {
            return err_invalid_response();
        }
        let null_map = resp.slice(..len);
        resp.advance(len);

        Some(null_map)
    } else {
        None
    };

    let read_null = |resp: &mut Bytes, i: usize| {
        if version >= ProtocolVersion::V13 {
            // read from the null bitmap
            Ok((null_map.as_ref().unwrap()[i / 8] >> (i % 8)) & 1 != 0)
        } else {
            // read from the response
            if resp.remaining() < 4 {
                return err_invalid_response();
            }
            Ok(resp.get_u32() != 0)
        }
    };

    let mut data = Vec::with_capacity(xsqlda.len());

    for (i, var) in xsqlda.iter().enumerate() {
        let column_type = var.to_column_type()?;

        if version >= ProtocolVersion::V13 && read_null(resp, i)? {
            // There is no data in protocol 13 if null, so just continue
            data.push(None);
            continue;
        }

        match column_type {
            t @ ColumnType::Text => {
                let d = resp.get_wire_bytes()?;

                let null = read_null(resp, i)?;
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

                let d = resp.slice(..len);
                resp.advance(len);

                let null = read_null(resp, i)?;
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
    let salt = resp.slice(..len);
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
    let mut pub_key = resp.slice(..len).to_vec();
    if len % 2 != 0 {
        // We need to add a 0 to the start
        pub_key = [b"0", &pub_key[..]].concat();
    }
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
        let bytes = self.slice(..len);

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

    let (stmt_type, stmt_handle, mut xsqlda, param_count) = conn
        .prepare_statement(
            db_handle,
            tr_handle,
            Dialect::D3,
            "
            SELECT
                1, 'abcdefghij' as tst, rand(), CURRENT_DATE, CURRENT_TIME, CURRENT_TIMESTAMP, -1, -2, -3, -4, -5, 1, 2, 3, 4, 5, 0 as last
            FROM RDB$DATABASE where 1 = ?
            ",
            // "
            // SELECT cast(1 as bigint), cast('abcdefghij' as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            // SELECT cast(2 as bigint), cast('abcdefgh' as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            // SELECT cast(3 as bigint), cast('abcdef' as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            // SELECT cast(4 as bigint), cast(null as varchar(10)) as tst FROM RDB$DATABASE UNION ALL
            // SELECT cast(null as bigint), cast('abcd' as varchar(10)) as tst FROM RDB$DATABASE
            // ",
        )
        .unwrap();

    println!("XSqlDa: {:#?}", xsqlda);
    println!("StmtType: {:?}", stmt_type);
    for var in &mut xsqlda {
        var.coerce().unwrap();
    }
    println!("Coerced XSqlDa: {:#?}", xsqlda);

    let params = crate::params::IntoParams::to_params(("1",));
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
