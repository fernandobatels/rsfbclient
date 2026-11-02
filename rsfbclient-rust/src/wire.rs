//! Structs and functions to write and parse the firebird wire protocol messages

#![allow(non_upper_case_globals)]

use bytes::{BufMut, Bytes, BytesMut};
use std::{borrow::Cow, convert::TryFrom, str, sync::Arc};

use crate::{
    client::BlobId,
    consts::{gds_to_msg, AuthPluginType, Cnct, ProtocolVersion, WireOp, P_REQ_ASYNC},
    raw::RawValue,
    srp::*,
    util::*,
    xsqlda::{XSqlVar, XSQLDA_DESCRIBE_VARS},
};
use rsfbclient_core::{ibase, Charset, Column, Dialect, FbError, FreeStmtOp, SqlType, TrOp};

/// Buffer length to use in the connection
pub const BUFFER_LENGTH: u32 = 1024;

/// Size of the info buffer offered to `op_prepare_statement` / `op_info_sql`
/// (`p_sqlst_buffer_length` / `p_info_buffer_length`, both XDR longs). The
/// old 1024 made the describe of any wide table come back *truncated*, and
/// every continuation costs a full round trip; 64 KiB fits the describe of
/// hundreds of columns in the prepare response itself.
pub const INFO_BUFFER_LENGTH: u32 = 65535;

/// Buffer requested per `op_get_segment` (`p_sgmt_length`, a USHORT on the
/// wire, so this is the protocol maximum). The server packs as many segments
/// as fit, each framed `u16le length + bytes`, so one round trip returns up to
/// ~64 KiB of blob — the old 1024 meant a 20 KiB procedure source cost 20
/// round trips. Firebird 3/4/5 all honour 65535 (fast-firebird ships it).
pub const BLOB_SEGMENT_BUFFER: u32 = 65535;

