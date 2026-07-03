//! `FirebirdConnection` implementation for the pure rust firebird client

use bytes::{BufMut, Bytes, BytesMut};
use std::{
    collections::VecDeque,
    env,
    io::{Read, Write},
    net::TcpStream,
};

use crate::{
    arc4::*,
    blr,
    consts::{AuthPluginType, ProtocolVersion, WireOp},
    srp::*,
    util::*,
    wire::*,
    xsqlda::{parse_xsqlda, xsqlda_to_blr, PrepareInfo, XSqlVar, XSQLDA_DESCRIBE_VARS},
};
use rsfbclient_core::*;

type RustDbHandle = DbHandle;
type RustTrHandle = TrHandle;
type RustStmtHandle = StmtHandle;

/// How many rows to request per op_fetch (round-trip). Configurable via
/// FB_FETCH_BATCH; defaults to 200. The crate originally used 1 (one row per round-trip).
fn fetch_batch_size() -> u32 {
    env::var("FB_FETCH_BATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(200)
}

/// Result of parsing ONE op_fetch_response.
enum FetchOne {
    /// A row (status=0, messages=1).
    Row(Vec<Column>),
    /// End of THIS batch (status=0, messages=0): the server ended the op_fetch
    /// without exhausting the cursor. Re-issuing op_fetch fetches the rest.
    BatchEnd,
    /// End of cursor (status=100). Nothing more to read.
    End,
}

enum FetchErr {
    /// Not enough bytes in the buffer — read more from the socket and retry.
    NeedMore,
    /// A real protocol/server error — propagate it.
    Fatal(FbError),
}

/// Firebird client implemented in pure rust
pub struct RustFbClient {
    conn: Option<FirebirdWireConnection>,
    charset: Charset,
}

/// Required configuration for an attachment with the pure rust client
#[derive(Default, Clone)]
pub struct RustFbClientAttachmentConfig {
    pub host: String,
    pub port: u16,
    pub db_name: String,
    pub user: String,
    pub pass: String,
    pub role_name: Option<String>,
}

/// A Connection to a firebird server
pub struct FirebirdWireConnection {
    /// Connection socket
    socket: FbStream,

    /// Wire protocol version
    pub(crate) version: ProtocolVersion,

    /// Buffer to read the network data
    buff: Box<[u8]>,

    /// Lazy responses to read
    lazy_count: u32,

    pub(crate) charset: Charset,
}

/// Data to keep track about a prepared statement
pub struct StmtHandleData {
    /// Statement handle
    handle: RustStmtHandle,
    /// Output xsqlda
    xsqlda: Vec<XSqlVar>,
    /// Blr representation of the above
    blr: Bytes,
    /// Number of parameters
    param_count: usize,
    /// Rows already fetched in a batch but not yet delivered (batch fetch).
    prefetched: VecDeque<Vec<Column>>,
    /// Cursor exhausted on the server (do not request more batches).
    cursor_eof: bool,
}

impl RustFbClient {
    ///Construct a new instance of the pure rust client
    pub fn new(charset: Charset) -> Self {
        Self {
            conn: None,
            charset,
        }
    }
}

impl FirebirdClientDbOps for RustFbClient {
    type DbHandle = RustDbHandle;
    type AttachmentConfig = RustFbClientAttachmentConfig;

    fn attach_database(
        &mut self,
        config: &Self::AttachmentConfig,
        dialect: Dialect,
        no_db_triggers: bool,
    ) -> Result<RustDbHandle, FbError> {
        let host = config.host.as_str();
        let port = config.port;
        let db_name = config.db_name.as_str();
        let user = config.user.as_str();
        let pass = config.pass.as_str();
        let role = match &config.role_name {
            Some(ro) => Some(ro.as_str()),
            None => None,
        };

        // Take the existing connection, or connects
        let mut conn = match self.conn.take() {
            Some(conn) => conn,
            None => FirebirdWireConnection::connect(
                host,
                port,
                db_name,
                user,
                pass,
                self.charset.clone(),
            )?,
        };

        let attach_result =
            conn.attach_database(db_name, user, pass, role, dialect, no_db_triggers);

        // Put the connection back
        self.conn.replace(conn);

        attach_result
    }

    fn detach_database(&mut self, db_handle: &mut RustDbHandle) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.detach_database(db_handle))
            .unwrap_or_else(err_client_not_connected)
    }

    fn drop_database(&mut self, db_handle: &mut RustDbHandle) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.drop_database(db_handle))
            .unwrap_or_else(err_client_not_connected)
    }

    fn create_database(
        &mut self,
        config: &Self::AttachmentConfig,
        page_size: Option<u32>,
        dialect: Dialect,
    ) -> Result<RustDbHandle, FbError> {
        let host = config.host.as_str();
        let port = config.port;
        let db_name = config.db_name.as_str();
        let user = config.user.as_str();
        let pass = config.pass.as_str();
        let role = match &config.role_name {
            Some(ro) => Some(ro.as_str()),
            None => None,
        };

        // Take the existing connection, or connects
        let mut conn = match self.conn.take() {
            Some(conn) => conn,
            None => FirebirdWireConnection::connect(
                host,
                port,
                db_name,
                user,
                pass,
                self.charset.clone(),
            )?,
        };

        let attach_result = conn.create_database(db_name, user, pass, page_size, role, dialect);

        // Put the connection back
        self.conn.replace(conn);

        attach_result
    }
}

