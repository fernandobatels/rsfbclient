use bytes::{Buf, Bytes};
use std::{
    collections::HashMap,
    env,
    io::{Read, Write},
    net::TcpStream,
};

use crate::{
    arc4::*,
    blr,
    consts::{AuthPluginType, ProtocolVersion, WireOp},
    srp::*,
    wire::*,
    xsqlda::{parse_xsqlda, xsqlda_to_blr, PrepareInfo, XSqlVar},
};
use rsfbclient_core::{
    ibase, Column, Dialect, FbError, FirebirdClient, FirebirdClientRemoteAttach, FreeStmtOp, Param,
    StmtType, TrIsolationLevel, TrOp,
};

/// Firebird client implemented in rust
pub struct RustFbClient {
    conn: Option<FirebirdWireConnection>,
}

/// A Connection to a firebird server
struct FirebirdWireConnection {
    /// Connection socket
    socket: FbStream,

    /// Wire protocol version
    version: ProtocolVersion,

    /// Buffer to read the network data
    buff: Box<[u8]>,

    /// Data for the prepared statements
    stmt_data_map: HashMap<StmtHandle, StmtData>,
}

/// Data to keep track about a prepared statement
struct StmtData {
    /// Output xsqlda
    xsqlda: Vec<XSqlVar>,
    /// Blr representation of the above
    blr: Bytes,
    /// Number of parameters
    param_count: usize,
}

impl FirebirdClientRemoteAttach for RustFbClient {
    /// Attach to a database, creating the connections if necessary.
    ///
    /// It will only connect only once, so calling a second time with different
    /// host or port will still use the old connection.
    fn attach_database(
        &mut self,
        host: &str,
        port: u16,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<Self::DbHandle, FbError> {
        // Take the existing connection, or connects
        let mut conn = match self.conn.take() {
            Some(conn) => conn,
            None => FirebirdWireConnection::connect(host, port, db_name, user, pass)?,
        };

        let attach_result = conn.attach_database(db_name, user, pass);

        // Put the connection back
        self.conn.replace(conn);

        attach_result
    }
}

impl FirebirdClient for RustFbClient {
    type DbHandle = DbHandle;
    type TrHandle = TrHandle;
    type StmtHandle = StmtHandle;

    type Args = ();

    fn new(_args: Self::Args) -> Result<Self, FbError>
    where
        Self: Sized,
    {
        Ok(Self { conn: None })
    }

    fn detach_database(&mut self, db_handle: Self::DbHandle) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.detach_database(db_handle))
            .unwrap_or_else(err_client_not_connected)
    }

    fn drop_database(&mut self, db_handle: Self::DbHandle) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.drop_database(db_handle))
            .unwrap_or_else(err_client_not_connected)
    }

    fn begin_transaction(
        &mut self,
        db_handle: Self::DbHandle,
        isolation_level: TrIsolationLevel,
    ) -> Result<Self::TrHandle, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.begin_transaction(db_handle, isolation_level))
            .unwrap_or_else(err_client_not_connected)
    }

    fn transaction_operation(
        &mut self,
        tr_handle: Self::TrHandle,
        op: TrOp,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.transaction_operation(tr_handle, op))
            .unwrap_or_else(err_client_not_connected)
    }

    fn exec_immediate(
        &mut self,
        _db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.exec_immediate(tr_handle, dialect, sql))
            .unwrap_or_else(err_client_not_connected)
    }

    fn prepare_statement(
        &mut self,
        db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, Self::StmtHandle), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.prepare_statement(db_handle, tr_handle, dialect, sql))
            .unwrap_or_else(err_client_not_connected)
    }

    fn free_statement(
        &mut self,
        stmt_handle: Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.free_statement(stmt_handle, op))
            .unwrap_or_else(err_client_not_connected)
    }

    fn execute(
        &mut self,
        tr_handle: Self::TrHandle,
        stmt_handle: Self::StmtHandle,
        params: Vec<Param>,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.execute(tr_handle, stmt_handle, &params))
            .unwrap_or_else(err_client_not_connected)
    }

    fn fetch(&mut self, stmt_handle: Self::StmtHandle) -> Result<Option<Vec<Column>>, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.fetch(stmt_handle))
            .unwrap_or_else(err_client_not_connected)
    }
}

fn err_client_not_connected<T>() -> Result<T, FbError> {
    Err("Client not connected to the server, call `attach_database` to connect".into())
}

impl FirebirdWireConnection {
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

        // Random key for the srp
        let srp_key: [u8; 32] = rand::random();