/// Connection request.
///
/// `operation` is what the connection is for — `op_attach` / `op_create`
/// for a database, `op_service_attach` for the service manager — and
/// `db_name` the database path (or `service_mgr`). `max_protocol` caps the
/// protocol versions offered (see [`ProtocolVersion::OFFERED`]); the server
/// answers with the highest offered version it supports.
pub fn connect(
    operation: WireOp,
    db_name: &str,
    user: &str,
    username: &str,
    hostname: &str,
    srp_key: &[u8],
    max_protocol: u16,
) -> Bytes {
    // PROTOCOL_VERSION, Arch type (Generic=1), min type, max type (5 =
    // ptype_lazy_send), weight (preference, higher wins among the supported)
    let protocols: Vec<[u32; 5]> = ProtocolVersion::OFFERED
        .iter()
        .filter(|v| v.number() <= max_protocol)
        .enumerate()
        .map(|(i, v)| [*v as u32, 1, 0, 5, 2 * (i as u32 + 1)])
        .collect();

    let mut connect = BytesMut::with_capacity(256);

    connect.put_u32(WireOp::Connect as u32);
    connect.put_u32(operation as u32);
    connect.put_u32(3); // CONNECT_VERSION
    connect.put_u32(1); // arch_generic

    // Db file path / name
    connect.put_wire_bytes(db_name.as_bytes());

    // Protocol versions understood
    connect.put_u32(protocols.len() as u32);

    // Request SRP by default, so use Sha1
    let srp = SrpClient::<sha1::Sha1>::new(srp_key, &SRP_GROUP);

    let uid = {
        let mut uid = BytesMut::new();

        let pubkey = hex::encode(srp.get_a_pub());

        // Database username
        uid.put_u8(Cnct::Login as u8);
        uid.put_u8(user.len() as u8);
        uid.put(user.as_bytes());

        // Request the strongest plugin we implement; the server falls back to the
        // next one in the plugin list below if it does not support it
        let plugin = AuthPluginType::Srp256.name();

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

    connect.freeze()
}

/// Continue authentication request
pub fn cont_auth(data: &[u8], plugin: AuthPluginType, plugin_list: String, keys: &[u8]) -> Bytes {
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
pub fn crypt(algo: &str, kind: &str) -> Bytes {
    let mut req = BytesMut::with_capacity(12 + algo.len() + kind.len());

    req.put_u32(WireOp::Crypt as u32);
    // Encryption algorithm
    req.put_wire_bytes(algo.as_bytes());
    // Encryption type
    req.put_wire_bytes(kind.as_bytes());

    req.freeze()
}

/// Attach request
pub fn attach(
    db_name: &str,
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    charset: Charset,
    role_name: Option<&str>,
    dialect: Dialect,
    no_db_triggers: bool,
    auth_plugin: Option<&AuthPlugin>,
    srp_key: &[u8; 32],
) -> Result<Bytes, FbError> {
    let dpb = build_dpb(
        user,
        pass,
        protocol,
        charset,
        None,
        role_name,
        dialect,
        no_db_triggers,
        auth_plugin,
        srp_key,
    )?;

    let mut attach = BytesMut::with_capacity(16 + db_name.len() + dpb.len());

    attach.put_u32(WireOp::Attach as u32);
    attach.put_u32(0); // Database Object ID

    attach.put_wire_bytes(db_name.as_bytes());

    attach.put_wire_bytes(&dpb);

    Ok(attach.freeze())
}

/// Create db request
pub fn create(
    db_name: &str,
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    charset: Charset,
    page_size: Option<u32>,
    role_name: Option<&str>,
    dialect: Dialect,
    auth_plugin: Option<&AuthPlugin>,
    srp_key: &[u8; 32],
) -> Result<Bytes, FbError> {
    let dpb = build_dpb(
        user,
        pass,
        protocol,
        charset,
        page_size,
        role_name,
        dialect,
        false,
        auth_plugin,
        srp_key,
    )?;

    let mut create = BytesMut::with_capacity(16 + db_name.len() + dpb.len());

    create.put_u32(WireOp::Create as u32);
    create.put_u32(0); // Database Object ID

    create.put_wire_bytes(db_name.as_bytes());

    create.put_wire_bytes(&dpb);

    Ok(create.freeze())
}

/// Dpb builder
fn build_dpb(
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    charset: Charset,
    page_size: Option<u32>,
    role_name: Option<&str>,
    dialect: Dialect,
    no_db_triggers: bool,
    auth_plugin: Option<&AuthPlugin>,
    srp_key: &[u8; 32],
) -> Result<Bytes, FbError> {
    let mut params = Vec::new();

    if let Some(ps) = page_size {
        params.push(DpbData {
            tag: ibase::isc_dpb_page_size as u8,
            data: ps.to_be_bytes().to_vec().into(),
        });
    }

    let charset = charset.on_firebird.as_bytes();

    params.push(DpbData {
        tag: ibase::isc_dpb_lc_ctype as u8,
        data: charset.into(),
    });

    params.push(DpbData {
        tag: ibase::isc_dpb_user_name as u8,
        data: user.as_bytes().into(),
    });

    if let Some(role) = role_name {
        params.push(DpbData {
            tag: ibase::isc_dpb_sql_role_name as u8,
            data: role.as_bytes().into(),
        });
    }

    params.push(DpbData {
        tag: ibase::isc_dpb_sql_dialect as u8,
        data: vec![dialect as u8].into(),
    });

    if no_db_triggers {
        params.push(DpbData {
            tag: ibase::isc_dpb_no_db_triggers as u8,
            data: vec![1].into(),
        });
    }

    match protocol {
        // Plaintext password
        ProtocolVersion::V10 => {
            params.push(DpbData {
                tag: ibase::isc_dpb_password as u8,
                data: pass.as_bytes().to_vec().into(),
            });
        }

        // Hashed password
        ProtocolVersion::V11 | ProtocolVersion::V12 => {
            #[allow(deprecated)]
            let enc_pass = pwhash::unix_crypt::hash_with("9z", pass).unwrap();
            let enc_pass = &enc_pass[2..];

            params.push(DpbData {
                tag: ibase::isc_dpb_password_enc as u8,
                data: enc_pass.as_bytes().to_vec().into(),
            });
        }

        // Protocol 13+ (auth plugins). The DPB carries the SRP proof when the
        // password was not verified on cont_auth (WireCrypt = Disabled);
        // otherwise it was already verified and nothing is added. Protocols
        // 14-19 attach exactly like 13.
        _ => {
            if let Some(auth) = auth_plugin {
                if let Some(auth_data) = &auth.data {
                    params.push(DpbData {
                        tag: ibase::isc_dpb_auth_plugin_name as u8,
                        data: auth.kind.name().as_bytes().into(),
                    });

                    params.push(DpbData {
                        tag: ibase::isc_dpb_auth_plugin_list as u8,
                        data: AuthPluginType::plugin_list().into_bytes().into(),
                    });

                    let proof = {
                        match auth.kind {
                            AuthPluginType::Srp => {
                                let srp = SrpClient::<sha1::Sha1>::new(srp_key, &SRP_GROUP);
                                let verifier =
                                    crate::client::srp_verifier(srp, user, pass, auth_data)?;

                                hex::encode(verifier.get_proof())
                            }
                            AuthPluginType::Srp256 => {
                                let srp = SrpClient::<sha2::Sha256>::new(srp_key, &SRP_GROUP);
                                let verifier =
                                    crate::client::srp_verifier(srp, user, pass, auth_data)?;

                                hex::encode(verifier.get_proof())
                            }
                        }
                    };

                    params.push(DpbData {
                        tag: ibase::isc_dpb_specific_auth_data as u8,
                        data: proof.into_bytes().into(),
                    });
                }
            }
        }
    }

    Ok(dpb(&params))
}

struct DpbData<'a> {
    tag: u8,
    data: Cow<'a, [u8]>,
}

fn dpb(data: &[DpbData<'_>]) -> Bytes {
    let max_len = data.iter().map(|d| d.data.len()).max().unwrap_or_default();

    let mut dpb = BytesMut::with_capacity(64);

    let v2 = max_len > 255;

    dpb.put_u8(if v2 {
        ibase::isc_dpb_version2
    } else {
        ibase::isc_dpb_version1
    } as u8); //Version

    for d in data {
        dpb.put_u8(d.tag);
        if v2 {
            dpb.put_u32_le(d.data.len() as u32);
        } else {
            dpb.put_u8(d.data.len() as u8);
        }
        dpb.put_slice(&d.data);
    }

    dpb.freeze()
}

/// Detach from the database request
pub fn detach(db_handle: u32) -> Bytes {
    let mut tr = BytesMut::with_capacity(8);

    tr.put_u32(WireOp::Detach as u32);
    tr.put_u32(db_handle);

    tr.freeze()
}

/// Drop database request
pub fn drop_database(db_handle: u32) -> Bytes {
    let mut tr = BytesMut::with_capacity(8);

    tr.put_u32(WireOp::DropDatabase as u32);
    tr.put_u32(db_handle);

    tr.freeze()
}

/// Name of the service manager endpoint, the "database" a service
/// connection attaches to.
pub const SERVICE_MGR: &str = "service_mgr";

/// Service manager attach request (`op_service_attach`): like `op_attach`,
/// but the "database" is `service_mgr` and the parameter block an SPB.
pub fn service_attach(
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    expected_db: Option<&str>,
    auth_plugin: Option<&AuthPlugin>,
    srp_key: &[u8; 32],
) -> Result<Bytes, FbError> {
    let spb = build_spb(user, pass, protocol, expected_db, auth_plugin, srp_key)?;

    let mut req = BytesMut::with_capacity(16 + SERVICE_MGR.len() + spb.len());

    req.put_u32(WireOp::ServiceAttach as u32);
    req.put_u32(0); // Object ID

    req.put_wire_bytes(SERVICE_MGR.as_bytes());

    req.put_wire_bytes(&spb);

    Ok(req.freeze())
}

/// Service manager detach request
pub fn service_detach(svc_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(8);

    req.put_u32(WireOp::ServiceDetach as u32);
    req.put_u32(svc_handle);

    req.freeze()
}

/// Start a service action (`op_service_start`): `request` is the action
/// byte followed by its arguments (see `rsfbclient_core::services`).
pub fn service_start(svc_handle: u32, request: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(16 + request.len());

    req.put_u32(WireOp::ServiceStart as u32);
    req.put_u32(svc_handle);
    req.put_u32(0); // Incarnation of object
    req.put_wire_bytes(request);

    req.freeze()
}

/// Service information request (`op_service_info`). `send_items` are the
/// `isc_service_query` send items (usually empty), `receive_items` what
/// to read back (`isc_info_svc_line`, `isc_info_svc_server_version`, …);
/// the server answers with an `op_response` whose data is the info
/// buffer, at most `buffer_length` bytes.
pub fn service_info(svc_handle: u32, send_items: &[u8], receive_items: &[u8], buffer_length: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(24 + send_items.len() + receive_items.len());

    req.put_u32(WireOp::ServiceInfo as u32);
    req.put_u32(svc_handle);
    req.put_u32(0); // Incarnation of object
    req.put_wire_bytes(send_items);
    req.put_wire_bytes(receive_items);
    req.put_u32(buffer_length);

    req.freeze()
}

/// Attach SPB builder (version 2: a one-byte length per argument).
///
/// The credentials follow the same rules as the DPB: a plaintext password
/// on protocol 10, a hashed one on 11/12, and on 13+ the SRP proof (when
/// the password was not already verified on `op_cont_auth`). The service
/// manager accepts exactly the `isc_spb_*` twins of the `isc_dpb_*` auth
/// tags, so the same handshake state drives both.
fn build_spb(
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    expected_db: Option<&str>,
    auth_plugin: Option<&AuthPlugin>,
    srp_key: &[u8; 32],
) -> Result<Bytes, FbError> {
    use rsfbclient_core::services::push_attach_arg;

    let mut spb = vec![
        ibase::isc_spb_version as u8,
        ibase::isc_spb_current_version as u8,
    ];

    push_attach_arg(&mut spb, ibase::isc_spb_user_name as u8, user.as_bytes());

    if let Some(db) = expected_db {
        push_attach_arg(&mut spb, ibase::isc_spb_expected_db as u8, db.as_bytes());
    }

    match protocol {
        ProtocolVersion::V10 => {
            push_attach_arg(&mut spb, ibase::isc_spb_password as u8, pass.as_bytes());
        }

        ProtocolVersion::V11 | ProtocolVersion::V12 => {
            #[allow(deprecated)]
            let enc_pass = pwhash::unix_crypt::hash_with("9z", pass).unwrap();
            push_attach_arg(&mut spb, ibase::isc_spb_password_enc as u8, enc_pass[2..].as_bytes());
        }

        _ => {
            if let Some(auth) = auth_plugin {
                if let Some(auth_data) = &auth.data {
                    push_attach_arg(&mut spb, ibase::isc_spb_auth_plugin_name as u8, auth.kind.name().as_bytes());
                    push_attach_arg(
                        &mut spb,
                        ibase::isc_spb_auth_plugin_list as u8,
                        AuthPluginType::plugin_list().as_bytes(),
                    );

                    let proof = match auth.kind {
                        AuthPluginType::Srp => {
                            let srp = SrpClient::<sha1::Sha1>::new(srp_key, &SRP_GROUP);
                            let verifier = crate::client::srp_verifier(srp, user, pass, auth_data)?;
                            hex::encode(verifier.get_proof())
                        }
                        AuthPluginType::Srp256 => {
                            let srp = SrpClient::<sha2::Sha256>::new(srp_key, &SRP_GROUP);
                            let verifier = crate::client::srp_verifier(srp, user, pass, auth_data)?;
                            hex::encode(verifier.get_proof())
                        }
                    };

                    push_attach_arg(&mut spb, ibase::isc_spb_specific_auth_data as u8, proof.as_bytes());
                }
            }
        }
    }

    Ok(Bytes::from(spb))
}

/// Begin transaction request
pub fn transaction(db_handle: u32, tpb: &[u8]) -> Bytes {
    let mut tr = BytesMut::with_capacity(12 + tpb.len());

    tr.put_u32(WireOp::Transaction as u32);
    tr.put_u32(db_handle);
    tr.put_wire_bytes(tpb);

    tr.freeze()
}

/// Commit / Rollback transaction request
pub fn transaction_operation(tr_handle: u32, op: TrOp) -> Bytes {
    let mut tr = BytesMut::with_capacity(8);

    let op = match op {
        TrOp::Commit => WireOp::Commit,
        TrOp::CommitRetaining => WireOp::CommitRetaining,
        TrOp::Rollback => WireOp::Rollback,
        TrOp::RollbackRetaining => WireOp::RollbackRetaining,
    };

    tr.put_u32(op as u32);
    tr.put_u32(tr_handle);

    tr.freeze()
}

/// Execute immediate request
pub fn exec_immediate(
    tr_handle: u32,
    dialect: u32,
    sql: &str,
    charset: &Charset,
) -> Result<Bytes, FbError> {
    let bytes = charset.encode(sql)?;
    let mut req = BytesMut::with_capacity(28 + bytes.len());

    req.put_u32(WireOp::ExecImmediate as u32);
    req.put_u32(tr_handle);
    req.put_u32(0); // Statement handle, apparently unused
    req.put_u32(dialect);
    req.put_wire_bytes(&bytes);
    req.put_u32(0); // TODO: parameters
    req.put_u32(BUFFER_LENGTH);

    Ok(req.freeze())
}

/// Statement allocation request (lazy response)
pub fn allocate_statement(db_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(8);

    req.put_u32(WireOp::AllocateStatement as u32);
    req.put_u32(db_handle);

    req.freeze()
}

/// Prepare statement request. Use u32::MAX as `stmt_handle` if the statement was allocated
/// in the previous request
pub fn prepare_statement(
    tr_handle: u32,
    stmt_handle: u32,
    dialect: u32,
    query: &str,
    charset: &Charset,
) -> Result<Bytes, FbError> {
    let bytes = charset.encode(query)?;
    let mut req = BytesMut::with_capacity(28 + bytes.len() + XSQLDA_DESCRIBE_VARS.len());

    req.put_u32(WireOp::PrepareStatement as u32);
    req.put_u32(tr_handle);
    req.put_u32(stmt_handle);
    req.put_u32(dialect);
    req.put_wire_bytes(&bytes);
    req.put_wire_bytes(&XSQLDA_DESCRIBE_VARS); // Data to be returned

    req.put_u32(INFO_BUFFER_LENGTH);

    Ok(req.freeze())
}

/// Statement information request
pub fn info_sql(stmt_handle: u32, requested_items: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(24 + requested_items.len());

    req.put_u32(WireOp::InfoSql as u32);
    req.put_u32(stmt_handle);
    req.put_u32(0); // Incarnation of object
    req.put_wire_bytes(requested_items);
    req.put_u32(INFO_BUFFER_LENGTH);

    req.freeze()
}

/// Close or drop statement request
pub fn free_statement(stmt_handle: u32, op: FreeStmtOp) -> Bytes {
    let mut req = BytesMut::with_capacity(12);

    req.put_u32(WireOp::FreeStatement as u32);
    req.put_u32(stmt_handle);
    req.put_u32(op as u32);

    req.freeze()
}

/// Trailing, version-gated fields of `op_execute` / `op_execute2`
/// (`P_SQLDATA` in protocol.cpp): a server at protocol >= 16 *blocks* reading
/// them, and one below rejects them, so they must follow the negotiated
/// version exactly.
fn put_execute_tail(req: &mut BytesMut, version: ProtocolVersion, inline_blob_size: u32) {
    if version >= ProtocolVersion::V16 {
        req.put_u32(0); // p_sqldata_timeout: 0 = no statement timeout
    }
    if version >= ProtocolVersion::V18 {
        req.put_u32(0); // p_sqldata_cursor_flags: none (no scrollable cursor)
    }
    if version >= ProtocolVersion::V19 {
        // p_sqldata_inline_blob_size: blobs up to this many bytes (data plus
        // 2-byte segment framing) arrive as op_inline_blob packets right
        // before the row that references them, instead of costing
        // open/get_segment/close round trips each. 0 turns inlining off.
        req.put_u32(inline_blob_size);
    }
}

/// Execute prepared statement request.
pub fn execute(
    tr_handle: u32,
    stmt_handle: u32,
    input_blr: &[u8],
    input_data: &[u8],
    version: ProtocolVersion,
    inline_blob_size: u32,
) -> Bytes {
    let mut req = BytesMut::with_capacity(48 + input_blr.len() + input_data.len());

    req.put_u32(WireOp::Execute as u32);
    req.put_u32(stmt_handle);
    req.put_u32(tr_handle);

    req.put_wire_bytes(input_blr);
    req.put_u32(0);
    req.put_u32(if input_blr.is_empty() { 0 } else { 1 });

    req.put_slice(input_data);

    put_execute_tail(&mut req, version, inline_blob_size);

    req.freeze()
}

/// Execute prepared statement request.
pub fn execute2(
    tr_handle: u32,
    stmt_handle: u32,
    input_blr: &[u8],
    input_data: &[u8],
    output_blr: &[u8],
    version: ProtocolVersion,
    inline_blob_size: u32,
) -> Bytes {
    let mut req =
        BytesMut::with_capacity(52 + input_blr.len() + input_data.len() + output_blr.len());

    req.put_u32(WireOp::Execute2 as u32);
    req.put_u32(stmt_handle);
    req.put_u32(tr_handle);

    req.put_wire_bytes(input_blr);
    req.put_u32(0); // Input message number
    req.put_u32(if input_blr.is_empty() { 0 } else { 1 }); // Messages

    req.put_slice(input_data);

    req.put_wire_bytes(output_blr);
    req.put_u32(0); // Output message number

    put_execute_tail(&mut req, version, inline_blob_size);

    req.freeze()
}

/// Fetch row request
pub fn fetch(stmt_handle: u32, blr: &[u8], count: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(20 + blr.len());

    req.put_u32(WireOp::Fetch as u32);
    req.put_u32(stmt_handle);
    req.put_wire_bytes(blr);
    req.put_u32(0); // Message number
    req.put_u32(count); // Message count: request a batch of rows in a single round-trip

    req.freeze()
}

/// Auxiliary channel request.
///
/// Firebird does not push the event notifications on the connection that
/// registered them: it asks the client to open a second, "async" connection
/// and answers this request with the address it listens on for it.
pub fn connect_request(db_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(16);

    req.put_u32(WireOp::ConnectRequest as u32);
    req.put_u32(P_REQ_ASYNC); // Connection type
    req.put_u32(db_handle); // Related object
    req.put_u32(0); // Partner identification

    req.freeze()
}

/// Event notification request
pub fn que_events(db_handle: u32, epb: &[u8], event_id: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(24 + epb.len());

    req.put_u32(WireOp::QueEvents as u32);
    req.put_u32(db_handle);
    req.put_wire_bytes(epb); // Event parameter block
    req.put_u32(0); // Ast routine address, unused by the remote protocol
    req.put_u32(0); // Ast routine argument, unused by the remote protocol
    req.put_u32(event_id);

    req.freeze()
}

/// Cancel an event notification request
pub fn cancel_events(db_handle: u32, event_id: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(12);

    req.put_u32(WireOp::CancelEvents as u32);
    req.put_u32(db_handle);
    req.put_u32(event_id);

    req.freeze()
}

#[derive(Debug)]
/// `WireOp::Event` notification, pushed by the server on the auxiliary channel
pub struct EventNotification {
    /// Id of the `WireOp::QueEvents` registration this notification answers
    pub event_id: u32,
    /// Event parameter block holding the updated occurrence counters
    pub epb: Bytes,
}

/// Parse an event notification (`WireOp::Event`), op code included
pub fn parse_event_notification(resp: &mut Bytes) -> Result<EventNotification, FbError> {
    let op_code = resp.get_u32()?;

    if op_code != WireOp::Event as u32 {
        return err_conn_rejected(op_code);
    }

    resp.get_u32()?; // Database handle
    let epb = resp.get_wire_bytes()?;
    resp.get_u32()?; // Ast routine address, always zero
    resp.get_u32()?; // Ast routine argument, always zero
    let event_id = resp.get_u32()?;

    Ok(EventNotification { event_id, epb })
}

/// Extract the port of the auxiliary channel from the `WireOp::ConnectRequest`
/// response data, which holds a raw `sockaddr` of the server.
///
/// Only the port is used: the address the server reports is the one it sees
/// locally, which is wrong as soon as it sits behind a NAT, so the reference
/// client reuses the address of the main connection instead. This is what
/// `aux_connect` does in the firebird sources. The port lives at offset 2 in
/// network byte order for both `sockaddr_in` and `sockaddr_in6`, whatever the
/// sockaddr layout of the server.
pub fn parse_aux_port(data: &[u8]) -> Result<u16, FbError> {
    if data.len() < 4 {
        return Err(FbError::from(
            "Invalid auxiliary channel address returned by the server",
        ));
    }

    let port = u16::from_be_bytes([data[2], data[3]]);
    if port == 0 {
        return Err(FbError::from(
            "The server did not return a port for the auxiliary event channel",
        ));
    }

    Ok(port)
}

/// Create blob request
pub fn create_blob(tr_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(16);

    req.put_u32(WireOp::CreateBlob as u32);
    req.put_u32(tr_handle);
    req.put_u64(0); // Blob id, but we are creating one!?

    req.freeze()
}

/// Open blob request
pub fn open_blob(tr_handle: u32, blob_id: u64) -> Bytes {
    let mut req = BytesMut::with_capacity(16);

    req.put_u32(WireOp::OpenBlob as u32);
    req.put_u32(tr_handle);
    req.put_u64(blob_id);

    req.freeze()
}

/// Get blob segment request. `buffer_length` is how many bytes of framed
/// segments the server may pack into the response (max [`BLOB_SEGMENT_BUFFER`]).
pub fn get_segment(blob_handle: u32, buffer_length: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(16);

    req.put_u32(WireOp::GetSegment as u32);
    req.put_u32(blob_handle);
    req.put_u32(buffer_length.min(BLOB_SEGMENT_BUFFER));
    req.put_u32(0); // Data segment, apparently unused

    req.freeze()
}

/// Unpacks the segment framing of blob data as the server ships it in an
/// `op_get_segment` response and in an `op_inline_blob` packet: a run of
/// `u16le length + bytes`.
pub fn unframe_segments(mut data: Bytes) -> Result<Bytes, FbError> {
    // A single segment (the usual case) is handed back without copying.
    if data.remaining() >= 2 {
        let len = u16::from_le_bytes([data[0], data[1]]) as usize;
        if data.remaining() == 2 + len {
            data.advance(2)?;
            return Ok(data);
        }
    }

    let mut blob_data = BytesMut::with_capacity(data.remaining());

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

    Ok(blob_data.freeze())
}

#[derive(Debug, Clone)]
/// A blob the server inlined (`WireOp::InlineBlob`, protocol 19+). The server
/// sends these right before the `op_fetch_response` / `op_sql_response` whose
/// row references them, for every non-NULL blob column whose framed data fits
/// the size announced in `op_execute`.
pub struct InlineBlob {
    /// Blob id, as it appears in the row
    pub id: u64,
    /// Blob content, segment framing removed
    pub data: Bytes,
}

/// Parse the body of an `op_inline_blob` packet (op code already consumed):
/// `p_tran_id` (XDR short), `p_blob_id` (quad), `p_blob_info` (counted
/// bytes: the isc_info_blob_* clumplets, not needed here), `p_blob_data`
/// (counted bytes, segment framed).
pub fn parse_inline_blob(resp: &mut Bytes) -> Result<InlineBlob, FbError> {
    let _tr_handle = resp.get_u32()?; // ids are unique per connection, tx not needed
    let id = resp.get_u64()?;
    let _info = resp.get_wire_bytes()?;
    let data = unframe_segments(resp.get_wire_bytes()?)?;

    Ok(InlineBlob { id, data })
}

/// Put blob segment request
pub fn put_segment(blob_handle: u32, segment: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(8 + segment.len());

    req.put_u32(WireOp::PutSegment as u32);
    req.put_u32(blob_handle);
    req.put_u32(segment.len() as u32);
    req.put_wire_bytes(segment);

    req.freeze()
}

/// Close blob segment request
pub fn close_blob(blob_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(8);

    req.put_u32(WireOp::CloseBlob as u32);
    req.put_u32(blob_handle);

    req.freeze()
}

#[derive(Debug)]
/// `WireOp::Response` response
pub struct Response {
    pub handle: u32,
    pub object_id: u64,
    pub data: Bytes,
}

/// Parse a server response (`WireOp::Response`)
pub fn parse_response(resp: &mut Bytes) -> Result<Response, FbError> {
    let handle = resp.get_u32()?;
    let object_id = resp.get_u64()?;

    let data = resp.get_wire_bytes()?;

    parse_status_vector(resp)?;

    Ok(Response {
        handle,
        object_id,
        data,
    })
}

/// Parse a server sql response (`WireOp::FetchResponse`)
pub fn parse_fetch_response(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
    inline: &[InlineBlob],
) -> Result<Option<Vec<ParsedColumn>>, FbError> {
    const END_OF_STREAM: u32 = 100;

    let status = resp.get_u32()?;

    if status == END_OF_STREAM {
        return Ok(None);
    }

    Ok(Some(parse_sql_response(
        resp, xsqlda, version, charset, inline,
    )?))
}

/// Like [`parse_fetch_response`] but decodes straight into `Vec<Column>`. Only
/// valid when the statement has no blob columns (caller checks `has_blob`).
pub fn parse_fetch_response_columns(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
) -> Result<Option<Vec<Column>>, FbError> {
    const END_OF_STREAM: u32 = 100;

    let status = resp.get_u32()?;

    if status == END_OF_STREAM {
        return Ok(None);
    }

    Ok(Some(parse_sql_response_columns(
        resp, xsqlda, version, charset,
    )?))
}

/// Parse a server sql response (`WireOp::SqlResponse`)
/// Identical to the FetchResponse, but has no status.
///
/// `inline` holds the blobs the server inlined ahead of this row (protocol
/// 19+); a blob column whose id is among them is decoded on the spot instead
/// of being left for the open/get_segment/close round trips.
pub fn parse_sql_response(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
    inline: &[InlineBlob],
) -> Result<Vec<ParsedColumn>, FbError> {
    let mut data = Vec::with_capacity(xsqlda.len());
    decode_row(
        resp,
        xsqlda,
        version,
        &mut ParsedColumnSink {
            charset,
            inline,
            out: &mut data,
        },
    )?;
    Ok(data)
}

/// Like [`parse_sql_response`] but decodes straight into `Vec<Column>`, skipping
/// the intermediate `Vec<ParsedColumn>` and the per-column move loop. Only valid
/// when the statement has no blob columns: a blob here is an error, since
/// resolving it needs a connection round-trip this function does not have.
pub fn parse_sql_response_columns(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
) -> Result<Vec<Column>, FbError> {
    let mut cols = Vec::with_capacity(xsqlda.len());
    decode_row(
        resp,
        xsqlda,
        version,
        &mut ColumnSink {
            charset,
            out: &mut cols,
        },
    )?;
    Ok(cols)
}

/// Like [`parse_fetch_response`] but decodes into `out` as [`RawValue`], with no
/// charset decode and no per-column allocation: text is a `Bytes` handle onto the
/// read buffer. `out` is reused across rows, so a whole result set streams with no
/// per-row allocation. Returns `Ok(false)` at end of cursor. Blob columns are
/// rejected (the caller checks `has_blob`).
pub fn parse_fetch_response_raw(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    out: &mut Vec<RawValue>,
) -> Result<bool, FbError> {
    const END_OF_STREAM: u32 = 100;

    let status = resp.get_u32()?;

    if status == END_OF_STREAM {
        return Ok(false);
    }

    parse_sql_response_raw(resp, xsqlda, version, out)?;

    Ok(true)
}

/// Body of [`parse_fetch_response_raw`] (after the status word).
pub fn parse_sql_response_raw(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    out: &mut Vec<RawValue>,
) -> Result<(), FbError> {
    out.clear();

    decode_row(resp, xsqlda, version, &mut RawSink { out })
}

/// Destination for the columns of one decoded row. The wire layout is decoded in
/// exactly one place ([`decode_row`]); the sink decides what a column becomes --
/// a [`ParsedColumn`] (blobs deferred), a [`Column`] directly, or a zero-copy
/// [`RawValue`].
trait RowSink {
    fn null(&mut self, var: &XSqlVar, sqltype: u32) -> Result<(), FbError>;

    /// Text still in the connection charset, borrowed from the read buffer.
    fn text(&mut self, var: &XSqlVar, sqltype: u32, data: Bytes) -> Result<(), FbError>;

    fn integer(&mut self, var: &XSqlVar, sqltype: u32, value: i64) -> Result<(), FbError>;

    fn int128(&mut self, var: &XSqlVar, sqltype: u32, value: i128) -> Result<(), FbError>;

    fn floating(&mut self, var: &XSqlVar, sqltype: u32, value: f64) -> Result<(), FbError>;

    fn timestamp(
        &mut self,
        var: &XSqlVar,
        sqltype: u32,
        ts: ibase::ISC_TIMESTAMP,
    ) -> Result<(), FbError>;

    fn boolean(&mut self, var: &XSqlVar, sqltype: u32, value: bool) -> Result<(), FbError>;

    /// Only the blob id is on the wire; the data needs its own round-trips, so
    /// sinks that cannot issue them reject the column.
    fn blob(&mut self, var: &XSqlVar, sqltype: u32, id: BlobId) -> Result<(), FbError>;
}

/// Sink that collects [`ParsedColumn`], leaving blobs to be resolved later.
struct ParsedColumnSink<'a> {
    charset: &'a Charset,
    /// Blobs inlined by the server for this row (protocol 19+)
    inline: &'a [InlineBlob],
    out: &'a mut Vec<ParsedColumn>,
}

impl ParsedColumnSink<'_> {
    fn push(&mut self, var: &XSqlVar, sqltype: u32, value: SqlType) {
        self.out.push(ParsedColumn::Complete(Column::new(
            var.alias_name.clone(),
            sqltype,
            value,
        )));
    }
}