impl FirebirdClientSqlOps for RustFbClient {
    type DbHandle = RustDbHandle;
    type TrHandle = RustTrHandle;
    type StmtHandle = StmtHandleData;

    fn begin_transaction(
        &mut self,
        db_handle: &mut Self::DbHandle,
        confs: TransactionConfiguration,
    ) -> Result<Self::TrHandle, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.begin_transaction(db_handle, confs))
            .unwrap_or_else(err_client_not_connected)
    }

    fn transaction_operation(
        &mut self,
        tr_handle: &mut Self::TrHandle,
        op: TrOp,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.transaction_operation(tr_handle, op))
            .unwrap_or_else(err_client_not_connected)
    }

    fn exec_immediate(
        &mut self,
        _db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
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
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
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
        stmt_handle: &mut Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.free_statement(stmt_handle, op))
            .unwrap_or_else(err_client_not_connected)
    }

    fn execute(
        &mut self,
        _db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<usize, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.execute(tr_handle, stmt_handle, &params))
            .unwrap_or_else(err_client_not_connected)
    }

    fn execute2(
        &mut self,
        _db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<Vec<Column>, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.execute2(tr_handle, stmt_handle, &params))
            .unwrap_or_else(err_client_not_connected)
    }

    fn fetch(
        &mut self,
        _db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
    ) -> Result<Option<Vec<Column>>, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.fetch(tr_handle, stmt_handle))
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
        charset: Charset,
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

        let req = connect(db_name, user, &username, &hostname, &srp_key);
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
                }
            }
        }

        Ok(Self {
            socket,
            version,
            buff,
            lazy_count: 0,
            charset,
        })
    }

    /// Create the database and attach, returning a database handle
    pub fn create_database(
        &mut self,
        db_name: &str,
        user: &str,
        pass: &str,
        page_size: Option<u32>,
        role_name: Option<&str>,
        dialect: Dialect,
    ) -> Result<DbHandle, FbError> {
        self.socket.write_all(&create(
            db_name,
            user,
            pass,
            self.version,
            self.charset.clone(),
            page_size,
            role_name.clone(),
            dialect,
        ))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(DbHandle(resp.handle))
    }

    /// Connect to a database, returning a database handle
    pub fn attach_database(
        &mut self,
        db_name: &str,
        user: &str,
        pass: &str,
        role_name: Option<&str>,
        dialect: Dialect,
        no_db_triggers: bool,
    ) -> Result<DbHandle, FbError> {
        self.socket.write_all(&attach(
            db_name,
            user,
            pass,
            self.version,
            self.charset.clone(),
            role_name.clone(),
            dialect,
            no_db_triggers,
        ))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(DbHandle(resp.handle))
    }

    /// Disconnect from the database
    pub fn detach_database(&mut self, db_handle: &mut DbHandle) -> Result<(), FbError> {
        self.socket.write_all(&detach(db_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Drop the database
    pub fn drop_database(&mut self, db_handle: &mut DbHandle) -> Result<(), FbError> {
        self.socket.write_all(&drop_database(db_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Start a new transaction, with the specified transaction parameter buffer
    pub fn begin_transaction(
        &mut self,
        db_handle: &mut DbHandle,
        confs: TransactionConfiguration,
    ) -> Result<TrHandle, FbError> {
        let mut tpb = vec![
            ibase::isc_tpb_version3 as u8,
            confs.isolation.into(),
            confs.data_access as u8,
            confs.lock_resolution.into(),
        ];
        if let TrLockResolution::Wait(Some(time)) = confs.lock_resolution {
            tpb.push(ibase::isc_tpb_lock_timeout as u8);
            tpb.push(4 as u8);
            tpb.extend_from_slice(&time.to_le_bytes());
        }

        if let TrIsolationLevel::ReadCommited(rec) = confs.isolation {
            tpb.push(rec as u8);
        }

        self.socket.write_all(&transaction(db_handle.0, &tpb))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(TrHandle(resp.handle))
    }

    /// Commit / Rollback a transaction
    pub fn transaction_operation(
        &mut self,
        tr_handle: &mut TrHandle,
        op: TrOp,
    ) -> Result<(), FbError> {
        self.socket
            .write_all(&transaction_operation(tr_handle.0, op))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Execute a sql immediately, without returning rows
    pub fn exec_immediate(
        &mut self,
        tr_handle: &mut TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError> {
        self.socket.write_all(&exec_immediate(
            tr_handle.0,
            dialect as u32,
            sql,
            &self.charset,
        )?)?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Alloc and prepare a statement
    ///
    /// Returns the statement type, handle and xsqlda describing the columns
    pub fn prepare_statement(
        &mut self,
        db_handle: &mut DbHandle,
        tr_handle: &mut TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, StmtHandleData), FbError> {
        // Alloc statement
        self.socket.write_all(&allocate_statement(db_handle.0))?;
        // Prepare statement
        self.socket.write_all(&prepare_statement(
            tr_handle.0,
            u32::MAX,
            dialect as u32,
            sql,
            &self.charset,
        )?)?;
        self.socket.flush()?;

        let (mut op_code, mut resp) = self.read_packet()?;

        // Read lazy responses
        for _ in 0..self.lazy_count {
            if op_code != WireOp::Response as u32 {
                return err_conn_rejected(op_code);
            }
            self.lazy_count -= 1;
            parse_response(&mut resp)?;

            op_code = resp.get_u32()?;
        }

        // Alloc resp
        if op_code != WireOp::Response as u32 {
            return err_conn_rejected(op_code);
        }

        let stmt_handle = StmtHandle(parse_response(&mut resp)?.handle);

        // Prepare resp
        let op_code = resp.get_u32()?;

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
            let next_index = (xsqlda.len() as u16).to_le_bytes();

            self.socket.write_all(&info_sql(
                stmt_handle.0,
                &[
                    &[
                        ibase::isc_info_sql_sqlda_start as u8, // Describe a xsqlda
                        2,
                        next_index[0], // Index, first byte
                        next_index[1], // Index, second byte
                    ],
                    &XSQLDA_DESCRIBE_VARS[..], // Data to be returned
                ]
                .concat(),
            ))?;
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

        Ok((
            stmt_type,
            StmtHandleData {
                handle: stmt_handle,
                xsqlda,
                blr,
                param_count,
                prefetched: VecDeque::new(),
                cursor_eof: false,
            },
        ))
    }

    /// Closes or drops a statement
    pub fn free_statement(
        &mut self,
        stmt_handle: &mut StmtHandleData,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        self.socket
            .write_all(&free_statement(stmt_handle.handle.0, op))?;
        // Obs.: Lazy response

        self.lazy_count += 1;

        Ok(())
    }

    /// Execute the prepared statement with parameters
    pub fn execute(
        &mut self,
        tr_handle: &mut TrHandle,
        stmt_handle: &mut StmtHandleData,
        params: &[SqlType],
    ) -> Result<usize, FbError> {
        if params.len() != stmt_handle.param_count {
            return Err(format!(
                "Tried to execute a statement that has {} parameters while providing {}",
                stmt_handle.param_count,
                params.len()
            )
            .into());
        }

        // Reopen the cursor: drop prefetched rows and the batch-fetch EOF flag
        // from the previous execution. Without this, re-executing the same
        // statement would inherit cursor_eof=true and fetch nothing.
        stmt_handle.prefetched.clear();
        stmt_handle.cursor_eof = false;

        // Execute
        let params = blr::params_to_blr(self, tr_handle, params)?;

        self.socket.write_all(&execute(
            tr_handle.0,
            stmt_handle.handle.0,
            &params.blr,
            &params.values,
        ))?;
        self.socket.flush()?;

        self.read_response()?;

        // Get affected rows
        self.socket.write_all(&info_sql(
            stmt_handle.handle.0,
            &[ibase::isc_info_sql_records as u8], // Request affected rows,
        ))?;
        self.socket.flush()?;

        let mut data = self.read_response()?.data;

        parse_info_sql_affected_rows(&mut data)
    }

    /// Execute the prepared statement with parameters, returning data
    pub fn execute2(
        &mut self,
        tr_handle: &mut TrHandle,
        stmt_handle: &mut StmtHandleData,
        params: &[SqlType],
    ) -> Result<Vec<Column>, FbError> {
        if params.len() != stmt_handle.param_count {
            return Err(format!(
                "Tried to execute a statement that has {} parameters while providing {}",
                stmt_handle.param_count,
                params.len()
            )
            .into());
        }

        // Reopen the cursor (same reason as execute): reset the batch-fetch
        // state from the previous execution.
        stmt_handle.prefetched.clear();
        stmt_handle.cursor_eof = false;

        let params = blr::params_to_blr(self, tr_handle, params)?;

        self.socket.write_all(&execute2(
            tr_handle.0,
            stmt_handle.handle.0,
            &params.blr,
            &params.values,
            &stmt_handle.blr,
        ))?;
        self.socket.flush()?;

        let (mut op_code, mut resp) = read_packet(&mut self.socket, &mut self.buff)?;

        // Read lazy responses
        for _ in 0..self.lazy_count {
            if op_code != WireOp::Response as u32 {
                return err_conn_rejected(op_code);
            }
            self.lazy_count -= 1;
            parse_response(&mut resp)?;

            op_code = resp.get_u32()?;
        }

        if op_code == WireOp::Response as u32 {
            // An error ocurred
            parse_response(&mut resp)?;
        }

        if op_code != WireOp::SqlResponse as u32 {
            return err_conn_rejected(op_code);
        }

        let parsed_cols =
            parse_sql_response(&mut resp, &stmt_handle.xsqlda, self.version, &self.charset)?;

        parse_response(&mut resp)?;

        let mut cols = Vec::with_capacity(parsed_cols.len());

        for pc in parsed_cols {
            cols.push(pc.into_column(self, tr_handle)?);
        }

        Ok(cols)
    }

    /// Fetch ONE row. Served from a buffer filled in batches: when the buffer
    /// empties, a single op_fetch requests `FB_FETCH_BATCH` rows in one
    /// round-trip (it used to be one row per round-trip). Streaming is
    /// preserved — rows come out one at a time, memory bounded to one batch.
    pub fn fetch(
        &mut self,
        tr_handle: &mut TrHandle,
        stmt_handle: &mut StmtHandleData,
    ) -> Result<Option<Vec<Column>>, FbError> {
        let count = fetch_batch_size();
        let mut empty_batches = 0u32;
        while stmt_handle.prefetched.is_empty() && !stmt_handle.cursor_eof {
            self.fetch_batch(tr_handle, stmt_handle, count)?;
            empty_batches += 1;
            // Safety net: a well-behaved server never sends empty batches
            // without exhausting the cursor; guards against a hang if it does.
            if empty_batches > 1000 {
                return Err("fetch: too many empty batches without end of cursor".into());
            }
        }
        Ok(stmt_handle.prefetched.pop_front())
    }

    /// Requests `count` rows in one op_fetch and reads every op_fetch_response
    /// that arrives, filling `stmt_handle.prefetched`. Parses greedily; if bytes
    /// are missing mid-response, reads more from the socket and resumes (robust
    /// against responses split across TCP segments — the cause of the old
    /// "Invalid server response, missing bytes").
    fn fetch_batch(
        &mut self,
        tr_handle: &mut TrHandle,
        stmt_handle: &mut StmtHandleData,
        count: u32,
    ) -> Result<(), FbError> {
        self.socket
            .write_all(&fetch(stmt_handle.handle.0, &stmt_handle.blr, count))?;
        self.socket.flush()?;

        let mut acc = BytesMut::new();
        let mut got = 0u32;

        loop {
            // Parse as many responses as the accumulated bytes allow.
            let mut view = std::mem::take(&mut acc).freeze();
            loop {
                let snapshot = view.clone(); // O(1): Bytes shares the underlying buffer
                let saved_lazy = self.lazy_count;
                match self.parse_one_fetch_response(&mut view, &stmt_handle.xsqlda, tr_handle) {
                    Ok(FetchOne::Row(cols)) => {
                        stmt_handle.prefetched.push_back(cols);
                        got += 1;
                        // Do NOT return on got>=count: after the rows, the server
                        // always sends a terminating op_fetch_response (messages=0
                        // = end of this batch, or status=100 = end of cursor).
                        // Returning early would discard that terminator (still
                        // buffered) and desync the next op_fetch. Let BatchEnd/End
                        // end the loop. Guard against a server sending more rows
                        // than requested (should not happen).
                        if got > count {
                            return Err("server sent more rows than requested in op_fetch".into());
                        }
                    }
                    Ok(FetchOne::BatchEnd) => {
                        // Server ended this op_fetch without exhausting the cursor.
                        // Deliver what arrived; the next fetch() re-issues op_fetch.
                        return Ok(());
                    }
                    Ok(FetchOne::End) => {
                        stmt_handle.cursor_eof = true;
                        return Ok(());
                    }
                    Err(FetchErr::NeedMore) => {
                        self.lazy_count = saved_lazy; // undo partial lazy consumption
                        view = snapshot;
                        break;
                    }
                    Err(FetchErr::Fatal(e)) => return Err(e),
                }
            }

            // Missing bytes: keep the unconsumed tail and read more from the socket.
            let mut next = BytesMut::from(view.as_ref());
            let n = self.socket.read(&mut self.buff)?;
            if n == 0 {
                return Err("Fetch: connection closed mid-batch".into());
            }
            next.extend_from_slice(&self.buff[..n]);
            acc = next;
        }
    }

    /// Tries to parse ONE op_fetch_response from `view`. On NeedMore, the caller
    /// restores `view` from a snapshot (the partial consumption here is discarded).
    fn parse_one_fetch_response(
        &mut self,
        view: &mut Bytes,
        xsqlda: &[XSqlVar],
        tr_handle: &mut TrHandle,
    ) -> Result<FetchOne, FetchErr> {
        // op_code, skipping Dummy packets
        let mut op_code = loop {
            if view.remaining() < 4 {
                return Err(FetchErr::NeedMore);
            }
            let oc = view.get_u32().map_err(|_| FetchErr::NeedMore)?;
            if oc != WireOp::Dummy as u32 {
                break oc;
            }
        };

        // Pending lazy responses
        for _ in 0..self.lazy_count {
            if op_code != WireOp::Response as u32 {
                return Err(FetchErr::Fatal(
                    format!("unexpected op_code in fetch (op {})", op_code).into(),
                ));
            }
            self.lazy_count -= 1;
            parse_response(view).map_err(|_| FetchErr::NeedMore)?;
            if view.remaining() < 4 {
                return Err(FetchErr::NeedMore);
            }
            op_code = view.get_u32().map_err(|_| FetchErr::NeedMore)?;
        }

        if op_code == WireOp::Response as u32 {
            // Error reported by the server
            parse_response(view).map_err(FetchErr::Fatal)?;
        }

        if op_code != WireOp::FetchResponse as u32 {
            return Err(FetchErr::Fatal(
                format!("unexpected op_code in fetch (op {})", op_code).into(),
            ));
        }

        // Body: [status: u32][messages: u32][null_map][columns...]. Peek status
        // and messages without consuming, to tell end-of-cursor (status=100),
        // end-of-batch (messages=0) and a row (messages=1) apart BEFORE delegating.
        if view.remaining() < 4 {
            return Err(FetchErr::NeedMore);
        }
        let status = {
            let mut peek = view.clone();
            peek.get_u32().map_err(|_| FetchErr::NeedMore)?
        };
        if status == 100 {
            // End of cursor: consume only the status (same as parse_fetch_response).
            view.advance(4).map_err(|_| FetchErr::NeedMore)?;
            return Ok(FetchOne::End);
        }
        if view.remaining() < 8 {
            return Err(FetchErr::NeedMore);
        }
        let messages = {
            let mut peek = view.clone();
            peek.get_u32().map_err(|_| FetchErr::NeedMore)?; // status
            peek.get_u32().map_err(|_| FetchErr::NeedMore)? // messages
        };
        if messages == 0 {
            // End of this batch with no row: consume status+messages, stop the batch.
            view.advance(8).map_err(|_| FetchErr::NeedMore)?;
            return Ok(FetchOne::BatchEnd);
        }

        // A row is present. Delegate to the crate parser (re-reads status+messages+data).
        // charset cloned so we don't hold a borrow of self when calling into_column.
        let version = self.version;
        let charset = self.charset.clone();
        match parse_fetch_response(view, xsqlda, version, &charset) {
            Ok(None) => Ok(FetchOne::End),
            Ok(Some(parsed)) => {
                let mut cols = Vec::with_capacity(parsed.len());
                for pc in parsed {
                    cols.push(pc.into_column(self, tr_handle).map_err(FetchErr::Fatal)?);
                }
                Ok(FetchOne::Row(cols))
            }
            // Underflow (bytes missing mid-response) has a fixed message -> read
            // more and resume. Any OTHER error (e.g. invalid UTF-8 when decoding a
            // column) is a real data error: propagate it, don't turn it into an
            // infinite wait for bytes that never arrive.
            Err(e) => {
                if matches!(&e, FbError::Other(m) if m == "Invalid server response, missing bytes")
                {
                    Err(FetchErr::NeedMore)
                } else {
                    Err(FetchErr::Fatal(e))
                }
            }
        }
    }

    /// Create a new blob, returning the blob handle and id
    pub fn create_blob(
        &mut self,
        tr_handle: &mut TrHandle,
    ) -> Result<(BlobHandle, BlobId), FbError> {
        self.socket.write_all(&create_blob(tr_handle.0))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok((BlobHandle(resp.handle), BlobId(resp.object_id)))
    }

    /// Put blob segments
    pub fn put_segments(&mut self, blob_handle: BlobHandle, data: &[u8]) -> Result<(), FbError> {
        for segment in data.chunks(crate::blr::MAX_DATA_LENGTH) {
            self.socket
                .write_all(&put_segment(blob_handle.0, segment))?;
            self.socket.flush()?;

            self.read_response()?;
        }

        Ok(())
    }

    /// Open a blob, returning the blob handle
    pub fn open_blob(
        &mut self,
        tr_handle: &mut TrHandle,
        blob_id: BlobId,
    ) -> Result<BlobHandle, FbError> {
        self.socket.write_all(&open_blob(tr_handle.0, blob_id.0))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(BlobHandle(resp.handle))
    }

    /// Get a blob segment, returns the bytes and true if there is more data
    pub fn get_segment(&mut self, blob_handle: BlobHandle) -> Result<(Bytes, bool), FbError> {
        self.socket.write_all(&get_segment(blob_handle.0))?;
        self.socket.flush()?;

        let mut blob_data = BytesMut::with_capacity(256);

        let resp = self.read_response()?;
        let mut data = resp.data;

        loop {
            if data.remaining() < 2 {
                break;
            }
            let len = data.get_u16_le()? as usize;
            if data.remaining() < len {
                return err_invalid_response();
            }
            blob_data.put_slice(&data[..len]);
            data.advance(len)?;
        }

        Ok((blob_data.freeze(), resp.handle == 2))
    }

    /// Closes a blob handle
    pub fn close_blob(&mut self, blob_handle: BlobHandle) -> Result<(), FbError> {
        self.socket.write_all(&close_blob(blob_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Read a server response
    fn read_response(&mut self) -> Result<Response, FbError> {
        read_response(&mut self.socket, &mut self.buff, &mut self.lazy_count)
    }

    /// Reads a packet from the socket
    fn read_packet(&mut self) -> Result<(u32, Bytes), FbError> {
        read_packet(&mut self.socket, &mut self.buff)
    }
}

/// Read a server response
fn read_response(
    socket: &mut impl Read,
    buff: &mut [u8],
    lazy_count: &mut u32,
) -> Result<Response, FbError> {
    let (mut op_code, mut resp) = read_packet(socket, buff)?;

    // Read lazy responses
    for _ in 0..*lazy_count {
        if op_code != WireOp::Response as u32 {
            return err_conn_rejected(op_code);
        }
        *lazy_count -= 1;
        parse_response(&mut resp)?;

        op_code = resp.get_u32()?;
    }

    if op_code != WireOp::Response as u32 {
        return err_conn_rejected(op_code);
    }

    parse_response(&mut resp)
}

/// Reads a packet from the socket
fn read_packet(socket: &mut impl Read, buff: &mut [u8]) -> Result<(u32, Bytes), FbError> {
    let mut len = socket.read(buff)?;
    let mut resp = BytesMut::from(&buff[..len]);

    loop {
        if len == buff.len() {
            // The buffer was not large enough, so read more
            len = socket.read(buff)?;
            resp.put_slice(&buff[..len]);
        } else {
            break;
        }
    }
    let mut resp = resp.freeze();

    let op_code = loop {
        let op_code = resp.get_u32()?;

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
        proof.as_bytes(),
        plugin,
        AuthPluginType::plugin_list(),
        &[],
    ))?;
    socket.flush()?;

    read_response(&mut socket, buff, &mut 0)?;

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

    read_response(&mut socket, buff, &mut 0)?;

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

#[derive(Debug, Clone, Copy)]
/// A blob handle
pub struct BlobHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A blob Identificator
pub struct BlobId(pub(crate) u64);

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
#[ignore]
fn connection_test() {
    use rsfbclient_core::charset::UTF_8;

    let db_name = "test.fdb";
    let user = "SYSDBA";
    let pass = "masterkey";

    let mut conn =
        FirebirdWireConnection::connect("127.0.0.1", 3050, db_name, user, pass, UTF_8).unwrap();

    let mut db_handle = conn
        .attach_database(db_name, user, pass, None, Dialect::D3, false)
        .unwrap();

    let mut tr_handle = conn
        .begin_transaction(&mut db_handle, TransactionConfiguration::default())
        .unwrap();

    let (stmt_type, mut stmt_handle) = conn
        .prepare_statement(
            &mut db_handle,
            &mut tr_handle,
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

    let params = match rsfbclient_core::IntoParams::to_params((1,)) {
        rsfbclient_core::ParamsType::Positional(params) => params,
        _ => unreachable!(),
    };

    conn.execute(&mut tr_handle, &mut stmt_handle, &params)
        .unwrap();

    loop {
        let resp = conn.fetch(&mut tr_handle, &mut stmt_handle).unwrap();

        if resp.is_none() {
            break;
        }
        println!("Fetch Resp: {:#?}", resp);
    }

    std::thread::sleep(std::time::Duration::from_millis(100));
}
