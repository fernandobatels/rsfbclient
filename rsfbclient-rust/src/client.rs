//! `FirebirdConnection` implementation for the pure rust firebird client

use bytes::{BufMut, Bytes, BytesMut};
use std::{
    collections::VecDeque,
    env,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
};

use crate::{
    arc4::*,
    blr,
    consts::{AuthPluginType, ProtocolVersion, WireOp},
    events::*,
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

/// Size of the scratch buffer for a single socket read. Batch fetch pulls many
/// rows per op_fetch, so a bigger read cuts read syscalls and, more importantly,
/// keeps rows from being split mid-response and re-parsed on the
/// incomplete-response retry path of `read_with` (wasted CPU that scales with
/// the column count).
const READ_BUFFER_LEN: usize = 64 * 1024;

/// Result of parsing ONE op_fetch_response. `T` is the decoded row: statements
/// with blob columns use `Vec<ParsedColumn>`, since fetching a blob costs extra
/// round-trips that must not be interleaved with the responses of the running
/// batch (see `fetch_batch`); blob-free statements decode straight into
/// `Vec<Column>`.
enum FetchOne<T> {
    /// A row (status=0, messages=1).
    Row(T),
    /// End of THIS batch (status=0, messages=0): the server ended the op_fetch
    /// without exhausting the cursor. Re-issuing op_fetch fetches the rest.
    BatchEnd,
    /// End of cursor (status=100). Nothing more to read.
    End,
}

/// What one op_fetch_response is, decided from its framing alone (op code,
/// status and message count) before any column is decoded.
enum Framed {
    /// A row is present; `resp` is left at the start of the response body.
    Row,
    /// End of this batch: the response was consumed, no row.
    BatchEnd,
    /// End of cursor: the response was consumed, nothing more to read.
    End,
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

    /// Scratch buffer for a single socket read
    buff: Box<[u8]>,

    /// Bytes received from the socket but not consumed yet.
    ///
    /// The wire protocol is a byte stream with no packet framing, so one read()
    /// can return half a response or several responses at once. Whatever is left
    /// over after parsing has to survive until the next read — dropping it (as
    /// the old per-call buffer did) desynchronises the stream and every later
    /// operation reads garbage as its op code.
    pending: Bytes,

    /// Lazy responses to read
    lazy_count: u32,

    pub(crate) charset: Charset,

    /// AuthPlugin data for use on attach when WireCrypt = Disabled
    pub(crate) auth_plugin: Option<AuthPlugin>,
    /// Key for the srp auth
    pub(crate) srp_key: [u8; 32],

    /// Auxiliary connection the server pushes the event notifications on.
    /// Opened on the first wait, and reused afterwards: firebird keeps a single
    /// auxiliary port per attachment
    event_channel: Option<EventChannel>,

    /// Id to use for the next event registration
    next_event_id: u32,
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
    /// Any output column is a blob. Blobs need a deferred round-trip to fetch, so
    /// they go through the two-phase `ParsedColumn` path. When false (the common
    /// case) fetch decodes rows straight into `Vec<Column>`, skipping the second
    /// buffer and the per-column move loop.
    has_blob: bool,
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

impl FirebirdClientDbEvents for RustFbClient {
    fn wait_for_event(
        &mut self,
        db_handle: &mut Self::DbHandle,
        name: String,
    ) -> Result<(), FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.wait_for_event(db_handle, &name))
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
        // The wire protocol is request/response with small writes, so Nagle has
        // nothing to coalesce and every statement waits out a delayed ACK
        // (~40ms). Measured against Firebird 2.5: SELECT 1 FROM RDB$DATABASE
        // 44ms -> 0.2ms, a 1000-row select 107ms -> 5.7ms.
        let _ = socket.set_nodelay(true);

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

        let mut buff = vec![0; READ_BUFFER_LEN].into_boxed_slice();
        let mut pending = Bytes::new();

        let ConnectionResponse {
            version,
            mut auth_plugin,
            continue_auth,
        } = read_with(&mut socket, &mut buff, &mut pending, &mut 0, |resp, _| {
            parse_accept(resp)
        })?;

        if let Some(auth_plugin) = &mut auth_plugin {
            loop {
                match auth_plugin.kind {
                    plugin @ AuthPluginType::Srp => {
                        let srp = SrpClient::<sha1::Sha1>::new(&srp_key, &SRP_GROUP);

                        if let Some(data) = auth_plugin.data.clone() {
                            if continue_auth {
                                // Continue autentication if needed
                                socket = srp_auth(
                                    socket,
                                    &mut buff,
                                    &mut pending,
                                    srp,
                                    plugin,
                                    user,
                                    pass,
                                    &data,
                                )?;
                            }

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

                            *auth_plugin = read_with(
                                &mut socket,
                                &mut buff,
                                &mut pending,
                                &mut 0,
                                |resp, _| parse_cont_auth(resp),
                            )?;
                        }
                    }
                    plugin @ AuthPluginType::Srp256 => {
                        let srp = SrpClient::<sha2::Sha256>::new(&srp_key, &SRP_GROUP);

                        if let Some(data) = auth_plugin.data.clone() {
                            if continue_auth {
                                // Continue autentication if needed
                                socket = srp_auth(
                                    socket,
                                    &mut buff,
                                    &mut pending,
                                    srp,
                                    plugin,
                                    user,
                                    pass,
                                    &data,
                                )?;
                            }

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

                            *auth_plugin = read_with(
                                &mut socket,
                                &mut buff,
                                &mut pending,
                                &mut 0,
                                |resp, _| parse_cont_auth(resp),
                            )?;
                        }
                    }
                }
            }
        }

        Ok(Self {
            socket,
            version,
            buff,
            pending,
            lazy_count: 0,
            charset,
            auth_plugin: if continue_auth {
                // Already authenticated
                None
            } else {
                // Needs to authenticate in attach
                auth_plugin
            },
            srp_key,
            event_channel: None,
            next_event_id: 1,
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
            role_name,
            dialect,
            self.auth_plugin.as_ref(),
            &self.srp_key,
        )?)?;
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
            role_name,
            dialect,
            no_db_triggers,
            self.auth_plugin.as_ref(),
            &self.srp_key,
        )?)?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(DbHandle(resp.handle))
    }

    /// Disconnect from the database
    pub fn detach_database(&mut self, db_handle: &mut DbHandle) -> Result<(), FbError> {
        // The server drops the auxiliary port along with the attachment
        self.close_event_channel();

        self.socket.write_all(&detach(db_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Drop the database
    pub fn drop_database(&mut self, db_handle: &mut DbHandle) -> Result<(), FbError> {
        // The server drops the auxiliary port along with the attachment
        self.close_event_channel();

        self.socket.write_all(&drop_database(db_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Wait until `name` is posted on the database.
    ///
    /// Blocks the connection: the notification arrives on the auxiliary
    /// channel, but registering the interest and acknowledging it both happen
    /// on this connection.
    pub fn wait_for_event(&mut self, db_handle: &mut DbHandle, name: &str) -> Result<(), FbError> {
        let name = normalize_event_name(name)?;

        self.open_event_channel(db_handle)?;

        // Firebird considers an interest satisfied as soon as the counter it
        // holds for the event has reached the counter of the registration, so
        // registering with a counter of zero always fires straight away. That
        // first notification carries the current counter and is a
        // synchronization, not an event. The native client has to do the very
        // same thing, hence its two `isc_wait_for_event` calls.
        let event_id = self.new_event_id();
        let counters = self.que_events(db_handle, name, 0, event_id)?;
        let posted = event_count(&counters, name)?;

        // Now that the current counter is known, register again: this time the
        // server only answers once the event is really posted
        let event_id = self.new_event_id();
        self.que_events(db_handle, name, posted, event_id)?;

        Ok(())
    }

    /// Allocate the id of a new event registration.
    ///
    /// Every registration gets its own id, so the notification of a previous
    /// one can never be taken for the one being waited on. Zero is skipped:
    /// firebird uses it to mark a registration as already handled.
    fn new_event_id(&mut self) -> u32 {
        let event_id = self.next_event_id;

        self.next_event_id = self.next_event_id.wrapping_add(1).max(1);

        event_id
    }

    /// Register an interest in `name` and block until the server notifies it.
    ///
    /// Returns the occurrence counters of the notification.
    fn que_events(
        &mut self,
        db_handle: &mut DbHandle,
        name: &str,
        count: u32,
        event_id: u32,
    ) -> Result<Vec<(String, u32)>, FbError> {
        // Encoded with the charset of the connection, like every other string
        // this connection sends
        let epb = event_block(&self.charset, [(name, count)])?;

        self.socket
            .write_all(&que_events(db_handle.0, &epb, event_id))?;
        self.socket.flush()?;

        // The registration is acknowledged here, the notification itself comes
        // later on the auxiliary channel
        self.read_response()?;

        let channel = match self.event_channel.as_mut() {
            Some(channel) => channel,
            None => return Err(FbError::from("The event channel was closed")),
        };

        match channel.recv_event(event_id) {
            Ok(counters) => Ok(counters),

            Err(err) => {
                // The notification will never arrive, so release the
                // registration the server is still holding. Best effort: the
                // whole connection may be gone.
                self.event_channel = None;
                self.cancel_events(db_handle, event_id).ok();

                Err(err)
            }
        }
    }

    /// Cancel a pending event registration
    fn cancel_events(&mut self, db_handle: &mut DbHandle, event_id: u32) -> Result<(), FbError> {
        self.socket
            .write_all(&cancel_events(db_handle.0, event_id))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Open the auxiliary connection the event notifications are pushed on, if
    /// it is not already open for this database handle
    fn open_event_channel(&mut self, db_handle: &mut DbHandle) -> Result<(), FbError> {
        if matches!(&self.event_channel, Some(channel) if channel.db_handle() == db_handle.0) {
            return Ok(());
        }
        self.event_channel = None;

        self.socket.write_all(&connect_request(db_handle.0))?;
        self.socket.flush()?;

        let resp = self.read_response()?;
        let port = parse_aux_port(&resp.data)?;

        // The server is listening by the time it answered, so connect now: it
        // gives up on the auxiliary port after `ConnectionTimeout` seconds
        let peer = self.socket.peer_addr()?;
        self.event_channel = Some(EventChannel::open(
            db_handle.0,
            self.charset.clone(),
            peer,
            port,
        )?);

        Ok(())
    }

    /// Close the auxiliary event connection, if any
    fn close_event_channel(&mut self) {
        self.event_channel = None;
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

        // Both responses (alloc + prepare) come back in one go. The prepare one
        // carries the xsqlda and is easily larger than a single read for a wide
        // table, so it has to be read until it is complete.
        let (stmt_handle, mut prepare_data) = read_with(
            &mut self.socket,
            &mut self.buff,
            &mut self.pending,
            &mut self.lazy_count,
            |resp, lazy_count| {
                // Alloc resp
                let op_code = skip_lazy_responses(resp, lazy_count)?;
                if op_code != WireOp::Response as u32 {
                    return err_conn_rejected(op_code);
                }
                let stmt_handle = StmtHandle(parse_response(resp)?.handle);

                // Prepare resp
                let op_code = next_op_code(resp)?;
                if op_code != WireOp::Response as u32 {
                    return err_conn_rejected(op_code);
                }

                Ok((stmt_handle, parse_response(resp)?.data))
            },
        )?;

        // Parsed outside the retry above: `data` is length-delimited inside the
        // response, so it is complete by construction here, and parse_xsqlda
        // appends to `xsqlda` — retrying it would duplicate columns.
        let mut xsqlda = Vec::new();

        let PrepareInfo {
            stmt_type,
            mut param_count,
            mut truncated,
        } = parse_xsqlda(&mut prepare_data, &mut xsqlda)?;

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
        let has_blob = xsqlda
            .iter()
            .any(|var| var.sqltype as u32 & !1 == ibase::SQL_BLOB);

        Ok((
            stmt_type,
            StmtHandleData {
                handle: stmt_handle,
                xsqlda,
                blr,
                param_count,
                prefetched: VecDeque::new(),
                cursor_eof: false,
                has_blob,
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

        let version = self.version;
        let charset = self.charset.clone();
        let xsqlda = &stmt_handle.xsqlda;

        let parsed_cols = read_with(
            &mut self.socket,
            &mut self.buff,
            &mut self.pending,
            &mut self.lazy_count,
            |resp, lazy_count| {
                let op_code = skip_lazy_responses(resp, lazy_count)?;

                if op_code == WireOp::Response as u32 {
                    // An error ocurred
                    parse_response(resp)?;
                }

                if op_code != WireOp::SqlResponse as u32 {
                    return err_conn_rejected(op_code);
                }

                let parsed_cols = parse_sql_response(resp, xsqlda, version, &charset)?;

                // The sql response is followed by its own op_response, framed with
                // its own op_code just like every other response on the wire.
                let op_code = next_op_code(resp)?;
                if op_code != WireOp::Response as u32 {
                    return err_conn_rejected(op_code);
                }
                parse_response(resp)?;

                Ok(parsed_cols)
            },
        )?;

        // Only now, with the response above fully consumed, is it safe to run the
        // extra round-trips a blob column needs: issuing them earlier would read
        // the blob replies from behind the still-unparsed bytes of this response.
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
    /// that arrives, filling `stmt_handle.prefetched`.
    fn fetch_batch(
        &mut self,
        tr_handle: &mut TrHandle,
        stmt_handle: &mut StmtHandleData,
        count: u32,
    ) -> Result<(), FbError> {
        self.socket
            .write_all(&fetch(stmt_handle.handle.0, &stmt_handle.blr, count))?;
        self.socket.flush()?;

        if stmt_handle.has_blob {
            self.read_batch_deferring_blobs(tr_handle, stmt_handle, count)
        } else {
            self.read_batch_columns(stmt_handle, count)
        }
    }

    /// Reads a batch of a blob-free statement, decoding each row straight into
    /// `Vec<Column>`: no `ParsedColumn` buffer, no per-column move loop and no
    /// per-row charset clone (`into_column` is not used, so the charset is only
    /// borrowed).
    fn read_batch_columns(
        &mut self,
        stmt_handle: &mut StmtHandleData,
        count: u32,
    ) -> Result<(), FbError> {
        let version = self.version;
        let charset = self.charset.clone();
        let xsqlda = &stmt_handle.xsqlda;

        let mut rows = Vec::new();
        let mut cursor_eof = false;
        let mut got = 0u32;

        loop {
            let one = read_with(
                &mut self.socket,
                &mut self.buff,
                &mut self.pending,
                &mut self.lazy_count,
                |resp, lazy_count| {
                    parse_one_fetch_response_columns(resp, lazy_count, xsqlda, version, &charset)
                },
            )?;

            match one {
                FetchOne::Row(cols) => {
                    rows.push(cols);
                    got += 1;
                    // Same as in `read_batch_deferring_blobs`: the terminating
                    // op_fetch_response has to be consumed, so don't stop on
                    // got >= count.
                    if got > count {
                        return Err("server sent more rows than requested in op_fetch".into());
                    }
                }
                FetchOne::BatchEnd => break,
                FetchOne::End => {
                    cursor_eof = true;
                    break;
                }
            }
        }

        stmt_handle.cursor_eof = cursor_eof;
        stmt_handle.prefetched.extend(rows);

        Ok(())
    }

    /// Reads a batch of a statement with blob columns: the rows are parsed first
    /// and the blobs resolved afterwards, once the batch is fully consumed.
    fn read_batch_deferring_blobs(
        &mut self,
        tr_handle: &mut TrHandle,
        stmt_handle: &mut StmtHandleData,
        count: u32,
    ) -> Result<(), FbError> {
        let version = self.version;
        let charset = self.charset.clone();
        let xsqlda = &stmt_handle.xsqlda;

        // Read the whole batch before touching any blob. Resolving a blob costs
        // its own round-trips, and starting one while later rows of this batch
        // are still unparsed would make the blob replies queue up behind them.
        let mut rows: Vec<Vec<ParsedColumn>> = Vec::new();
        let mut cursor_eof = false;
        let mut got = 0u32;

        loop {
            let one = read_with(
                &mut self.socket,
                &mut self.buff,
                &mut self.pending,
                &mut self.lazy_count,
                |resp, lazy_count| {
                    parse_one_fetch_response(resp, lazy_count, xsqlda, version, &charset)
                },
            )?;

            match one {
                FetchOne::Row(cols) => {
                    rows.push(cols);
                    got += 1;
                    // Do NOT stop on got>=count: after the rows, the server always
                    // sends a terminating op_fetch_response (messages=0 = end of
                    // this batch, or status=100 = end of cursor). Let BatchEnd/End
                    // end the loop, so the terminator is consumed. Guard against a
                    // server sending more rows than requested (should not happen).
                    if got > count {
                        return Err("server sent more rows than requested in op_fetch".into());
                    }
                }
                // Server ended this op_fetch without exhausting the cursor.
                // Deliver what arrived; the next fetch() re-issues op_fetch.
                FetchOne::BatchEnd => break,
                FetchOne::End => {
                    cursor_eof = true;
                    break;
                }
            }
        }

        stmt_handle.cursor_eof = cursor_eof;

        for parsed in rows {
            let mut cols = Vec::with_capacity(parsed.len());
            for pc in parsed {
                cols.push(pc.into_column(self, tr_handle)?);
            }
            stmt_handle.prefetched.push_back(cols);
        }

        Ok(())
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
        read_response(
            &mut self.socket,
            &mut self.buff,
            &mut self.pending,
            &mut self.lazy_count,
        )
    }
}

/// Reads from `socket` until `parse` succeeds against the accumulated bytes,
/// then keeps the unconsumed tail in `pending` for the next call.
///
/// This is the only correct way to read a Firebird response: the protocol has no
/// packet framing, so a response is complete exactly when it parses. A single
/// read() may return a partial response — the parse then underflows and we read
/// more instead of handing a truncated buffer to the caller — or several
/// responses at once, in which case the surplus stays in `pending` rather than
/// being dropped and leaving the stream misaligned.
///
/// `parse` may consume from `lazy_count`; a failed attempt is rolled back so the
/// retry starts from the same state.
fn read_with<T>(
    socket: &mut impl Read,
    buff: &mut [u8],
    pending: &mut Bytes,
    lazy_count: &mut u32,
    mut parse: impl FnMut(&mut Bytes, &mut u32) -> Result<T, FbError>,
) -> Result<T, FbError> {
    loop {
        // O(1): Bytes shares the underlying allocation
        let mut view = pending.clone();
        let saved_lazy = *lazy_count;

        match parse(&mut view, lazy_count) {
            Ok(parsed) => {
                // Commit: `view` is what the parse did not consume.
                *pending = view;
                return Ok(parsed);
            }

            Err(e) if is_incomplete(&e) => {
                // Undo the partial consumption and wait for the rest.
                *lazy_count = saved_lazy;

                let len = socket.read(buff)?;
                if len == 0 {
                    return Err("Connection closed by the server".into());
                }

                let mut next = BytesMut::with_capacity(pending.len() + len);
                next.put_slice(pending);
                next.put_slice(&buff[..len]);
                *pending = next.freeze();
            }

            // A server-reported error is a fully parsed response, so commit it:
            // leaving its bytes in `pending` would make the next operation parse
            // them again.
            Err(e) => {
                *pending = view;
                return Err(e);
            }
        }
    }
}

/// Reads the next op code, skipping `op_dummy` keepalives.
fn next_op_code(resp: &mut Bytes) -> Result<u32, FbError> {
    loop {
        let op_code = resp.get_u32()?;

        if op_code != WireOp::Dummy as u32 {
            return Ok(op_code);
        }
    }
}

/// Consumes the lazy responses pending before the response we actually want.
fn skip_lazy_responses(resp: &mut Bytes, lazy_count: &mut u32) -> Result<u32, FbError> {
    let mut op_code = next_op_code(resp)?;

    while *lazy_count > 0 {
        if op_code != WireOp::Response as u32 {
            return err_conn_rejected(op_code);
        }
        *lazy_count -= 1;
        parse_response(resp)?;

        op_code = next_op_code(resp)?;
    }

    Ok(op_code)
}

/// Parses ONE op_fetch_response. Blob columns are returned unresolved — see
/// `fetch_batch`.
fn parse_one_fetch_response(
    resp: &mut Bytes,
    lazy_count: &mut u32,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
) -> Result<FetchOne<Vec<ParsedColumn>>, FbError> {
    match frame_fetch_response(resp, lazy_count)? {
        Framed::End => return Ok(FetchOne::End),
        Framed::BatchEnd => return Ok(FetchOne::BatchEnd),
        Framed::Row => {}
    }

    // Delegate to the crate parser (re-reads status+messages+data).
    match parse_fetch_response(resp, xsqlda, version, charset)? {
        Some(parsed) => Ok(FetchOne::Row(parsed)),
        None => Ok(FetchOne::End),
    }
}

/// Same as [`parse_one_fetch_response`], for a statement with no blob columns:
/// the row is decoded straight into `Vec<Column>`.
fn parse_one_fetch_response_columns(
    resp: &mut Bytes,
    lazy_count: &mut u32,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
) -> Result<FetchOne<Vec<Column>>, FbError> {
    match frame_fetch_response(resp, lazy_count)? {
        Framed::End => return Ok(FetchOne::End),
        Framed::BatchEnd => return Ok(FetchOne::BatchEnd),
        Framed::Row => {}
    }

    match parse_fetch_response_columns(resp, xsqlda, version, charset)? {
        Some(cols) => Ok(FetchOne::Row(cols)),
        None => Ok(FetchOne::End),
    }
}

/// Frames ONE op_fetch_response: consumes the response when it carries no row,
/// and leaves `resp` at the start of the body when it does, so the caller can
/// decode the row the way it needs.
fn frame_fetch_response(resp: &mut Bytes, lazy_count: &mut u32) -> Result<Framed, FbError> {
    let op_code = skip_lazy_responses(resp, lazy_count)?;

    if op_code == WireOp::Response as u32 {
        // Error reported by the server
        parse_response(resp)?;
    }

    if op_code != WireOp::FetchResponse as u32 {
        return Err(format!("unexpected op_code in fetch (op {})", op_code).into());
    }

    // Body: [status: u32][messages: u32][null_map][columns...]. Peek both
    // without consuming, to tell end-of-cursor (status=100), end-of-batch
    // (messages=0) and a row (messages=1) apart before delegating.
    if resp.remaining() < 8 {
        return err_invalid_response();
    }
    let (status, messages) = {
        let mut peek = resp.clone();
        (peek.get_u32()?, peek.get_u32()?)
    };

    if status == 100 {
        // End of cursor. Consume status AND messages: the server always sends
        // both, but parse_fetch_response stops after the status, which used to
        // leave four bytes in the stream for the next operation to read as its
        // op code (op 0 -> "Connection rejected with code 0").
        resp.advance(8)?;
        return Ok(Framed::End);
    }

    if messages == 0 {
        // End of this batch with no row.
        resp.advance(8)?;
        return Ok(Framed::BatchEnd);
    }

    Ok(Framed::Row)
}

/// Read a server response
fn read_response(
    socket: &mut impl Read,
    buff: &mut [u8],
    pending: &mut Bytes,
    lazy_count: &mut u32,
) -> Result<Response, FbError> {
    read_with(socket, buff, pending, lazy_count, |resp, lazy_count| {
        let op_code = skip_lazy_responses(resp, lazy_count)?;

        if op_code != WireOp::Response as u32 {
            return err_conn_rejected(op_code);
        }

        parse_response(resp)
    })
}

pub(crate) fn srp_verifier<D>(
    srp: SrpClient<D>,
    user: &str,
    pass: &str,
    data: &SrpAuthData,
) -> Result<SrpClientVerifier<D>, FbError>
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
    Ok(verifier)
}

/// Performs the srp authentication with the server, returning the encrypted stream
fn srp_auth<D>(
    mut socket: FbStream,
    buff: &mut [u8],
    pending: &mut Bytes,
    srp: SrpClient<D>,
    plugin: AuthPluginType,
    user: &str,
    pass: &str,
    data: &SrpAuthData,
) -> Result<FbStream, FbError>
where
    D: digest::Digest,
{
    let verifier = srp_verifier(srp, user, pass, data)?;

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

    read_response(&mut socket, buff, pending, &mut 0)?;

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

    read_response(&mut socket, buff, pending, &mut 0)?;

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

impl FbStream {
    /// Address of the server this stream is connected to
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        match self {
            FbStream::Plain(s) => s.peer_addr(),
            FbStream::Arc4(s) => s.peer_addr(),
        }
    }
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

#[cfg(test)]
mod read_tests {
    use super::*;
    use rsfbclient_core::charset::UTF_8;

    /// A `Read` that hands out at most `chunk` bytes at a time — what a response
    /// split across TCP segments looks like to the client.
    struct Chunked {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.chunk.min(buf.len()).min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    /// One op_response: handle, object_id, empty data, empty status vector.
    fn op_response(handle: u32) -> Vec<u8> {
        let mut b = BytesMut::new();
        b.put_u32(WireOp::Response as u32);
        b.put_u32(handle);
        b.put_u64(0); // object_id
        b.put_wire_bytes(&[]); // data
        b.put_u32(ibase::isc_arg_end); // status vector
        b.to_vec()
    }

    fn read_one(data: Vec<u8>, chunk: usize) -> (Result<Response, FbError>, Bytes) {
        let mut socket = Chunked {
            data,
            pos: 0,
            chunk,
        };
        let mut buff = vec![0u8; 2048];
        let mut pending = Bytes::new();

        let resp = read_response(&mut socket, &mut buff, &mut pending, &mut 0);
        (resp, pending)
    }

    /// A response delivered one byte at a time must be reassembled, not
    /// truncated. This is the bug behind "Invalid Xsqlda received from server"
    /// and "Invalid server response, missing bytes".
    #[test]
    fn reassembles_response_split_across_reads() {
        let packet = op_response(42);

        for chunk in [1, 2, 3, 5, 7, 11, packet.len() - 1] {
            let (resp, pending) = read_one(packet.clone(), chunk);
            let resp = resp.unwrap_or_else(|e| panic!("chunk {chunk}: {e}"));

            assert_eq!(resp.handle, 42, "chunk {chunk}");
            assert!(pending.is_empty(), "chunk {chunk}: {} left", pending.len());
        }
    }

    /// Two responses arriving in one read: the second must survive in `pending`.
    /// Dropping it is what desynchronised the stream and made the next operation
    /// read a random u32 as its op code ("Connection rejected with code ...").
    #[test]
    fn keeps_surplus_bytes_for_the_next_read() {
        let mut data = op_response(1);
        data.extend_from_slice(&op_response(2));
        let len = data.len();

        let mut socket = Chunked {
            data,
            pos: 0,
            chunk: len, // both responses in a single read
        };
        let mut buff = vec![0u8; 2048];
        let mut pending = Bytes::new();

        let first = read_response(&mut socket, &mut buff, &mut pending, &mut 0).unwrap();
        assert_eq!(first.handle, 1);
        assert!(!pending.is_empty(), "second response was dropped");

        // The second read must be served from `pending` without touching the
        // socket, which is now exhausted.
        let second = read_response(&mut socket, &mut buff, &mut pending, &mut 0).unwrap();
        assert_eq!(second.handle, 2);
        assert!(pending.is_empty());
    }

    /// End-of-cursor is `[status=100][count]`; consuming only the status left the
    /// count behind for the next operation to read as its op code.
    #[test]
    fn end_of_cursor_consumes_the_whole_response() {
        let mut b = BytesMut::new();
        b.put_u32(WireOp::FetchResponse as u32);
        b.put_u32(100); // status: end of cursor
        b.put_u32(0); // count — must be consumed too
        let mut resp = b.freeze();

        let one = parse_one_fetch_response(&mut resp, &mut 0, &[], ProtocolVersion::V13, &UTF_8)
            .unwrap_or_else(|e| panic!("{e}"));

        assert!(matches!(one, FetchOne::End));
        assert!(
            resp.is_empty(),
            "{} bytes left for the next operation to trip over",
            resp.len()
        );
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