        let req = connect(db_name, false, user, &username, &hostname, &srp_key);
        socket.write_all(&req)?;
        socket.flush()?;

        // May be a bit too much
        let mut buff = vec![0; BUFFER_LENGTH as usize * 2].into_boxed_slice();

        let len = socket.read(&mut buff)?;
        let mut resp = Bytes::copy_from_slice(&buff[..len]);

        let ConnectionResponse {
            version,
            auth_plugin,
        } = parse_accept(&mut resp)?;

        if let Some(mut auth_plugin) = auth_plugin {
            loop {
                match auth_plugin.kind {
                    plugin @ AuthPluginType::Srp => {
                        let srp = SrpClient::<sha1::Sha1>::new(&srp_key, &SRP_GROUP);

                        if let Some(data) = auth_plugin.data {
                            socket = srp_auth(socket, &mut buff, srp, plugin, user, pass, data)?;

                            // Authentication Ok
                            break;
                        } else {
                            // Server requested a different authentication method than the client specified
                            // in the initial connection

                            socket.write_all(&cont_auth(
                                hex::encode(srp.get_a_pub()).as_bytes(),
                                plugin,
                                AuthPluginType::plugin_list(),
                                &[],
                            ))?;
                            socket.flush()?;

                            let len = socket.read(&mut buff)?;
                            let mut resp = Bytes::copy_from_slice(&buff[..len]);

                            auth_plugin = parse_cont_auth(&mut resp)?;
                        }
                    }
                    plugin @ AuthPluginType::Srp256 => {
                        let srp = SrpClient::<sha2::Sha256>::new(&srp_key, &SRP_GROUP);

                        if let Some(data) = auth_plugin.data {
                            socket = srp_auth(socket, &mut buff, srp, plugin, user, pass, data)?;

                            // Authentication Ok
                            break;
                        } else {
                            // Server requested a different authentication method than the client specified
                            // in the initial connection

                            socket.write_all(&cont_auth(
                                &hex::encode(srp.get_a_pub()).as_bytes(),
                                plugin,
                                AuthPluginType::plugin_list(),
                                &[],
                            ))?;
                            socket.flush()?;

                            let len = socket.read(&mut buff)?;
                            let mut resp = Bytes::copy_from_slice(&buff[..len]);

                            auth_plugin = parse_cont_auth(&mut resp)?;
                        }
                    }
                }
            }
        }