impl RowSink for ParsedColumnSink<'_> {
    fn null(&mut self, var: &XSqlVar, sqltype: u32) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Null);
        Ok(())
    }

    fn text(&mut self, var: &XSqlVar, sqltype: u32, data: Bytes) -> Result<(), FbError> {
        let text = self.charset.decode(&data[..])?;
        self.push(var, sqltype, SqlType::Text(text));
        Ok(())
    }

    fn integer(&mut self, var: &XSqlVar, sqltype: u32, value: i64) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Integer(value));
        Ok(())
    }

    fn int128(&mut self, var: &XSqlVar, sqltype: u32, value: i128) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Int128(value));
        Ok(())
    }

    fn floating(&mut self, var: &XSqlVar, sqltype: u32, value: f64) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Floating(value));
        Ok(())
    }

    fn timestamp(
        &mut self,
        var: &XSqlVar,
        sqltype: u32,
        ts: ibase::ISC_TIMESTAMP,
    ) -> Result<(), FbError> {
        self.push(
            var,
            sqltype,
            SqlType::Timestamp(rsfbclient_core::date_time::decode_timestamp(ts)),
        );
        Ok(())
    }

    fn boolean(&mut self, var: &XSqlVar, sqltype: u32, value: bool) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Boolean(value));
        Ok(())
    }

    fn blob(&mut self, var: &XSqlVar, sqltype: u32, id: BlobId) -> Result<(), FbError> {
        let binary = var.sqlsubtype == 0;

        // Inlined by the server: complete right here, zero extra round trips.
        if let Some(inlined) = self.inline.iter().find(|b| b.id == id.0) {
            let value = if binary {
                SqlType::Binary(inlined.data.to_vec())
            } else {
                SqlType::Text(self.charset.decode(&inlined.data[..])?)
            };
            self.push(var, sqltype, value);
            return Ok(());
        }

        self.out.push(ParsedColumn::Blob {
            binary,
            id,
            col_name: var.alias_name.clone(),
        });
        Ok(())
    }
}

