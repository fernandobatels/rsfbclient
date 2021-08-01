//! Structs and functions to write and parse the firebird wire protocol messages

#![allow(non_upper_case_globals)]

use bytes::{BufMut, Bytes, BytesMut};
use std::{convert::TryFrom, str};

use crate::{
    client::{BlobId, FirebirdWireConnection},
    consts::{gds_to_msg, AuthPluginType, Cnct, ProtocolVersion, WireOp},
    srp::*,
    util::*,
    xsqlda::{XSqlVar, XSQLDA_DESCRIBE_VARS},
};
use rsfbclient_core::{ibase, Charset, Column, FbError, FreeStmtOp, SqlType, TrOp};

/// Buffer length to use in the connection
pub const BUFFER_LENGTH: u32 = 1024;

/// Connection request
pub fn connect(db_name: &str, user: &str, username: &str, hostname: &str, srp_key: &[u8]) -> Bytes {
    let protocols = [
        // PROTOCOL_VERSION, Arch type (Generic=1), min, max, weight
        [ProtocolVersion::V10 as u32, 1, 0, 5, 2],
        [ProtocolVersion::V11 as u32, 1, 0, 5, 4],
        [ProtocolVersion::V12 as u32, 1, 0, 5, 6],
        [ProtocolVersion::V13 as u32, 1, 0, 5, 8],
    ];

    let mut connect = BytesMut::with_capacity(256);

    connect.put_u32(WireOp::Connect as u32);
    connect.put_u32(WireOp::Attach as u32);
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

        // Request SRP by default
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
) -> Bytes {
    let dpb = build_dpb(user, pass, protocol, charset, None);

    let mut attach = BytesMut::with_capacity(16 + db_name.len() + dpb.len());

    attach.put_u32(WireOp::Attach as u32);
    attach.put_u32(0); // Database Object ID

    attach.put_wire_bytes(db_name.as_bytes());

    attach.put_wire_bytes(&dpb);

    attach.freeze()
}

/// Create db request
pub fn create(
    db_name: &str,
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    charset: Charset,
    page_size: Option<u32>,
) -> Bytes {
    let dpb = build_dpb(user, pass, protocol, charset, page_size);

    let mut create = BytesMut::with_capacity(16 + db_name.len() + dpb.len());

    create.put_u32(WireOp::Create as u32);
    create.put_u32(0); // Database Object ID

    create.put_wire_bytes(db_name.as_bytes());

    create.put_wire_bytes(&dpb);

    create.freeze()
}