        Ok(Self {
            socket,
            version,
            buff,
            stmt_data_map: Default::default(),
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
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(DbHandle(resp.handle))
    }

    /// Disconnect from the database
    pub fn detach_database(&mut self, db_handle: DbHandle) -> Result<(), FbError> {
        self.socket.write_all(&detach(db_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Drop the database
    pub fn drop_database(&mut self, db_handle: DbHandle) -> Result<(), FbError> {
        self.socket.write_all(&drop_database(db_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Start a new transaction, with the specified transaction parameter buffer
    pub fn begin_transaction(
        &mut self,
        db_handle: DbHandle,
        isolation_level: TrIsolationLevel,
    ) -> Result<TrHandle, FbError> {
        let tpb = [ibase::isc_tpb_version3 as u8, isolation_level as u8];

        self.socket
            .write_all(&transaction(db_handle.0, &tpb))
            .unwrap();
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(TrHandle(resp.handle))
    }

    /// Commit / Rollback a transaction
    pub fn transaction_operation(&mut self, tr_handle: TrHandle, op: TrOp) -> Result<(), FbError> {
        self.socket
            .write_all(&transaction_operation(tr_handle.0, op))?;
        self.socket.flush()?;

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
        self.socket.flush()?;

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
    ) -> Result<(StmtType, StmtHandle), FbError> {
        // Alloc statement
        self.socket.write_all(&allocate_statement(db_handle.0))?;
        // Prepare statement
        self.socket.write_all(&prepare_statement(
            tr_handle.0,
            u32::MAX,
            dialect as u32,
            sql,
        ))?;
        self.socket.flush()?;

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
            self.socket.flush()?;

            let mut data = self.read_response()?.data;

            let parse_resp = parse_xsqlda(&mut data, &mut xsqlda)?;
            truncated = parse_resp.truncated;
            param_count = parse_resp.param_count;
        }

        // Coerce the output columns and transform to blr
        for var in xsqlda.iter_mut() {
            var.coerce()?;
        }
        let blr = xsqlda_to_blr(&xsqlda)?;

        // Store the statement data
        self.stmt_data_map.insert(
            stmt_handle,
            StmtData {
                xsqlda,
                blr,
                param_count,
            },
        );

        Ok((stmt_type, stmt_handle))
    }

    /// Closes or drops a statement
    pub fn free_statement(
        &mut self,
        stmt_handle: StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        self.socket.write_all(&free_statement(stmt_handle.0, op))?;
        // Obs.: Lazy response

        if op == FreeStmtOp::Drop {
            self.stmt_data_map.remove(&stmt_handle);
        }

        Ok(())
    }

    /// Execute the prepared statement with parameters
    pub fn execute(
        &mut self,
        tr_handle: TrHandle,
        stmt_handle: StmtHandle,
        params: &[Param],
    ) -> Result<(), FbError> {
        if let Some(StmtData { param_count, .. }) = self.stmt_data_map.get_mut(&stmt_handle) {
            if params.len() != *param_count {
                return Err(format!(
                    "Tried to execute a statement that has {} parameters while providing {}",
                    param_count,
                    params.len()
                )
                .into());
            }

            let params = blr::params_to_blr(params, self.version)?;

            self.socket
                .write_all(&execute(
                    tr_handle.0,
                    stmt_handle.0,
                    &params.blr,
                    &params.values,
                ))
                .unwrap();
            self.socket.flush()?;

            self.read_response()?;

            Ok(())
        } else {
            Err("Tried to execute a dropped statement".into())
        }
    }

    /// Fetch rows from the executed statement, coercing the types
    /// according to the provided blr
    pub fn fetch(&mut self, stmt_handle: StmtHandle) -> Result<Option<Vec<Column>>, FbError> {
        if let Some(StmtData { blr, xsqlda, .. }) = self.stmt_data_map.get_mut(&stmt_handle) {
            self.socket.write_all(&fetch(stmt_handle.0, &blr))?;
            self.socket.flush()?;

            let (op_code, mut resp) = read_packet(&mut self.socket, &mut self.buff)?;

            if op_code == WireOp::Response as u32 {
                // An error ocurred
                parse_response(&mut resp)?;
            }

            if op_code != WireOp::FetchResponse as u32 {
                return err_conn_rejected(op_code);
            }

            parse_fetch_response(&mut resp, xsqlda, self.version)
        } else {
            Err("Tried to fetch a dropped statement".into())
        }
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

/// Performs the srp authentication with the server, returning the encrypted stream
fn srp_auth<D>(
    mut socket: FbStream,
    buff: &mut [u8],
    srp: SrpClient<D>,
    plugin: AuthPluginType,
    user: &str,
    pass: &str,
    data: SrpAuthData,
) -> Result<FbStream, FbError>
where
    D: digest::Digest,
{
    // Generate a private key with the salt received from the server
    let private_key = srp_private_key::<sha1::Sha1>(user.as_bytes(), pass.as_bytes(), &data.salt);

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
    socket.flush()?;

    read_response(&mut socket, buff)?;

    // Enable wire encryption
    socket.write_all(&crypt("Arc4", "Symmetric"))?;
    socket.flush()?;

    socket = FbStream::Arc4(Arc4Stream::new(
        match socket {
            FbStream::Plain(s) => s,
            _ => unreachable!("Stream was already encrypted!"),
        },
        &verifier.get_key(),
        buff.len(),
    ));

    read_response(&mut socket, buff)?;

    Ok(socket)
}

#[derive(Debug, Clone, Copy)]
/// A database handle
pub struct DbHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A transaction handle
pub struct TrHandle(u32);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
/// A statement handle
pub struct StmtHandle(u32);

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

#[test]
fn connection_test() {
    let db_name = "test.fdb";
    let user = "SYSDBA";
    let pass = "masterkey";

    let mut conn = FirebirdWireConnection::connect("127.0.0.1", 3050, db_name, user, pass).unwrap();

    let db_handle = conn.attach_database(db_name, user, pass).unwrap();

    let tr_handle = conn
        .begin_transaction(db_handle, TrIsolationLevel::Concurrency)
        .unwrap();

    let (stmt_type, stmt_handle) = conn
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

    println!("Statement type: {:?}", stmt_type);

    let params = rsfbclient_core::IntoParams::to_params((1,));

    conn.execute(tr_handle, stmt_handle, &params).unwrap();

    loop {
        let resp = conn.fetch(stmt_handle).unwrap();

        if resp.is_none() {
            break;
        }
        println!("Fetch Resp: {:#?}", resp);
    }

    std::thread::sleep(std::time::Duration::from_millis(100));
}