/// Sink that builds [`Column`] directly, for statements with no blob columns.
struct ColumnSink<'a> {
    charset: &'a Charset,
    out: &'a mut Vec<Column>,
}

impl ColumnSink<'_> {
    fn push(&mut self, var: &XSqlVar, sqltype: u32, value: SqlType) {
        self.out
            .push(Column::new(var.alias_name.clone(), sqltype, value));
    }
}

impl RowSink for ColumnSink<'_> {
    fn null(&mut self, var: &XSqlVar, sqltype: u32) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Null);
        Ok(())
    }

    fn text(&mut self, var: &XSqlVar, sqltype: u32, data: Bytes) -> Result<(), FbError> {
        let text = self.charset.decode(&data[..])?;
        self.push(var, sqltype, SqlType::Text(text));
        Ok(())
    }

    fn integer(&mut self, var: &XSqlVar, sqltype: u32, value: i64) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Integer(value));
        Ok(())
    }

    fn int128(&mut self, var: &XSqlVar, sqltype: u32, value: i128) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Int128(value));
        Ok(())
    }

    fn floating(&mut self, var: &XSqlVar, sqltype: u32, value: f64) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Floating(value));
        Ok(())
    }

    fn timestamp(
        &mut self,
        var: &XSqlVar,
        sqltype: u32,
        ts: ibase::ISC_TIMESTAMP,
    ) -> Result<(), FbError> {
        self.push(
            var,
            sqltype,
            SqlType::Timestamp(rsfbclient_core::date_time::decode_timestamp(ts)),
        );
        Ok(())
    }

    fn boolean(&mut self, var: &XSqlVar, sqltype: u32, value: bool) -> Result<(), FbError> {
        self.push(var, sqltype, SqlType::Boolean(value));
        Ok(())
    }

    fn blob(&mut self, _var: &XSqlVar, _sqltype: u32, _id: BlobId) -> Result<(), FbError> {
        Err("parse_sql_response_columns called on a statement with blob columns".into())
    }
}