/// Dpb builder
fn build_dpb(
    user: &str,
    pass: &str,
    protocol: ProtocolVersion,
    charset: Charset,
    page_size: Option<u32>,
) -> Bytes {
    let mut dpb = BytesMut::with_capacity(64);

    dpb.put_u8(1); //Version

    if let Some(ps) = page_size {
        dpb.put_slice(&[ibase::isc_dpb_page_size as u8, 4]);
        dpb.put_u32(ps);
    }

    let charset = charset.on_firebird.as_bytes();

    dpb.put_slice(&[ibase::isc_dpb_lc_ctype as u8, charset.len() as u8]);
    dpb.put_slice(charset);

    dpb.put_slice(&[ibase::isc_dpb_user_name as u8, user.len() as u8]);
    dpb.put_slice(user.as_bytes());

    match protocol {
        // Plaintext password
        ProtocolVersion::V10 => {
            dpb.put_slice(&[ibase::isc_dpb_password as u8, pass.len() as u8]);
            dpb.put_slice(pass.as_bytes());
        }

        // Hashed password
        ProtocolVersion::V11 | ProtocolVersion::V12 => {
            #[allow(deprecated)]
            let enc_pass = pwhash::unix_crypt::hash_with("9z", pass).unwrap();
            let enc_pass = &enc_pass[2..];

            dpb.put_slice(&[ibase::isc_dpb_password_enc as u8, enc_pass.len() as u8]);
            dpb.put_slice(enc_pass.as_bytes());
        }

        // Password already verified
        ProtocolVersion::V13 => {}
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

    req.put_u32(BUFFER_LENGTH);

    Ok(req.freeze())
}

/// Statement information request
pub fn info_sql(stmt_handle: u32, requested_items: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(24 + requested_items.len());

    req.put_u32(WireOp::InfoSql as u32);
    req.put_u32(stmt_handle);
    req.put_u32(0); // Incarnation of object
    req.put_wire_bytes(requested_items);
    req.put_u32(BUFFER_LENGTH);

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

/// Execute prepared statement request.
pub fn execute(tr_handle: u32, stmt_handle: u32, input_blr: &[u8], input_data: &[u8]) -> Bytes {
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

/// Execute prepared statement request.
pub fn execute2(
    tr_handle: u32,
    stmt_handle: u32,
    input_blr: &[u8],
    input_data: &[u8],
    output_blr: &[u8],
) -> Bytes {
    let mut req =
        BytesMut::with_capacity(40 + input_blr.len() + input_data.len() + output_blr.len());

    req.put_u32(WireOp::Execute2 as u32);
    req.put_u32(stmt_handle);
    req.put_u32(tr_handle);

    req.put_wire_bytes(input_blr);
    req.put_u32(0); // Input message number
    req.put_u32(if input_blr.is_empty() { 0 } else { 1 }); // Messages

    req.put_slice(input_data);

    req.put_wire_bytes(output_blr);
    req.put_u32(0); // Output message number

    req.freeze()
}

/// Fetch row request
pub fn fetch(stmt_handle: u32, blr: &[u8]) -> Bytes {
    let mut req = BytesMut::with_capacity(20 + blr.len());

    req.put_u32(WireOp::Fetch as u32);
    req.put_u32(stmt_handle);
    req.put_wire_bytes(blr);
    req.put_u32(0); // Message number
    req.put_u32(1); // Message count TODO: increase to return more rows in one fetch request

    req.freeze()
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

/// Get blob segment request
pub fn get_segment(blob_handle: u32) -> Bytes {
    let mut req = BytesMut::with_capacity(16);

    req.put_u32(WireOp::GetSegment as u32);
    req.put_u32(blob_handle);
    req.put_u32(BUFFER_LENGTH);
    req.put_u32(0); // Data segment, apparently unused

    req.freeze()
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
) -> Result<Option<Vec<ParsedColumn>>, FbError> {
    const END_OF_STREAM: u32 = 100;

    let status = resp.get_u32()?;

    if status == END_OF_STREAM {
        return Ok(None);
    }

    Ok(Some(parse_sql_response(resp, xsqlda, version, charset)?))
}

/// Parse a server sql response (`WireOp::SqlResponse`)
/// Identical to the FetchResponse, but has no status
pub fn parse_sql_response(
    resp: &mut Bytes,
    xsqlda: &[XSqlVar],
    version: ProtocolVersion,
    charset: &Charset,
) -> Result<Vec<ParsedColumn>, FbError> {
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

    let mut data = Vec::with_capacity(xsqlda.len());

    for (col_index, var) in xsqlda.iter().enumerate() {
        if version >= ProtocolVersion::V13 && read_null(resp, col_index)? {
            // There is no data in protocol 13 if null, so just continue
            data.push(ParsedColumn::Complete(Column::new(
                var.alias_name.clone(),
                SqlType::Null,
            )));
            continue;
        }

        // Remove nullable type indicator
        let sqltype = var.sqltype as u32 & (!1);

        match sqltype {
            ibase::SQL_VARYING => {
                let d = resp.get_wire_bytes()?;

                let null = read_null(resp, col_index)?;
                if null {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Null,
                    )))
                } else {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Text(charset.decode(&d[..])?),
                    )))
                }
            }

            ibase::SQL_INT64 => {
                let i = resp.get_i64()?;

                let null = read_null(resp, col_index)?;
                if null {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Null,
                    )))
                } else {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Integer(i),
                    )))
                }
            }

            ibase::SQL_DOUBLE => {
                let f = resp.get_f64()?;

                let null = read_null(resp, col_index)?;
                if null {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Null,
                    )))
                } else {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Floating(f),
                    )))
                }
            }

            ibase::SQL_TIMESTAMP => {
                let ts = ibase::ISC_TIMESTAMP {
                    timestamp_date: resp.get_i32()?,
                    timestamp_time: resp.get_u32()?,
                };

                let null = read_null(resp, col_index)?;
                if null {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Null,
                    )))
                } else {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Timestamp(rsfbclient_core::date_time::decode_timestamp(ts)),
                    )))
                }
            }

            ibase::SQL_BLOB if var.sqlsubtype == 0 || var.sqlsubtype == 1 => {
                let id = resp.get_u64()?;

                let null = read_null(resp, col_index)?;
                if null {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Null,
                    )))
                } else {
                    data.push(ParsedColumn::Blob {
                        binary: var.sqlsubtype == 0,
                        id: BlobId(id),
                        col_name: var.alias_name.clone(),
                    })
                }
            }

            ibase::SQL_BOOLEAN => {
                let b = resp.get_u8()? == 1;
                resp.advance(3)?; // Pad to 4 bytes

                let null = read_null(resp, col_index)?;

                if null {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Null,
                    )))
                } else {
                    data.push(ParsedColumn::Complete(Column::new(
                        var.alias_name.clone(),
                        SqlType::Boolean(b),
                    )))
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

    Ok(data)
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
        col_name: String,
    },
}

impl ParsedColumn {
    /// Get the rest of the data needed for the columns if necessary
    pub fn into_column(
        self,
        conn: &mut FirebirdWireConnection,
        tr_handle: &mut crate::TrHandle,
    ) -> Result<Column, FbError> {
        Ok(match self {
            ParsedColumn::Complete(c) => c,
            ParsedColumn::Blob {
                binary,
                id,
                col_name,
            } => {
                let mut data = Vec::with_capacity(256);

                let blob_handle = conn.open_blob(tr_handle, id)?;

                loop {
                    let (mut segment, end) = conn.get_segment(blob_handle)?;

                    data.put(&mut segment);

                    if end {
                        break;
                    }
                }

                conn.close_blob(blob_handle)?;

                Column::new(
                    col_name,
                    if binary {
                        SqlType::Binary(data)
                    } else {
                        SqlType::Text(conn.charset.decode(data)?)
                    },
                )
            }
        })
    }
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

#[derive(Debug)]
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
