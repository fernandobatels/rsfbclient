//! `FirebirdConnection` implementation for the pure rust firebird client

use bytes::{BufMut, Bytes, BytesMut};
use std::{
    collections::VecDeque,
    env,
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

use crate::{
    arc4::*,
    blr,
    consts::{AuthPluginType, ProtocolVersion, WireOp},
    events::*,
    raw::RawValue,
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
///
/// The server may end a batch earlier than requested (it stops after
/// MAX_PACKETS_PER_BATCH = 16 of its send buffers once at least 10 rows went
/// out), so a bigger value is a ceiling, not a promise — and the whole batch is
/// buffered in memory before rows are handed out.
fn fetch_batch_size() -> u32 {
    env::var("FB_FETCH_BATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(200)
}

/// Highest wire protocol version offered to the server (10..=19), defaults to
/// 19. `FB_MAX_PROTOCOL=13` is the escape hatch back to the pre-fork
/// negotiation (Firebird 3+ then stays at protocol 13: no inline blobs, but
/// also none of the version-gated execute fields).
fn max_protocol() -> u16 {
    env::var("FB_MAX_PROTOCOL")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .map(|n| n.clamp(10, 19))
        .unwrap_or(19)
}

/// Largest blob (data + 2-byte segment framing) the server may inline with a
/// row at protocol 19+. Defaults to the protocol maximum, 65535 — the same
/// default as fbclient's `MaxInlineBlobSize`. `FB_INLINE_BLOB_SIZE=0`
/// disables inlining; the pipelined open/get_segment/close path then handles
/// every blob.
fn inline_blob_size() -> u32 {
    env::var("FB_INLINE_BLOB_SIZE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map(|n| n.min(BLOB_SEGMENT_BUFFER))
        .unwrap_or(BLOB_SEGMENT_BUFFER)
}

/// Blobs opened per pipelined round in `resolve_blobs`. Each open blob holds
/// a server-side segment buffer of up to `BLOB_SEGMENT_BUFFER` bytes, so this
/// bounds server memory to ~2 MiB per connection while still amortising one
/// round trip over 32 blobs.
const BLOB_OPEN_WINDOW: usize = 32;

/// Max `op_get_segment` requests in flight in one round of `resolve_blobs`
/// (~2 MiB of responses). Same depth as fast-firebird's segment pipeline.
const BLOB_SEGMENT_WINDOW: usize = 32;

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

    /// Max blob size the server may inline with rows (protocol 19+), as sent
    /// in every op_execute. 0 when the protocol or the user turned it off.
    inline_blob_size: u32,

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
    /// The columns as the server described them at prepare, kept so
    /// `describe_statement` answers without a round trip.
    columns: Vec<ColumnDesc>,
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

    fn set_io_timeout(&mut self, timeout: Option<Duration>) -> Result<bool, FbError> {
        match self.conn.as_ref() {
            Some(conn) => {
                let tcp = conn.socket.tcp();
                tcp.set_read_timeout(timeout)?;
                tcp.set_write_timeout(timeout)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn database_info(
        &mut self,
        db_handle: &mut RustDbHandle,
        items: &[u8],
    ) -> Result<Vec<u8>, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.database_info(db_handle, items).map(|b| b.to_vec()))
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

    /// Answered from the prepare response: no round trip.
    fn describe_statement(
        &mut self,
        stmt_handle: &mut Self::StmtHandle,
    ) -> Result<Vec<ColumnDesc>, FbError> {
        Ok(stmt_handle.columns.clone())
    }

    fn statement_plan(&mut self, stmt_handle: &mut Self::StmtHandle) -> Result<StmtPlan, FbError> {
        self.conn
            .as_mut()
            .map(|conn| conn.statement_plan(stmt_handle))
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

impl StmtHandleData {
    /// Whether any output column is a blob. Raw streaming
    /// ([`RustFbClient::stream_raw`]) requires this to be false.
    pub fn has_blob(&self) -> bool {
        self.has_blob
    }
}

impl RustFbClient {
    /// Streaming fetch with no per-row allocation: each row is decoded straight
    /// into a reusable `Vec<RawValue>` (text borrows the read buffer, so there is
    /// no `Vec<Column>` and no `String` per row) and handed to `visit`. The
    /// statement must be blob-free (see [`StmtHandleData::has_blob`]).
    pub fn stream_raw<V>(
        &mut self,
        stmt_handle: &mut StmtHandleData,
        visit: V,
    ) -> Result<(), FbError>
    where
        V: FnMut(&[RawValue]) -> Result<(), FbError>,
    {
        self.conn
            .as_mut()
            .map(|conn| conn.stream_raw(stmt_handle, visit))
            .unwrap_or_else(err_client_not_connected)
    }
}

impl FirebirdWireConnection {
    /// Start a connection to the firebird server, for a database attach
    pub fn connect(
        host: &str,
        port: u16,
        db_name: &str,
        user: &str,
        pass: &str,
        charset: Charset,
    ) -> Result<Self, FbError> {
        Self::connect_for(host, port, WireOp::Attach, db_name, user, pass, charset)
    }

    /// Start a connection to the firebird server for `operation`
    /// (`op_attach` / `op_create` on a database, `op_service_attach` on the
    /// service manager), running the protocol negotiation and the
    /// authentication handshake. What follows — the attach itself — is sent
    /// by the caller.
    pub fn connect_for(
        host: &str,
        port: u16,
        operation: WireOp,
        db_name: &str,
        user: &str,
        pass: &str,
        charset: Charset,
    ) -> Result<Self, FbError> {
        let socket = connect_tcp(host, port)?;
        // The wire protocol is request/response with small writes, so Nagle has
        // nothing to coalesce and every statement waits out a delayed ACK
        // (~40ms). Measured against Firebird 2.5: SELECT 1 FROM RDB$DATABASE
        // 44ms -> 0.2ms, a 1000-row select 107ms -> 5.7ms.
        let _ = socket.set_nodelay(true);

        // normalize user
        let user = &normalize_user(user);

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

        let req = connect(
            operation,
            db_name,
            user,
            &username,
            &hostname,
            &srp_key,
            max_protocol(),
        );
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

        // Whether the password was verified during this handshake, so the
        // attach must not carry it again.
        let mut verified = continue_auth;

        if let Some(auth_plugin) = &mut auth_plugin {
            // Every pass after the first follows an op_cont_auth we sent: the
            // server's answer to it arrived as `auth_plugin`.
            let mut round = 0;
            loop {
                round += 1;
                match auth_plugin.kind {
                    AuthPluginType::Legacy => {
                        // Asked on op_cond_accept, or mid-exchange after an SRP
                        // round: answer with op_cont_auth now. On op_accept_data
                        // (WireCrypt = Disabled) the hash goes in the attach DPB
                        // instead — Firebird 5 rejects an extra round there.
                        if continue_auth || round > 1 {
                            socket.write_all(&cont_auth(
                                legacy_auth_data(pass).as_bytes(),
                                AuthPluginType::Legacy,
                                AuthPluginType::plugin_list(),
                                &[],
                            ))?;
                            socket.flush()?;

                            // op_response: a login error comes back as Err.
                            // No session key exists, so no wire encryption
                            // follows; a server with WireCrypt = Required
                            // refuses this login.
                            read_response(&mut socket, &mut buff, &mut pending, &mut 0)?;
                            verified = true;
                        }
                        break;
                    }
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

        // Inline blobs exist from protocol 19 on; an older server would reject
        // the extra op_execute field, so the size is only announced there.
        let inline_blob_size = if version >= ProtocolVersion::V19 {
            inline_blob_size()
        } else {
            0
        };

        Ok(Self {
            socket,
            version,
            buff,
            pending,
            lazy_count: 0,
            inline_blob_size,
            charset,
            auth_plugin: if verified {
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

    /// Database information (`op_info_database`): the raw reply stream for
    /// `items` (see `rsfbclient_core::dbinfo`).
    pub fn database_info(
        &mut self,
        db_handle: &mut DbHandle,
        items: &[u8],
    ) -> Result<Bytes, FbError> {
        self.socket.write_all(&info_database(db_handle.0, items))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(resp.data)
    }

    /// Attach to the service manager of the connected server. The connection
    /// must have been opened with [`Self::connect_for`] and
    /// `WireOp::ServiceAttach`. `expected_db` names the database the coming
    /// actions concern, for servers whose security database depends on it.
    pub fn service_attach(
        &mut self,
        user: &str,
        pass: &str,
        expected_db: Option<&str>,
    ) -> Result<SvcHandle, FbError> {
        self.socket.write_all(&service_attach(
            user,
            pass,
            self.version,
            expected_db,
            self.auth_plugin.as_ref(),
            &self.srp_key,
        )?)?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(SvcHandle(resp.handle))
    }

    /// Detach from the service manager
    pub fn service_detach(&mut self, svc_handle: SvcHandle) -> Result<(), FbError> {
        self.socket.write_all(&service_detach(svc_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Start a service action; `request` is the action byte and its
    /// arguments (see `rsfbclient_core::services`). Returns as soon as the
    /// server accepted it — the action then runs on the server, and its
    /// output is read with [`Self::service_query`].
    pub fn service_start(&mut self, svc_handle: SvcHandle, request: &[u8]) -> Result<(), FbError> {
        self.socket
            .write_all(&service_start(svc_handle.0, request))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Query the service manager: the server answers the `receive_items`
    /// into a buffer of at most `buffer_length` bytes, returned as-is. For
    /// `isc_info_svc_line` this blocks until the running action has a line
    /// to give, or has finished.
    pub fn service_query(
        &mut self,
        svc_handle: SvcHandle,
        send_items: &[u8],
        receive_items: &[u8],
        buffer_length: u32,
    ) -> Result<Bytes, FbError> {
        self.socket.write_all(&service_info(
            svc_handle.0,
            send_items,
            receive_items,
            buffer_length,
        ))?;
        self.socket.flush()?;

        let resp = self.read_response()?;

        Ok(resp.data)
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

        // What the server said, before coercion rewrites the types for the fetch.
        let columns: Vec<ColumnDesc> = xsqlda.iter().map(XSqlVar::describe).collect();

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
                columns,
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

    /// The plan the server chose for a prepared statement: `op_info_sql`
    /// asking `isc_info_sql_get_plan` + `isc_info_sql_explain_plan`. One
    /// round trip; when the two together overflow the info buffer, each is
    /// asked again on its own so it gets the whole buffer.
    pub fn statement_plan(
        &mut self,
        stmt_handle: &mut StmtHandleData,
    ) -> Result<StmtPlan, FbError> {
        let handle = stmt_handle.handle.0;
        let mut info = self.plan_info(handle, &PLAN_INFO_ITEMS)?;
        if info.truncated {
            let plan = self.plan_info(handle, &PLAN_INFO_ITEMS[..1])?;
            let explained = self.plan_info(handle, &PLAN_INFO_ITEMS[1..])?;
            info = PlanInfo {
                plan: plan.plan,
                explained: explained.explained,
                truncated: plan.truncated || explained.truncated,
            };
        }
        let charset = &self.charset;
        StmtPlan::from_info(info, |b| charset.decode(b))
    }

    fn plan_info(&mut self, stmt_handle: u32, items: &[u8]) -> Result<PlanInfo, FbError> {
        self.socket.write_all(&info_sql(stmt_handle, items))?;
        self.socket.flush()?;

        let data = self.read_response()?.data;

        Ok(parse_plan_info(&data))
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
            self.version,
            self.inline_blob_size,
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
            self.version,
            self.inline_blob_size,
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
                // Protocol 19+: the server may inline the row's blobs first.
                // Collected per attempt — `read_with` may re-run this closure
                // from the start of the buffer when the response is incomplete.
                let mut inline = Vec::new();
                let op_code = skip_lazy_responses(resp, lazy_count)?;
                let op_code = read_inline_blobs(resp, op_code, &mut inline)?;

                if op_code == WireOp::Response as u32 {
                    // An error ocurred
                    parse_response(resp)?;
                }

                if op_code != WireOp::SqlResponse as u32 {
                    return err_conn_rejected(op_code);
                }

                let parsed_cols = parse_sql_response(resp, xsqlda, version, &charset, &inline)?;

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
        let mut rows = self.resolve_blobs(tr_handle, vec![parsed_cols])?;

        Ok(rows.pop().unwrap_or_default())
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

        // All blobs of the batch in a few pipelined round trips, instead of
        // open + get_segment… + close per blob.
        stmt_handle
            .prefetched
            .extend(self.resolve_blobs(tr_handle, rows)?);

        Ok(())
    }

    /// Completes the blob columns of a batch of parsed rows.
    ///
    /// Serially a blob costs open + N×get_segment + close, each a round trip —
    /// on a remote link that dominated everything else (a metadata scan of a
    /// few thousand objects took minutes). Here the blobs of the whole batch
    /// are driven through a pipeline, `BLOB_OPEN_WINDOW` at a time:
    ///
    /// 1. every `op_open_blob` of the window goes out in one write, then the
    ///    responses are read back in order (1 round trip for 32 blobs);
    /// 2. one `op_get_segment` per open blob, again one write, responses in
    ///    order; blobs that did not reach EOF get a growing share of
    ///    `BLOB_SEGMENT_WINDOW` requests in the next round (1 round trip per
    ///    round, however many blobs it covers). Requests past EOF are safe:
    ///    the server answers them with a clean, empty EOF response;
    /// 3. `op_close_blob` is written for each handle but its response is left
    ///    pending (`lazy_count`): the server replies at once and
    ///    `skip_lazy_responses` consumes it ahead of the next real response,
    ///    so closes ride for free.
    ///
    /// Responses arrive strictly in request order, so a plain FIFO over the
    /// pipelined requests keeps the stream in sync. On a server error every
    /// outstanding response is still read, so the connection stays usable.
    fn resolve_blobs(
        &mut self,
        tr_handle: &mut TrHandle,
        rows: Vec<Vec<ParsedColumn>>,
    ) -> Result<Vec<Vec<Column>>, FbError> {
        /// A blob cell waiting for its data
        struct Pending {
            row: usize,
            col: usize,
            binary: bool,
            id: BlobId,
        }

        let mut pending: Vec<Pending> = Vec::new();
        let mut out: Vec<Vec<Column>> = Vec::with_capacity(rows.len());

        for (row, parsed) in rows.into_iter().enumerate() {
            let mut cols = Vec::with_capacity(parsed.len());
            for (col, pc) in parsed.into_iter().enumerate() {
                match pc {
                    ParsedColumn::Complete(c) => cols.push(c),
                    ParsedColumn::Blob {
                        binary,
                        id,
                        col_name,
                    } => {
                        pending.push(Pending {
                            row,
                            col,
                            binary,
                            id,
                        });
                        // Placeholder, filled below.
                        cols.push(Column::new(col_name, ibase::SQL_BLOB, SqlType::Null));
                    }
                }
            }
            out.push(cols);
        }

        for window in pending.chunks(BLOB_OPEN_WINDOW) {
            // 1. Open every blob of the window in one round trip.
            let mut req = BytesMut::with_capacity(16 * window.len());
            for p in window {
                req.put(open_blob(tr_handle.0, p.id.0));
            }
            self.socket.write_all(&req)?;
            self.socket.flush()?;

            let mut first_err: Option<FbError> = None;
            let mut handles: Vec<Option<BlobHandle>> = Vec::with_capacity(window.len());
            for _ in window {
                match self.read_response() {
                    Ok(resp) => handles.push(Some(BlobHandle(resp.handle))),
                    Err(e) => {
                        handles.push(None);
                        first_err.get_or_insert(e);
                    }
                }
            }

            // 2. Segment rounds until every open blob reached EOF.
            let mut data: Vec<Vec<u8>> = vec![Vec::new(); window.len()];
            let mut eof: Vec<bool> = handles.iter().map(|h| h.is_none()).collect();
            // Requests per blob in the next round: 1 for an unknown blob,
            // doubled (up to the window) each time a blob consumes its whole
            // share without hitting EOF — big blobs stream many segments per
            // round trip, small blobs cost no over-reads.
            let mut share: Vec<usize> = vec![1; window.len()];

            loop {
                let mut plan: Vec<usize> = Vec::new(); // blob index per request, in order
                let mut budget = BLOB_SEGMENT_WINDOW;
                for (i, &done) in eof.iter().enumerate() {
                    if done || budget == 0 {
                        continue;
                    }
                    let n = share[i].min(budget);
                    plan.extend(std::iter::repeat(i).take(n));
                    budget -= n;
                }
                if plan.is_empty() {
                    break;
                }

                let mut req = BytesMut::with_capacity(16 * plan.len());
                for &i in &plan {
                    let handle = handles[i].expect("planned segment for an unopened blob");
                    req.put(get_segment(handle.0, BLOB_SEGMENT_BUFFER));
                }
                self.socket.write_all(&req)?;
                self.socket.flush()?;

                let mut got: Vec<usize> = vec![0; window.len()];
                for &i in &plan {
                    match self.read_response() {
                        Ok(resp) => {
                            if eof[i] {
                                // Over-read past EOF: clean empty response, nothing to keep.
                                continue;
                            }
                            // handle == 2 (isc_segstr_eof): no more data after this.
                            let end = resp.handle == 2;
                            match unframe_segments(resp.data) {
                                Ok(segment) => data[i].extend_from_slice(&segment),
                                Err(e) => {
                                    eof[i] = true;
                                    first_err.get_or_insert(e);
                                    continue;
                                }
                            }
                            if end {
                                eof[i] = true;
                            } else {
                                got[i] += 1;
                            }
                        }
                        Err(e) => {
                            eof[i] = true;
                            first_err.get_or_insert(e);
                        }
                    }
                }

                for i in 0..window.len() {
                    if !eof[i] && got[i] >= share[i] {
                        share[i] = (share[i] * 2).min(BLOB_SEGMENT_WINDOW);
                    }
                }
            }

            // 3. Close the handles; the responses are consumed lazily.
            for handle in handles.iter().flatten() {
                self.close_blob_lazy(*handle)?;
            }

            if let Some(e) = first_err {
                return Err(e);
            }

            for (k, p) in window.iter().enumerate() {
                let bytes = std::mem::take(&mut data[k]);
                out[p.row][p.col].value = if p.binary {
                    SqlType::Binary(bytes)
                } else {
                    SqlType::Text(self.charset.decode(bytes)?)
                };
            }
        }

        Ok(out)
    }

    /// Streaming fetch that decodes each row straight into a reusable
    /// `Vec<RawValue>` (text borrows the read buffer; no `Vec<Column>`, no
    /// per-text `String`) and hands it to `visit`. Requires a blob-free statement
    /// and a cursor with no rows buffered by [`Self::fetch`].
    pub fn stream_raw<V>(
        &mut self,
        stmt_handle: &mut StmtHandleData,
        mut visit: V,
    ) -> Result<(), FbError>
    where
        V: FnMut(&[RawValue]) -> Result<(), FbError>,
    {
        if stmt_handle.has_blob {
            return Err("stream_raw requires a statement with no blob columns".into());
        }
        if !stmt_handle.prefetched.is_empty() {
            return Err("stream_raw cannot resume a cursor with rows buffered by fetch".into());
        }

        let count = fetch_batch_size();
        let mut row = Vec::new();
        let mut empty_batches = 0u32;

        while !stmt_handle.cursor_eof {
            let delivered = self.fetch_batch_raw(stmt_handle, count, &mut row, &mut visit)?;

            // Same safety net as `fetch`: a well-behaved server never sends empty
            // batches without exhausting the cursor.
            if delivered == 0 {
                empty_batches += 1;
                if empty_batches > 1000 {
                    return Err("stream_raw: too many empty batches without end of cursor".into());
                }
            } else {
                empty_batches = 0;
            }
        }

        Ok(())
    }

    /// One op_fetch of up to `count` rows, decoded into `row` and delivered to
    /// `visit` as they are parsed. Returns how many rows were delivered.
    fn fetch_batch_raw<V>(
        &mut self,
        stmt_handle: &mut StmtHandleData,
        count: u32,
        row: &mut Vec<RawValue>,
        visit: &mut V,
    ) -> Result<u32, FbError>
    where
        V: FnMut(&[RawValue]) -> Result<(), FbError>,
    {
        self.socket
            .write_all(&fetch(stmt_handle.handle.0, &stmt_handle.blr, count))?;
        self.socket.flush()?;

        let version = self.version;
        let xsqlda = &stmt_handle.xsqlda;
        let mut cursor_eof = false;
        let mut got = 0u32;

        loop {
            let framed = read_with(
                &mut self.socket,
                &mut self.buff,
                &mut self.pending,
                &mut self.lazy_count,
                |resp, lazy_count| {
                    parse_one_fetch_response_raw(resp, lazy_count, xsqlda, version, row)
                },
            )?;

            match framed {
                // `row` borrows the read buffer, so it has to be consumed before
                // the next response is read.
                Framed::Row => {
                    visit(row)?;
                    got += 1;
                    if got > count {
                        return Err("server sent more rows than requested in op_fetch".into());
                    }
                }
                Framed::BatchEnd => break,
                Framed::End => {
                    cursor_eof = true;
                    break;
                }
            }
        }

        stmt_handle.cursor_eof = cursor_eof;

        Ok(got)
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

    /// Open a blob, returning the blob handle.
    ///
    /// Single-blob reference path; batches go through `resolve_blobs`.
    #[allow(dead_code)]
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

    /// Get a blob segment, returns the bytes and true if there is more data.
    /// Requests the protocol-maximum 64 KiB per round trip.
    ///
    /// Single-blob reference path; batches go through `resolve_blobs`.
    #[allow(dead_code)]
    pub fn get_segment(&mut self, blob_handle: BlobHandle) -> Result<(Bytes, bool), FbError> {
        self.socket
            .write_all(&get_segment(blob_handle.0, BLOB_SEGMENT_BUFFER))?;
        self.socket.flush()?;

        let resp = self.read_response()?;
        let blob_data = unframe_segments(resp.data)?;

        Ok((blob_data, resp.handle == 2))
    }

    /// Closes a blob handle
    pub fn close_blob(&mut self, blob_handle: BlobHandle) -> Result<(), FbError> {
        self.socket.write_all(&close_blob(blob_handle.0))?;
        self.socket.flush()?;

        self.read_response()?;

        Ok(())
    }

    /// Closes a blob handle without waiting for the answer. The server sends
    /// the op_response for a close immediately (`end_blob` never defers it),
    /// so it is consumed as a lazy response ahead of the next read — same
    /// mechanism as `free_statement`.
    fn close_blob_lazy(&mut self, blob_handle: BlobHandle) -> Result<(), FbError> {
        self.socket.write_all(&close_blob(blob_handle.0))?;
        self.lazy_count += 1;

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

    /// A connection over an already-open plaintext socket, skipping the
    /// handshake — for loopback tests against a scripted fake server.
    #[cfg(test)]
    pub(crate) fn test_over_socket(socket: TcpStream, version: ProtocolVersion) -> Self {
        Self {
            socket: FbStream::Plain(socket),
            version,
            buff: vec![0; READ_BUFFER_LEN].into_boxed_slice(),
            pending: Bytes::new(),
            lazy_count: 0,
            inline_blob_size: 0,
            charset: rsfbclient_core::charset::UTF_8,
            auth_plugin: None,
            srp_key: [0; 32],
            event_channel: None,
            next_event_id: 1,
        }
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

/// Consumes the `op_inline_blob` packets (protocol 19+) the server sends ahead
/// of a row, collecting them into `inline`, and returns the op code of the
/// packet that follows them. `op_code` is the code already read; it is handed
/// back untouched when it is not an inline blob.
fn read_inline_blobs(
    resp: &mut Bytes,
    mut op_code: u32,
    inline: &mut Vec<InlineBlob>,
) -> Result<u32, FbError> {
    while op_code == WireOp::InlineBlob as u32 {
        inline.push(parse_inline_blob(resp)?);
        op_code = next_op_code(resp)?;
    }

    Ok(op_code)
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
    // Blobs the server inlined ahead of this row (protocol 19+). Fresh per
    // attempt: `read_with` re-runs the parse from the start of the buffer
    // when the response turns out incomplete.
    let mut inline = Vec::new();

    match frame_fetch_response(resp, lazy_count, &mut inline)? {
        Framed::End => return Ok(FetchOne::End),
        Framed::BatchEnd => return Ok(FetchOne::BatchEnd),
        Framed::Row => {}
    }

    // Delegate to the crate parser (re-reads status+messages+data).
    match parse_fetch_response(resp, xsqlda, version, charset, &inline)? {
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
    // Blob-free statement: the server never inlines anything for it.
    match frame_fetch_response(resp, lazy_count, &mut Vec::new())? {
        Framed::End => return Ok(FetchOne::End),
        Framed::BatchEnd => return Ok(FetchOne::BatchEnd),
        Framed::Row => {}
    }

    match parse_fetch_response_columns(resp, xsqlda, version, charset)? {
        Some(cols) => Ok(FetchOne::Row(cols)),
        None => Ok(FetchOne::End),
    }
}

/// Same as [`parse_one_fetch_response`], decoding the row into `out` as
/// [`RawValue`] instead of building a `Vec<Column>`.
fn parse_one_fetch_response_raw(
    resp: &mut Bytes,
    lazy_count: &mut u32,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    out: &mut Vec<RawValue>,
) -> Result<Framed, FbError> {
    // Blob-free statement: the server never inlines anything for it.
    match frame_fetch_response(resp, lazy_count, &mut Vec::new())? {
        Framed::End => Ok(Framed::End),
        Framed::BatchEnd => Ok(Framed::BatchEnd),
        Framed::Row => {
            if parse_fetch_response_raw(resp, xsqlda, version, out)? {
                Ok(Framed::Row)
            } else {
                Ok(Framed::End)
            }
        }
    }
}

/// Frames ONE op_fetch_response: consumes the response when it carries no row,
/// and leaves `resp` at the start of the body when it does, so the caller can
/// decode the row the way it needs.
fn frame_fetch_response(
    resp: &mut Bytes,
    lazy_count: &mut u32,
    inline: &mut Vec<InlineBlob>,
) -> Result<Framed, FbError> {
    let op_code = skip_lazy_responses(resp, lazy_count)?;
    let op_code = read_inline_blobs(resp, op_code, inline)?;

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

/// Normalizes the username to uppercase, unless it is encased by `"`
fn normalize_user(user: &str) -> String {
    if user.starts_with("\"") && user.ends_with("\"") {
        user.trim_matches('"').to_string()
    } else {
        user.to_uppercase()
    }
}

#[test]
fn test_normalize_user() {
    assert_eq!(normalize_user("Testuser"), "TESTUSER");
    assert_eq!(normalize_user("testuser"), "TESTUSER");
    assert_eq!(normalize_user("\"Testuser\""), "Testuser");
}

#[derive(Debug, Clone, Copy)]
/// A database handle
pub struct DbHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A transaction handle
pub struct TrHandle(u32);

#[derive(Debug, Clone, Copy)]
/// A service manager attachment handle
pub struct SvcHandle(u32);

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

    fn tcp(&self) -> &TcpStream {
        match self {
            FbStream::Plain(s) => s,
            FbStream::Arc4(s) => s.get_ref(),
        }
    }
}

/// How long opening the socket may take before the server counts as
/// unreachable. The system's own limit is a minute or more.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Open the socket to `host:port`, trying each address it resolves to, each
/// bounded by [`CONNECT_TIMEOUT`], with keep alive tuned (see [`keep_alive`]).
fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, FbError> {
    let mut last = None;
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(socket) => {
                keep_alive(&socket);
                return Ok(socket);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last
        .unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{host} resolves to no address"),
            )
        })
        .into())
}

/// Probe an idle connection after 30 s, every 10 s, three times: a server
/// that vanished without closing the socket (a dropped VPN, a NAT entry that
/// expired, a host that went to sleep) is noticed within about a minute while
/// waiting on a long query, instead of after the system's two hours. A live
/// server answers the probes from its TCP stack, so a long query is not cut.
/// Best effort, like firebird's own `setKeepAlive`.
fn keep_alive(socket: &TcpStream) {
    let ka = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10))
        .with_retries(3);
    let _ = socket2::SockRef::from(socket).set_tcp_keepalive(&ka);
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

/// Tests of the batch/blob paths against a scripted, Firebird-shaped peer on a
/// loopback socket (no real server, no database). The peer reproduces what
/// server.cpp does for the ops involved: rows (and, at protocol 19, the
/// op_inline_blob packets ahead of them) in answer to op_fetch; open /
/// get_segment (segments framed `u16le len + bytes`, EOF reported through
/// handle 2 — also for requests past EOF) / close served strictly in request
/// order, each close answered at once. Pipelining falls out of that order.
#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use rsfbclient_core::charset::UTF_8;
    use std::collections::HashMap;
    use std::io::BufReader;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug)]
    enum Cell {
        Int(i64),
        Blob(Option<u64>),
    }

    /// One op_fetch answer: rows, then a terminator (0 = batch end, 100 = cursor end).
    struct Batch {
        rows: Vec<Vec<Cell>>,
        status: u32,
    }

    #[derive(Default, Debug)]
    struct Stats {
        fetches: usize,
        opens: usize,
        segments: usize,
        closes: usize,
        inlined: usize,
        commits: usize,
    }

    struct FakeServer {
        blobs: HashMap<u64, Vec<u8>>,
        batches: Vec<Batch>,
        /// Largest framed blob inlined ahead of a row (0 = never, protocol < 19).
        inline_size: usize,
        /// Longest segment the fake engine stores (forces multi-segment responses).
        seg_max: usize,
        stats: Arc<Mutex<Stats>>,
    }

    fn read_u32(r: &mut impl Read) -> u32 {
        let mut b = [0u8; 4];
        r.read_exact(&mut b).expect("peer closed");
        u32::from_be_bytes(b)
    }

    fn read_u64(r: &mut impl Read) -> u64 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b).unwrap();
        u64::from_be_bytes(b)
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

    fn op_error(gds_code: u32) -> Vec<u8> {
        let mut b = BytesMut::new();
        b.put_u32(WireOp::Response as u32);
        b.put_u32(0);
        b.put_u64(0);
        b.put_wire_bytes(&[]);
        b.put_u32(ibase::isc_arg_gds);
        b.put_u32(gds_code);
        b.put_u32(ibase::isc_arg_end);
        b.to_vec()
    }

    fn frame(segments: &[&[u8]]) -> Vec<u8> {
        let mut data = BytesMut::new();
        for seg in segments {
            data.put_u16_le(seg.len() as u16);
            data.put_slice(seg);
        }
        data.to_vec()
    }

    impl FakeServer {
        fn serve(mut self, stream: TcpStream) {
            let mut rd = BufReader::new(stream.try_clone().unwrap());
            let mut wr = stream;
            // blob handle -> (blob id, read position)
            let mut open: HashMap<u32, (u64, usize)> = HashMap::new();
            let mut next_handle = 100u32;
            let mut batches = self.batches.drain(..).collect::<Vec<_>>().into_iter();

            loop {
                let mut op = [0u8; 4];
                if rd.read_exact(&mut op).is_err() {
                    return; // client went away
                }
                let op = u32::from_be_bytes(op);

                if op == WireOp::Fetch as u32 {
                    let _stmt = read_u32(&mut rd);
                    let _blr = read_cstring(&mut rd);
                    let _msg = read_u32(&mut rd);
                    let _count = read_u32(&mut rd);
                    self.stats.lock().unwrap().fetches += 1;
                    let batch = batches.next().expect("more op_fetch than scripted batches");
                    let mut out = BytesMut::new();
                    for row in &batch.rows {
                        // Inline blobs ride ahead of the row that references them.
                        for cell in row {
                            if self.inline_size == 0 {
                                break;
                            }
                            if let Cell::Blob(Some(id)) = cell {
                                let Some(data) = self.blobs.get(id) else {
                                    continue;
                                };
                                let framed = self.framed_whole(data);
                                if framed.len() <= self.inline_size {
                                    out.put_u32(WireOp::InlineBlob as u32);
                                    out.put_u32(1); // transaction id
                                    out.put_u64(*id);
                                    out.put_wire_bytes(&[6, 4, 0, 0, 0, 0, 0, 1]);
                                    out.put_wire_bytes(&framed);
                                    self.stats.lock().unwrap().inlined += 1;
                                }
                            }
                        }
                        out.put_u32(WireOp::FetchResponse as u32);
                        out.put_u32(0); // status
                        out.put_u32(1); // messages
                        let mut bitmap = vec![0u8; ((row.len() + 7) / 8 + 3) & !3];
                        for (i, cell) in row.iter().enumerate() {
                            if matches!(cell, Cell::Blob(None)) {
                                bitmap[i / 8] |= 1 << (i % 8);
                            }
                        }
                        out.put_slice(&bitmap);
                        for cell in row {
                            match cell {
                                Cell::Int(v) => out.put_i64(*v),
                                Cell::Blob(Some(id)) => out.put_u64(*id),
                                Cell::Blob(None) => {}
                            }
                        }
                    }
                    out.put_u32(WireOp::FetchResponse as u32);
                    out.put_u32(batch.status);
                    out.put_u32(0);
                    wr.write_all(&out).unwrap();
                } else if op == WireOp::OpenBlob as u32 {
                    let _tr = read_u32(&mut rd);
                    let id = read_u64(&mut rd);
                    self.stats.lock().unwrap().opens += 1;
                    if self.blobs.contains_key(&id) {
                        let h = next_handle;
                        next_handle += 1;
                        open.insert(h, (id, 0));
                        wr.write_all(&op_response(h, &[])).unwrap();
                    } else {
                        wr.write_all(&op_error(335544329)).unwrap(); // invalid BLOB ID
                    }
                } else if op == WireOp::GetSegment as u32 {
                    let h = read_u32(&mut rd);
                    let mut buffer_length = read_u32(&mut rd) as usize;
                    let _seg = read_cstring(&mut rd);
                    self.stats.lock().unwrap().segments += 1;
                    let (id, pos) = open.get_mut(&h).expect("get_segment on unknown handle");
                    let data = &self.blobs[id];
                    // rem_port::get_segment: pack segments while the buffer has room;
                    // a getSegment that finds no data sets state 2 (EOF).
                    let mut out = Vec::new();
                    let mut state = 0u32;
                    while buffer_length > 2 {
                        buffer_length -= 2;
                        if *pos >= data.len() {
                            state = 2;
                            break;
                        }
                        let take = (data.len() - *pos).min(self.seg_max).min(buffer_length);
                        out.extend_from_slice(&(take as u16).to_le_bytes());
                        out.extend_from_slice(&data[*pos..*pos + take]);
                        *pos += take;
                        buffer_length -= take;
                    }
                    wr.write_all(&op_response(state, &out)).unwrap();
                } else if op == WireOp::CloseBlob as u32 {
                    let h = read_u32(&mut rd);
                    assert!(
                        open.remove(&h).is_some(),
                        "close of unknown blob handle {h}"
                    );
                    self.stats.lock().unwrap().closes += 1;
                    wr.write_all(&op_response(0, &[])).unwrap(); // end_blob: never deferred
                } else if op == WireOp::Commit as u32 {
                    let _tr = read_u32(&mut rd);
                    self.stats.lock().unwrap().commits += 1;
                    assert!(open.is_empty(), "commit with blobs still open: {open:?}");
                    wr.write_all(&op_response(0, &[])).unwrap();
                } else {
                    panic!("fake server: unexpected op {op}");
                }
            }
        }

        fn framed_whole(&self, data: &[u8]) -> Vec<u8> {
            let segs: Vec<&[u8]> = data.chunks(self.seg_max.max(1)).collect();
            frame(&segs)
        }
    }

    /// Connects a client to `server` running on its own thread.
    fn connect_to(server: FakeServer, version: ProtocolVersion) -> FirebirdWireConnection {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            server.serve(stream);
        });
        let socket = TcpStream::connect(addr).unwrap();
        socket.set_nodelay(true).unwrap();

        FirebirdWireConnection {
            socket: FbStream::Plain(socket),
            version,
            buff: vec![0; READ_BUFFER_LEN].into_boxed_slice(),
            pending: Bytes::new(),
            lazy_count: 0,
            inline_blob_size: if version >= ProtocolVersion::V19 {
                65535
            } else {
                0
            },
            charset: UTF_8,
            auth_plugin: None,
            srp_key: [0; 32],
            event_channel: None,
            next_event_id: 1,
        }
    }

    fn stmt(cols: &[(&str, i16)]) -> StmtHandleData {
        let xsqlda: Vec<XSqlVar> = cols
            .iter()
            .map(|(name, subtype)| XSqlVar {
                sqltype: if *subtype < 0 {
                    ibase::SQL_INT64 as i16 + 1
                } else {
                    ibase::SQL_BLOB as i16 + 1
                },
                sqlsubtype: (*subtype).max(0),
                alias_name: (*name).into(),
                ..Default::default()
            })
            .collect();
        StmtHandleData {
            handle: StmtHandle(7),
            xsqlda,
            columns: vec![],
            blr: Bytes::new(),
            param_count: 0,
            prefetched: VecDeque::new(),
            cursor_eof: false,
            has_blob: true,
        }
    }

    fn text(c: &Column) -> &str {
        match &c.value {
            SqlType::Text(t) => t,
            other => panic!("{}: expected text, got {other:?}", c.name),
        }
    }

    fn binary(c: &Column) -> &[u8] {
        match &c.value {
            SqlType::Binary(b) => b,
            other => panic!("{}: expected binary, got {other:?}", c.name),
        }
    }

    fn fetch_all(conn: &mut FirebirdWireConnection, st: &mut StmtHandleData) -> Vec<Vec<Column>> {
        let mut tr = TrHandle(1);
        let mut rows = Vec::new();
        while let Some(row) = conn.fetch(&mut tr, st).unwrap() {
            rows.push(row);
        }
        rows
    }

    fn big(n: usize, seed: u8) -> Vec<u8> {
        (0..n)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    /// `n` bytes of ASCII (valid in any charset) for text blobs.
    fn big_text(n: usize) -> Vec<u8> {
        (0..n).map(|i| b'a' + (i % 26) as u8).collect()
    }

    /// Text, binary, NULL, empty, multi-segment, exactly-one-buffer and
    /// several-round-trip blobs across two batches, with the same blob id
    /// used twice. Then a commit, which only succeeds if every lazily-closed
    /// handle's response was consumed in order.
    #[test]
    fn batch_blobs_resolve_through_the_pipeline_and_leave_the_stream_in_sync() {
        let stats = Arc::new(Mutex::new(Stats::default()));
        let mut blobs = HashMap::new();
        blobs.insert(1, "procedure source — ação".as_bytes().to_vec());
        blobs.insert(2, big(200_000, 3)); // several 64 KiB rounds, share ramps 1→2→4
        blobs.insert(3, big_text(65533)); // fills one buffer exactly: EOF only on the next request
        blobs.insert(4, Vec::new()); // empty blob: EOF with no data
        blobs.insert(5, big(70_000, 9)); // one byte more than a buffer
        let server = FakeServer {
            blobs,
            batches: vec![
                Batch {
                    rows: vec![
                        vec![Cell::Blob(Some(1)), Cell::Int(10), Cell::Blob(Some(2))],
                        vec![Cell::Blob(None), Cell::Int(20), Cell::Blob(Some(4))],
                        vec![Cell::Blob(Some(3)), Cell::Int(30), Cell::Blob(Some(5))],
                    ],
                    status: 0, // more batches follow
                },
                Batch {
                    rows: vec![vec![Cell::Blob(Some(1)), Cell::Int(40), Cell::Blob(None)]],
                    status: 100,
                },
            ],
            inline_size: 0,
            seg_max: 30_000,
            stats: stats.clone(),
        };
        let mut conn = connect_to(server, ProtocolVersion::V13);
        let mut st = stmt(&[("SRC", 1), ("N", -1), ("BIN", 0)]);

        let rows = fetch_all(&mut conn, &mut st);
        assert_eq!(rows.len(), 4);

        assert_eq!(text(&rows[0][0]), "procedure source — ação");
        assert!(matches!(rows[0][1].value, SqlType::Integer(10)));
        assert_eq!(binary(&rows[0][2]), &big(200_000, 3)[..]);

        assert!(rows[1][0].value.is_null());
        assert_eq!(rows[1][0].raw_type, ibase::SQL_BLOB);
        assert_eq!(binary(&rows[1][2]), &[] as &[u8]);

        assert_eq!(text(&rows[2][0]).len(), 65533);
        assert_eq!(text(&rows[2][0]).as_bytes(), &big_text(65533)[..]);
        assert_eq!(binary(&rows[2][2]), &big(70_000, 9)[..]);

        assert_eq!(text(&rows[3][0]), "procedure source — ação");
        assert!(rows[3][2].value.is_null());

        // The pending lazy close responses are skipped ahead of the commit's.
        conn.transaction_operation(&mut TrHandle(1), TrOp::Commit)
            .unwrap();

        let s = stats.lock().unwrap();
        assert_eq!(s.fetches, 2);
        assert_eq!(s.opens, 6, "one open per non-NULL blob cell");
        assert_eq!(s.closes, 6, "every handle closed");
        assert_eq!(s.commits, 1);
        // 200 000 B → 4 data responses (64 KiB each) + over-reads from the ramp;
        // 65 533 B → 1 data + 1 EOF; 70 000 → 2; empty → 1; text → 1 (×2).
        assert!(s.segments >= 6 + 4, "{s:?}");
        assert!(s.segments <= 24, "runaway over-reading: {s:?}");
    }

    /// Protocol 19: blobs the server inlines cost no open/get_segment/close at
    /// all; those over the announced size go through the pipeline.
    #[test]
    fn inlined_blobs_skip_the_blob_round_trips() {
        let stats = Arc::new(Mutex::new(Stats::default()));
        let mut blobs = HashMap::new();
        blobs.insert(1, "memo".as_bytes().to_vec());
        blobs.insert(2, big(100_000, 1)); // over 65535 framed: not inlined
        blobs.insert(4, Vec::new());
        let server = FakeServer {
            blobs,
            batches: vec![Batch {
                rows: vec![
                    vec![Cell::Blob(Some(1)), Cell::Blob(Some(2))],
                    vec![Cell::Blob(Some(4)), Cell::Blob(None)],
                ],
                status: 100,
            }],
            inline_size: 65535,
            seg_max: 65535,
            stats: stats.clone(),
        };
        let mut conn = connect_to(server, ProtocolVersion::V19);
        let mut st = stmt(&[("MEMO", 1), ("DOC", 0)]);

        let rows = fetch_all(&mut conn, &mut st);
        assert_eq!(rows.len(), 2);
        assert_eq!(text(&rows[0][0]), "memo");
        assert_eq!(binary(&rows[0][1]), &big(100_000, 1)[..]);
        assert_eq!(text(&rows[1][0]), "");
        assert!(rows[1][1].value.is_null());

        conn.transaction_operation(&mut TrHandle(1), TrOp::Commit)
            .unwrap();

        let s = stats.lock().unwrap();
        assert_eq!(s.inlined, 2, "memo and the empty blob were inlined");
        assert_eq!(
            s.opens, 1,
            "only the 100 KB blob went through open/get_segment/close"
        );
        assert_eq!(s.closes, 1);
    }

    /// A server error on one blob of the window (here an invalid id) fails the
    /// fetch, but every outstanding response is drained and the opened handles
    /// closed, so the connection keeps working.
    #[test]
    fn a_blob_error_is_reported_and_the_connection_stays_usable() {
        let stats = Arc::new(Mutex::new(Stats::default()));
        let mut blobs = HashMap::new();
        blobs.insert(1, "ok".as_bytes().to_vec());
        blobs.insert(3, big(70_000, 2));
        let server = FakeServer {
            blobs,
            batches: vec![
                Batch {
                    rows: vec![
                        vec![Cell::Blob(Some(1))],
                        vec![Cell::Blob(Some(99))], // unknown id → invalid BLOB ID on open
                        vec![Cell::Blob(Some(3))],
                    ],
                    status: 0,
                },
                Batch {
                    rows: vec![vec![Cell::Blob(Some(1))]],
                    status: 100,
                },
            ],
            inline_size: 0,
            seg_max: 65535,
            stats: stats.clone(),
        };
        let mut conn = connect_to(server, ProtocolVersion::V13);
        let mut st = stmt(&[("SRC", 1)]);
        let mut tr = TrHandle(1);

        let err = conn.fetch(&mut tr, &mut st).unwrap_err().to_string();
        assert!(err.contains("invalid BLOB ID"), "{err}");

        // Next batch: served correctly, nothing left over from the failed one.
        let row = conn.fetch(&mut tr, &mut st).unwrap().expect("a row");
        assert_eq!(text(&row[0]), "ok");
        assert!(conn.fetch(&mut tr, &mut st).unwrap().is_none());
        conn.transaction_operation(&mut tr, TrOp::Commit).unwrap();

        let s = stats.lock().unwrap();
        assert_eq!(s.opens, 4);
        assert_eq!(
            s.closes, 3,
            "the two handles that did open, plus the second batch's"
        );
        assert_eq!(s.commits, 1);
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

#[cfg(test)]
mod io_timeout_tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    /// A server that accepts and then never says a word: what a vanished host
    /// looks like from the client until TCP gives up.
    fn silent_server() -> (SocketAddr, std::sync::mpsc::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (hold, release) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = release.recv();
        });
        (addr, hold)
    }

    #[test]
    fn a_bounded_read_on_a_silent_server_fails_in_time() {
        let (addr, _hold) = silent_server();
        let socket = connect_tcp(&addr.ip().to_string(), addr.port()).unwrap();
        let mut cli = RustFbClient {
            conn: Some(FirebirdWireConnection::test_over_socket(
                socket,
                ProtocolVersion::V13,
            )),
            charset: rsfbclient_core::charset::UTF_8,
        };
        assert!(cli
            .set_io_timeout(Some(Duration::from_millis(200)))
            .unwrap());
        let t = Instant::now();
        let r = cli.database_info(&mut DbHandle(0), &[1]);
        assert!(r.is_err(), "a silent server can't answer");
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "took {:?}",
            t.elapsed()
        );
    }

    #[test]
    fn a_client_without_a_connection_says_it_cannot() {
        let mut cli = RustFbClient {
            conn: None,
            charset: rsfbclient_core::charset::UTF_8,
        };
        assert!(!cli.set_io_timeout(Some(Duration::from_secs(1))).unwrap());
    }

    #[test]
    fn connecting_to_a_closed_port_fails_instead_of_hanging() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(connect_tcp("127.0.0.1", port).is_err());
    }
}