/// Sink that emits [`RawValue`]: no charset decode, no allocation per column --
/// text is a refcounted handle onto the read buffer.
struct RawSink<'a> {
    out: &'a mut Vec<RawValue>,
}

impl RowSink for RawSink<'_> {
    fn null(&mut self, _var: &XSqlVar, _sqltype: u32) -> Result<(), FbError> {
        self.out.push(RawValue::Null);
        Ok(())
    }

    fn text(&mut self, _var: &XSqlVar, _sqltype: u32, data: Bytes) -> Result<(), FbError> {
        self.out.push(RawValue::Text(data));
        Ok(())
    }

    fn integer(&mut self, _var: &XSqlVar, _sqltype: u32, value: i64) -> Result<(), FbError> {
        self.out.push(RawValue::Integer(value));
        Ok(())
    }

    fn int128(&mut self, _var: &XSqlVar, _sqltype: u32, value: i128) -> Result<(), FbError> {
        self.out.push(RawValue::Int128(value));
        Ok(())
    }

    fn floating(&mut self, _var: &XSqlVar, _sqltype: u32, value: f64) -> Result<(), FbError> {
        self.out.push(RawValue::Floating(value));
        Ok(())
    }

    fn timestamp(
        &mut self,
        _var: &XSqlVar,
        _sqltype: u32,
        ts: ibase::ISC_TIMESTAMP,
    ) -> Result<(), FbError> {
        self.out.push(RawValue::Timestamp(
            rsfbclient_core::date_time::decode_timestamp(ts),
        ));
        Ok(())
    }

    fn boolean(&mut self, _var: &XSqlVar, _sqltype: u32, value: bool) -> Result<(), FbError> {
        self.out.push(RawValue::Boolean(value));
        Ok(())
    }

    fn blob(&mut self, _var: &XSqlVar, _sqltype: u32, _id: BlobId) -> Result<(), FbError> {
        Err("raw streaming called on a statement with blob columns".into())
    }
}

/// Decodes the columns of one sql/fetch response body (after the op_code and,
/// for a fetch, the status word) into `sink`. The wire format lives here once;
/// the sink decides what each column becomes.
fn decode_row<S: RowSink>(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    sink: &mut S,
) -> Result<(), FbError> {
    let has_row = resp.get_u32()? != 0;
    if !has_row {
        return Err("Fetch returned no columns".into());
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
        resp.advance(len)?;

        Some(null_map)
    } else {
        None
    };

    let read_null = |resp: &mut Bytes, i: usize| {
        if version >= ProtocolVersion::V13 {
            // read from the null bitmap
            let null_map = null_map.as_ref().expect("Null map was not initialized");
            Ok::<_, FbError>((null_map[i / 8] >> (i % 8)) & 1 != 0)
        } else {
            // read from the response
            Ok(resp.get_u32()? != 0)
        }
    };

    for (col_index, var) in xsqlda.iter().enumerate() {
        // Remove nullable type indicator
        let sqltype = var.sqltype as u32 & (!1);

        if version >= ProtocolVersion::V13 && read_null(resp, col_index)? {
            // There is no data in protocol 13 if null, so just continue
            sink.null(var, sqltype)?;
            continue;
        }

        match sqltype {
            ibase::SQL_VARYING => {
                let d = resp.get_wire_bytes()?;

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.text(var, sqltype, d)?
                }
            }

            ibase::SQL_INT64 => {
                let i = resp.get_i64()?;

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.integer(var, sqltype, i)?
                }
            }

            ibase::SQL_INT128 => {
                let high = resp.get_i64()?;
                let low = resp.get_u64()?;
                let i = (i128::from(high) << 64) | i128::from(low);

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.int128(var, sqltype, i)?
                }
            }

            ibase::SQL_DOUBLE => {
                let f = resp.get_f64()?;

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.floating(var, sqltype, f)?
                }
            }

            ibase::SQL_TIMESTAMP => {
                let ts = ibase::ISC_TIMESTAMP {
                    timestamp_date: resp.get_i32()?,
                    timestamp_time: resp.get_u32()?,
                };

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.timestamp(var, sqltype, ts)?
                }
            }

            ibase::SQL_BLOB if var.sqlsubtype <= 1 => {
                let id = resp.get_u64()?;

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.blob(var, sqltype, BlobId(id))?
                }
            }

            ibase::SQL_BOOLEAN => {
                let b = resp.get_u8()? == 1;
                resp.advance(3)?; // Pad to 4 bytes

                if read_null(resp, col_index)? {
                    sink.null(var, sqltype)?
                } else {
                    sink.boolean(var, sqltype, b)?
                }
            }

            sqltype => {
                return Err(format!(
                    "Conversion from sql type {} (subtype {}) not implemented",
                    sqltype, var.sqlsubtype
                )
                .into());
            }
        }
    }

    Ok(())
}

/// Column data parsed from a fetch response
pub enum ParsedColumn {
    /// All data received
    Complete(Column),
    /// Blobs need more requests to get the actual data
    Blob {
        /// True if blob type 0
        binary: bool,
        /// Blob id
        id: BlobId,
        /// Column name
        col_name: Arc<str>,
    },
}

/// Parses the error messages from the response
pub fn parse_status_vector(resp: &mut Bytes) -> Result<(), FbError> {
    // Sql error code (default to -1)
    let mut sql_code = -1;
    // Error messages
    let mut message = String::new();

    // Code of the last error message
    let mut gds_code = 0;
    // Error message argument index
    let mut num_arg = 0;

    loop {
        match resp.get_u32()? {
            // New error message
            ibase::isc_arg_gds => {
                gds_code = resp.get_u32()?;

                if gds_code != 0 {
                    message += gds_to_msg(gds_code);
                    num_arg = 0;
                }
            }

            // New warning (e.g. from a DDL statement). Not surfaced as an error,
            // just consume its code so the vector stays aligned.
            ibase::isc_arg_warning => {
                resp.get_u32()?;
            }

            // Error message arg number
            ibase::isc_arg_number => {
                let num = resp.get_i32()?;
                // Sql error code
                if gds_code == 335544436 {
                    sql_code = num
                }

                num_arg += 1;
                message = message.replace(&format!("@{}", num_arg), &format!("{}", num));
            }

            // Error message arg string
            ibase::isc_arg_string => {
                let msg = resp.get_wire_bytes()?;
                let msg = std::str::from_utf8(&msg[..]).unwrap_or("**Invalid message**");

                num_arg += 1;
                message = message.replace(&format!("@{}", num_arg), msg);
            }

            // Aditional error message string
            ibase::isc_arg_interpreted => {
                let msg = resp.get_wire_bytes()?;
                let msg = std::str::from_utf8(&msg[..]).unwrap_or("**Invalid message**");

                message += msg;
            }

            ibase::isc_arg_sql_state => {
                resp.get_wire_bytes()?;
            }

            // End of error messages
            ibase::isc_arg_end => break,

            cod => {
                return Err(format!("Invalid / Unknown status vector item: {}", cod).into());
            }
        }
    }

    if message.ends_with('\n') {
        message.pop();
    }

    if !message.is_empty() {
        Err(FbError::Sql {
            code: sql_code,
            msg: message,
        })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
/// Data from the response of a connection request
pub struct ConnectionResponse {
    pub version: ProtocolVersion,
    pub auth_plugin: Option<AuthPlugin>,
    pub continue_auth: bool,
}

#[derive(Debug)]
pub struct AuthPlugin {
    pub kind: AuthPluginType,
    pub data: Option<SrpAuthData>,
    pub keys: Bytes,
}

/// Parse the connect response response (`WireOp::Accept`, `WireOp::AcceptData`, `WireOp::CondAccept` )
pub fn parse_accept(resp: &mut Bytes) -> Result<ConnectionResponse, FbError> {
    let op_code = resp.get_u32()?;

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

    let continue_auth = op_code == WireOp::CondAccept as u32;

    let version =
        ProtocolVersion::try_from(resp.get_u32()?).map_err(|e| FbError::Other(e.to_string()))?;
    resp.get_u32()?; // Arch
    resp.get_u32()?; // Type

    let auth_plugin =
        if op_code == WireOp::AcceptData as u32 || op_code == WireOp::CondAccept as u32 {
            let auth_data = parse_srp_auth_data(&mut resp.get_wire_bytes()?)?;

            let plugin = AuthPluginType::parse(&resp.get_wire_bytes()?)?;

            let authenticated = resp.get_u32()? != 0;

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
        continue_auth,
    })
}

/// Parse an authentication continuation response (`WireOp::ContAuth`)
pub fn parse_cont_auth(resp: &mut Bytes) -> Result<AuthPlugin, FbError> {
    let op_code = resp.get_u32()?;

    if op_code == WireOp::Response as u32 {
        // Returned an error
        parse_response(resp)?;
    }

    if op_code != WireOp::ContAuth as u32 {
        return err_conn_rejected(op_code);
    }

    let auth_data = parse_srp_auth_data(&mut resp.get_wire_bytes()?)?;
    let plugin = AuthPluginType::parse(&resp.get_wire_bytes()?)?;
    let _plugin_list = resp.get_wire_bytes()?;
    let keys = resp.get_wire_bytes()?;

    Ok(AuthPlugin {
        kind: plugin,
        data: auth_data,
        keys,
    })
}

#[derive(Debug, Clone)]
pub struct SrpAuthData {
    pub salt: Box<[u8]>,
    pub pub_key: Box<[u8]>,
}

/// Parse the auth data from the Srp / Srp256 plugin
pub fn parse_srp_auth_data(resp: &mut Bytes) -> Result<Option<SrpAuthData>, FbError> {
    if resp.is_empty() {
        return Ok(None);
    }

    let len = resp.get_u16_le()? as usize;
    if resp.remaining() < len {
        return err_invalid_response();
    }
    let salt = resp.slice(..len);
    // * DO NOT PARSE AS HEXADECIMAL *
    let salt = salt.to_vec();
    resp.advance(len)?;

    let len = resp.get_u16_le()? as usize;
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
    resp.advance(len)?;

    Ok(Some(SrpAuthData {
        salt: salt.into_boxed_slice(),
        pub_key: pub_key.into_boxed_slice(),
    }))
}

/// Parse the result of an `InfoSql` requesting affected rows data
pub fn parse_info_sql_affected_rows(data: &mut Bytes) -> Result<usize, FbError> {
    let mut affected_rows = 0;

    let item = data.get_u8()?;

    if item == ibase::isc_info_end as u8 {
        return Ok(0); // No affected rows data
    }
    debug_assert_eq!(item, ibase::isc_info_sql_records as u8);

    data.advance(2)?; // Skip data length

    loop {
        match data.get_u8()? as u32 {
            ibase::isc_info_req_select_count => {
                // Not interested in the selected count
                data.advance(6)?; //  Skip data length (assume 0x04 0x00) and data (4 bytes)
            }

            ibase::isc_info_req_insert_count
            | ibase::isc_info_req_update_count
            | ibase::isc_info_req_delete_count => {
                data.advance(2)?; //  Skip data length (assume 0x04 0x00)

                affected_rows += data.get_u32_le()? as usize;
            }

            ibase::isc_info_end => {
                break;
            }

            _ => return Err(FbError::from("Invalid affected rows response")),
        }
    }

    Ok(affected_rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsfbclient_core::charset::UTF_8;

    fn assert_int128_decoded(expected: i128) {
        let mut response = BytesMut::new();
        response.put_u32(1); // Row is present.
        response.put_u32(0); // Protocol 13 null bitmap, aligned to four bytes.
        response.put_i64((expected >> 64) as i64);
        response.put_u64(expected as u64);

        let xsqlda = [XSqlVar {
            sqltype: ibase::SQL_INT128 as i16 + 1,
            data_length: 16,
            alias_name: "VALUE".into(),
            ..Default::default()
        }];

        let mut response = response.freeze();
        let columns =
            parse_sql_response(&mut response, &xsqlda, ProtocolVersion::V13, &UTF_8, &[]).unwrap();

        match &columns[0] {
            ParsedColumn::Complete(column) => match &column.value {
                SqlType::Int128(actual) => assert_eq!(*actual, expected),
                actual => panic!("expected INT128, got {actual:?}"),
            },
            ParsedColumn::Blob { .. } => panic!("expected INT128, got blob"),
        }
    }

    #[test]
    fn int128_wire_value_keeps_all_bits() {
        assert_int128_decoded(123456789012345678901234567890_i128);
        assert_int128_decoded(-123456789012345678901234567890_i128);
    }

    /// Reads the protocol offers back out of a connect packet.
    fn offered_versions(max_protocol: u16) -> Vec<(u32, u32)> {
        let srp_key: [u8; 32] = rand::random();
        let mut b = connect(
            WireOp::Attach,
            "db.fdb",
            "SYSDBA",
            "me",
            "127.0.0.1:1",
            &srp_key,
            max_protocol,
        );

        b.get_u32().unwrap(); // op_connect
        b.get_u32().unwrap(); // op_attach
        b.get_u32().unwrap(); // CONNECT_VERSION
        b.get_u32().unwrap(); // arch_generic
        b.get_wire_bytes().unwrap(); // db name
        let count = b.get_u32().unwrap();
        b.get_wire_bytes().unwrap(); // user identification

        let mut offers = Vec::new();
        for _ in 0..count {
            let version = b.get_u32().unwrap();
            assert_eq!(b.get_u32().unwrap(), 1, "arch_generic");
            assert_eq!(b.get_u32().unwrap(), 0, "min ptype");
            assert_eq!(b.get_u32().unwrap(), 5, "max ptype = lazy_send");
            let weight = b.get_u32().unwrap();
            offers.push((version, weight));
        }
        assert!(b.is_empty(), "{} bytes after the protocol offers", b.len());
        offers
    }

    /// The server picks the highest offered version it supports, so the offer
    /// list decides whether a Firebird 5 server answers 19 (inline blobs) or,
    /// with FB_MAX_PROTOCOL=13, the pre-fork 13.
    #[test]
    fn connect_offers_protocols_up_to_the_cap_with_ascending_weights() {
        let all = offered_versions(19);
        let versions: Vec<u32> = all.iter().map(|(v, _)| *v).collect();
        assert_eq!(
            versions,
            ProtocolVersion::OFFERED
                .iter()
                .map(|v| *v as u32)
                .collect::<Vec<_>>()
        );
        assert!(
            all.windows(2).all(|w| w[0].1 < w[1].1),
            "weights must prefer newer versions: {all:?}"
        );
        assert!(all.len() <= 10, "pre-FB6 servers read at most 10 offers");

        let capped: Vec<u32> = offered_versions(13).iter().map(|(v, _)| *v).collect();
        assert_eq!(
            capped,
            [
                ProtocolVersion::V10 as u32,
                ProtocolVersion::V11 as u32,
                ProtocolVersion::V12 as u32,
                ProtocolVersion::V13 as u32,
            ]
        );

        let fb4: Vec<u32> = offered_versions(16).iter().map(|(v, _)| *v).collect();
        assert_eq!(fb4.last(), Some(&(ProtocolVersion::V16 as u32)));
        assert!(!fb4.contains(&(ProtocolVersion::V19 as u32)));
    }

    #[test]
    fn protocol_numbers_strip_the_flag_sign_extension() {
        assert_eq!(ProtocolVersion::V10.number(), 10);
        assert_eq!(ProtocolVersion::V13.number(), 13);
        assert_eq!(ProtocolVersion::V16.number(), 16);
        assert_eq!(ProtocolVersion::V19.number(), 19);
        assert!(ProtocolVersion::V19 > ProtocolVersion::V16);
        assert!(ProtocolVersion::V16 > ProtocolVersion::V13);
    }

    /// P_SQLDATA (protocol.cpp): timeout from 16, cursor flags from 18, inline
    /// blob size from 19 — and nothing before 16. The head of the packet must
    /// not move.
    #[test]
    fn execute_trailing_fields_follow_the_negotiated_version() {
        let blr = [1u8, 2, 3];
        let data = [9u8, 9, 9, 9];
        let base = execute(1, 2, &blr, &data, ProtocolVersion::V13, 65535);
        let n = base.len();

        assert_eq!(
            execute(1, 2, &blr, &data, ProtocolVersion::V15, 65535),
            base
        );

        let v16 = execute(1, 2, &blr, &data, ProtocolVersion::V16, 65535);
        assert_eq!(&v16[..n], &base[..]);
        assert_eq!(&v16[n..], &[0, 0, 0, 0], "p_sqldata_timeout = 0");

        let v19 = execute(1, 2, &blr, &data, ProtocolVersion::V19, 65535);
        assert_eq!(&v19[..n], &base[..]);
        assert_eq!(
            &v19[n..],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF],
            "timeout, cursor flags, inline blob size (65535 BE)"
        );

        let base2 = execute2(1, 2, &blr, &data, &blr, ProtocolVersion::V13, 0);
        let m = base2.len();
        let v19_2 = execute2(1, 2, &blr, &data, &blr, ProtocolVersion::V19, 1024);
        assert_eq!(&v19_2[..m], &base2[..]);
        assert_eq!(&v19_2[m..], &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0]);

        // Inlining off is still three fields at 19 (the server reads them all).
        let off = execute(1, 2, &blr, &data, ProtocolVersion::V19, 0);
        assert_eq!(off.len(), n + 12);
        assert_eq!(&off[n + 8..], &[0, 0, 0, 0]);
    }

    #[test]
    fn get_segment_asks_for_the_protocol_maximum_at_most() {
        let mut req = get_segment(5, 1 << 20);
        assert_eq!(req.get_u32().unwrap(), WireOp::GetSegment as u32);
        assert_eq!(req.get_u32().unwrap(), 5);
        assert_eq!(
            req.get_u32().unwrap(),
            BLOB_SEGMENT_BUFFER,
            "USHORT on the wire"
        );
        assert_eq!(req.get_u32().unwrap(), 0);
        assert!(req.is_empty());

        let mut small = get_segment(5, 1024);
        small.advance(8).unwrap();
        assert_eq!(small.get_u32().unwrap(), 1024);
    }

    #[test]
    fn unframe_segments_joins_the_u16le_framed_runs() {
        let mut b = BytesMut::new();
        b.put_u16_le(3);
        b.put_slice(b"abc");
        b.put_u16_le(0); // an empty segment in the middle
        b.put_u16_le(2);
        b.put_slice(b"de");
        assert_eq!(&unframe_segments(b.freeze()).unwrap()[..], b"abcde");

        assert!(unframe_segments(Bytes::new()).unwrap().is_empty());

        // The single-segment fast path hands the bytes through unchanged.
        let mut one = BytesMut::new();
        one.put_u16_le(4);
        one.put_slice(b"wxyz");
        assert_eq!(&unframe_segments(one.freeze()).unwrap()[..], b"wxyz");

        // A frame claiming more bytes than present is a protocol error, not a panic.
        let mut cut = BytesMut::new();
        cut.put_u16_le(9);
        cut.put_slice(b"ab");
        assert!(unframe_segments(cut.freeze()).is_err());
    }

    /// Body of an op_inline_blob as the server encodes it (op code excluded).
    fn inline_blob_body(tr_id: u32, blob_id: u64, segments: &[&[u8]]) -> Bytes {
        let mut data = BytesMut::new();
        for seg in segments {
            data.put_u16_le(seg.len() as u16);
            data.put_slice(seg);
        }
        let mut b = BytesMut::new();
        b.put_u32(tr_id);
        b.put_u64(blob_id);
        // isc_info_blob_* clumplets; opaque to the client
        b.put_wire_bytes(&[4, 2, 0, 1, 0, 6, 4, 0, 5, 0, 0, 0, 1]);
        b.put_wire_bytes(&data);
        b.freeze()
    }

    #[test]
    fn parse_inline_blob_reads_the_p_inline_blob_layout() {
        let mut body = inline_blob_body(3, 0x0000_0007_0000_0009, &[b"hel", b"lo"]);
        let blob = parse_inline_blob(&mut body).unwrap();
        assert_eq!(blob.id, 0x0000_0007_0000_0009);
        assert_eq!(&blob.data[..], b"hello");
        assert!(body.is_empty(), "{} bytes left", body.len());

        let mut empty = inline_blob_body(3, 1, &[]);
        let blob = parse_inline_blob(&mut empty).unwrap();
        assert!(blob.data.is_empty());
        assert!(empty.is_empty());

        // A truncated packet reports "missing bytes" so read_with waits for more.
        let whole = inline_blob_body(3, 1, &[b"abc"]);
        let mut cut = whole.slice(..whole.len() - 3);
        assert!(crate::util::is_incomplete(
            &parse_inline_blob(&mut cut).unwrap_err()
        ));
    }

    fn blob_var(name: &str, subtype: i16) -> XSqlVar {
        XSqlVar {
            sqltype: ibase::SQL_BLOB as i16 + 1,
            sqlsubtype: subtype,
            alias_name: name.into(),
            ..Default::default()
        }
    }

    /// A row with a text blob, a binary blob and a NULL blob. Only the blobs
    /// the server inlined complete here; the rest stay `ParsedColumn::Blob`
    /// for the pipelined read.
    #[test]
    fn inlined_blobs_complete_their_columns_during_row_decode() {
        let xsqlda = [
            blob_var("SRC", 1),
            blob_var("BIN", 0),
            blob_var("NIL", 1),
            blob_var("BIG", 1),
        ];

        let mut row = BytesMut::new();
        row.put_u32(1); // messages: a row follows
        row.put_slice(&[0b0000_0100, 0, 0, 0]); // null bitmap: column 2 is NULL
        row.put_u64(0xA1); // SRC
        row.put_u64(0xB2); // BIN
        row.put_u64(0xD4); // BIG (NIL has no data on the wire when NULL)

        let inline = [
            InlineBlob {
                id: 0xA1,
                data: Bytes::from_static("texto é".as_bytes()),
            },
            InlineBlob {
                id: 0xB2,
                data: Bytes::from_static(&[0, 1, 255]),
            },
        ];

        let mut resp = row.freeze();
        let cols =
            parse_sql_response(&mut resp, &xsqlda, ProtocolVersion::V19, &UTF_8, &inline).unwrap();
        assert!(resp.is_empty());
        assert_eq!(cols.len(), 4);

        match &cols[0] {
            ParsedColumn::Complete(c) => {
                assert_eq!(&*c.name, "SRC");
                assert_eq!(c.raw_type, ibase::SQL_BLOB);
                assert!(
                    matches!(&c.value, SqlType::Text(t) if t == "texto é"),
                    "{:?}",
                    c.value
                );
            }
            other => panic!("SRC should be complete: {}", describe(other)),
        }
        match &cols[1] {
            ParsedColumn::Complete(c) => {
                assert!(matches!(&c.value, SqlType::Binary(b) if b == &[0, 1, 255]));
            }
            other => panic!("BIN should be complete: {}", describe(other)),
        }
        match &cols[2] {
            ParsedColumn::Complete(c) => assert!(c.value.is_null()),
            other => panic!("NIL should be NULL: {}", describe(other)),
        }
        match &cols[3] {
            ParsedColumn::Blob {
                binary,
                id,
                col_name,
            } => {
                assert!(!binary);
                assert_eq!(id.0, 0xD4);
                assert_eq!(&**col_name, "BIG");
            }
            other => panic!(
                "BIG was not inlined and must stay pending: {}",
                describe(other)
            ),
        }

        // Without inline blobs every non-NULL blob stays pending (protocol 13 behaviour).
        let mut again = BytesMut::new();
        again.put_u32(1);
        again.put_slice(&[0b0000_0100, 0, 0, 0]);
        again.put_u64(0xA1);
        again.put_u64(0xB2);
        again.put_u64(0xD4);
        let cols = parse_sql_response(
            &mut again.freeze(),
            &xsqlda,
            ProtocolVersion::V13,
            &UTF_8,
            &[],
        )
        .unwrap();
        assert_eq!(
            cols.iter()
                .filter(|c| matches!(c, ParsedColumn::Blob { .. }))
                .count(),
            3
        );
    }

    fn describe(pc: &ParsedColumn) -> String {
        match pc {
            ParsedColumn::Complete(c) => format!("complete {:?}", c.value),
            ParsedColumn::Blob { id, .. } => format!("pending blob {:#x}", id.0),
        }
    }
}
