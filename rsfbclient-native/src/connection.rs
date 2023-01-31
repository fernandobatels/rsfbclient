//! `FirebirdConnection` implementation for the native fbclient

use crate::{
    ibase::{self, IBase},
    params::Params,
    row::ColumnBuffer,
    status::Status,
    xsqlda::XSqlDa,
};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use rsfbclient_core::*;
use std::ffi::CString;
use std::os::raw::c_char;
use std::{convert::TryFrom, io::Cursor, ptr, str};

type NativeDbHandle = ibase::isc_db_handle;
type NativeTrHandle = ibase::isc_tr_handle;
type NativeStmtHandle = ibase::isc_stmt_handle;

/// Client that wraps the native fbclient library
pub struct NativeFbClient<T: LinkageMarker> {
    ibase: T::L,
    status: Status,
    charset: Charset,
}

/// The remote part of native client configuration
#[derive(Clone, Default)]
pub struct RemoteConfig {
    pub host: String,
    pub port: u16,
    pub pass: String,
}

/// Data associated with a prepared statement
pub struct StmtHandleData {
    /// Statement handle
    handle: NativeStmtHandle,
    /// Output xsqlda
    xsqlda: XSqlDa,
    /// Buffers for the output xsqlda
    col_buffers: Vec<ColumnBuffer>,
}

///The common part of native client configuration (for both embedded/remote)
#[derive(Clone, Default)]
pub struct NativeFbAttachmentConfig {
    pub db_name: String,
    pub user: String,
    pub role_name: Option<String>,
    pub remote: Option<RemoteConfig>,
}

/// A marker trait which can be used to
/// obtain the associated client instance
pub trait LinkageMarker: Send + Sync {
    type L: IBase + Send;
}

/// Configuration details for dynamic linking
#[derive(Clone)]
pub struct DynLink(pub Charset);

#[cfg(feature = "linking")]
impl LinkageMarker for DynLink {
    type L = ibase::IBaseLinking;
}

#[cfg(feature = "linking")]
impl DynLink {
    pub fn to_client(&self) -> NativeFbClient<DynLink> {
        let result: NativeFbClient<DynLink> = NativeFbClient {
            ibase: ibase::IBaseLinking,
            status: Default::default(),
            charset: self.0.clone(),
        };
        result
    }
}

/// Configuration details for dynamic loading
#[derive(Clone)]
pub struct DynLoad {
    pub charset: Charset,
    pub lib_path: String,
}

#[cfg(feature = "dynamic_loading")]
impl LinkageMarker for DynLoad {
    type L = ibase::IBaseDynLoading;
}

#[cfg(feature = "dynamic_loading")]
impl DynLoad {
    pub fn try_to_client(&self) -> Result<NativeFbClient<Self>, FbError> {
        let load_result = ibase::IBaseDynLoading::with_client(self.lib_path.as_ref())
            .map_err(|e| FbError::from(e.to_string()))?;

        let result: NativeFbClient<DynLoad> = NativeFbClient {
            ibase: load_result,
            status: Default::default(),
            charset: self.charset.clone(),
        };

        Ok(result)
    }
}

impl<T: LinkageMarker> FirebirdClientDbOps for NativeFbClient<T> {
    type DbHandle = NativeDbHandle;
    type AttachmentConfig = NativeFbAttachmentConfig;

    fn attach_database(
        &mut self,
        config: &Self::AttachmentConfig,
    ) -> Result<NativeDbHandle, FbError> {
        let (dpb, conn_string) = self.build_dpb(config);
        let mut handle = 0;

        unsafe {
            if self.ibase.isc_attach_database()(
                &mut self.status[0],
                conn_string.len() as i16,
                conn_string.as_ptr() as *const _,
                &mut handle,
                dpb.len() as i16,
                dpb.as_ptr() as *const _,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle, 0);

        Ok(handle)
    }

    fn detach_database(&mut self, db_handle: &mut NativeDbHandle) -> Result<(), FbError> {
        unsafe {
            // Close the connection, if the handle is valid
            if *db_handle != 0
                && self.ibase.isc_detach_database()(&mut self.status[0], db_handle) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    fn drop_database(&mut self, db_handle: &mut NativeDbHandle) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_drop_database()(&mut self.status[0], db_handle) != 0 {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    fn create_database(
        &mut self,
        config: &Self::AttachmentConfig,
        page_size: Option<u32>,
    ) -> Result<NativeDbHandle, FbError> {
        let (mut dpb, conn_string) = self.build_dpb(config);
        let mut handle = 0;

        if let Some(ps) = page_size {
            dpb.extend(&[ibase::isc_dpb_page_size as u8, 4]);
            dpb.write_u32::<LittleEndian>(ps)?;
        }

        unsafe {
            if self.ibase.isc_create_database()(
                &mut self.status[0],
                conn_string.len() as i16,
                conn_string.as_ptr() as *const _,
                &mut handle,
                dpb.len() as i16,
                dpb.as_ptr() as *const _,
                0,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle, 0);

        Ok(handle)
    }
}

impl<T: LinkageMarker> FirebirdClientSqlOps for NativeFbClient<T> {
    type DbHandle = NativeDbHandle;
    type TrHandle = NativeTrHandle;
    type StmtHandle = StmtHandleData;

    fn begin_transaction(
        &mut self,
        db_handle: &mut Self::DbHandle,
        confs: TransactionConfiguration,
    ) -> Result<Self::TrHandle, FbError> {
        let mut handle = 0;

        // Transaction parameter buffer
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

        #[repr(C)]
        struct IscTeb {
            db_handle: *mut ibase::isc_db_handle,
            tpb_len: usize,
            tpb_ptr: *const u8,
        }

        unsafe {
            if self.ibase.isc_start_multiple()(
                &mut self.status[0],
                &mut handle,
                1,
                &mut IscTeb {
                    db_handle,
                    tpb_len: tpb.len(),
                    tpb_ptr: &tpb[0],
                } as *mut _ as _,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        // Assert that the handle is valid
        debug_assert_ne!(handle, 0);

        Ok(handle)
    }

    fn transaction_operation(
        &mut self,
        tr_handle: &mut Self::TrHandle,
        op: TrOp,
    ) -> Result<(), FbError> {
        let handle = tr_handle;
        unsafe {
            if match op {
                TrOp::Commit => self.ibase.isc_commit_transaction()(&mut self.status[0], handle),
                TrOp::CommitRetaining => {
                    self.ibase.isc_commit_retaining()(&mut self.status[0], handle)
                }
                TrOp::Rollback => {
                    self.ibase.isc_rollback_transaction()(&mut self.status[0], handle)
                }
                TrOp::RollbackRetaining => {
                    self.ibase.isc_rollback_retaining()(&mut self.status[0], handle)
                }
            } != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    fn exec_immediate(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError> {
        let sql = self.charset.encode(sql)?;

        unsafe {
            if self.ibase.isc_dsql_execute_immediate()(
                &mut self.status[0],
                db_handle,
                tr_handle,
                sql.len() as u16,
                sql.as_ptr() as *const _,
                dialect as u16,
                ptr::null(),
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    fn prepare_statement(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, Self::StmtHandle), FbError> {
        let sql = self.charset.encode(sql)?;

        let mut handle = 0;

        let mut xsqlda = XSqlDa::new(1);

        let mut stmt_type = 0;

        unsafe {
            if self.ibase.isc_dsql_allocate_statement()(&mut self.status[0], db_handle, &mut handle)
                != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }

            if self.ibase.isc_dsql_prepare()(
                &mut self.status[0],
                tr_handle,
                &mut handle,
                sql.len() as u16,
                sql.as_ptr() as *const _,
                dialect as u16,
                &mut *xsqlda,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }

            let row_count = xsqlda.sqld;

            if row_count > xsqlda.sqln {
                // Need more XSQLVARs
                xsqlda = XSqlDa::new(row_count);

                if self.ibase.isc_dsql_describe()(&mut self.status[0], &mut handle, 1, &mut *xsqlda)
                    != 0
                {
                    return Err(self.status.as_error(&self.ibase));
                }
            }

            // Get the statement type
            let info_req = [ibase::isc_info_sql_stmt_type as std::os::raw::c_char];
            let mut info_buf = [0; 10];

            if self.ibase.isc_dsql_sql_info()(
                &mut self.status[0],
                &mut handle,
                info_req.len() as i16,
                &info_req[0],
                info_buf.len() as i16,
                &mut info_buf[0],
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }

            for &v in &info_buf[3..] {
                // Search for the data
                if v != 0 {
                    stmt_type = v;
                    break;
                }
            }
        }

        let stmt_type = StmtType::try_from(stmt_type as u8)
            .map_err(|_| FbError::from(format!("Invalid statement type: {}", stmt_type)))?;

        // Create the column buffers and set the xsqlda conercions
        let col_buffers = (0..xsqlda.sqld)
            .map(|col| {
                let xcol = xsqlda
                    .get_xsqlvar_mut(col as usize)
                    .ok_or_else(|| FbError::from("Error getting the xsqlvar"))?;

                ColumnBuffer::from_xsqlvar(xcol)
            })
            .collect::<Result<_, _>>()?;

        Ok((
            stmt_type,
            StmtHandleData {
                handle,
                xsqlda,
                col_buffers,
            },
        ))
    }

    fn free_statement(
        &mut self,
        stmt_handle: &mut Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_dsql_free_statement()(
                &mut self.status[0],
                &mut stmt_handle.handle,
                op as u16,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        Ok(())
    }

    fn execute(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<usize, FbError> {
        let params = Params::new(
            db_handle,
            tr_handle,
            &self.ibase,
            &mut self.status,
            &mut stmt_handle.handle,
            params,
            &self.charset,
        )?;

        unsafe {
            if self.ibase.isc_dsql_execute()(
                &mut self.status[0],
                tr_handle,
                &mut stmt_handle.handle,
                1,
                if let Some(xsqlda) = &params.xsqlda {
                    &**xsqlda
                } else {
                    ptr::null()
                },
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        // Just to make sure the params are not dropped too soon
        drop(params);

        // Get the affected rows count
        let info_req = [ibase::isc_info_sql_records as std::os::raw::c_char];
        let mut info_buf = [0u8; 64];

        unsafe {
            if self.ibase.isc_dsql_sql_info()(
                &mut self.status[0],
                &mut stmt_handle.handle,
                info_req.len() as i16,
                &info_req[0],
                info_buf.len() as i16,
                info_buf.as_mut_ptr() as _,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        let mut affected = 0;

        let mut data = Cursor::new(info_buf);

        if data.read_u8()? == ibase::isc_info_sql_records as u8 {
            let _info_buf_size = data.read_u16::<LittleEndian>()?;

            loop {
                match data.read_u8()? as u32 {
                    ibase::isc_info_req_select_count => {
                        // Not interested in the selected count
                        let len = data.read_u16::<LittleEndian>()? as usize;
                        let _selected = data.read_uint::<LittleEndian>(len)?;
                    }

                    ibase::isc_info_req_insert_count
                    | ibase::isc_info_req_update_count
                    | ibase::isc_info_req_delete_count => {
                        let len = data.read_u16::<LittleEndian>()? as usize;

                        affected += data.read_uint::<LittleEndian>(len)? as usize;
                    }

                    ibase::isc_info_end => {
                        break;
                    }

                    _ => return Err(FbError::from("Invalid affected rows response")),
                }
            }
        }

        Ok(affected as usize)
    }

    fn fetch(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
    ) -> Result<Option<Vec<Column>>, FbError> {
        unsafe {
            let fetch_status = self.ibase.isc_dsql_fetch()(
                &mut self.status[0],
                &mut stmt_handle.handle,
                1,
                &*stmt_handle.xsqlda,
            );

            // 100 indicates that no more rows: http://docwiki.embarcadero.com/InterBase/2020/en/Isc_dsql_fetch()
            if fetch_status == 100 {
                return Ok(None);
            }

            if fetch_status != 0 {
                return Err(self.status.as_error(&self.ibase));
            };
        }

        let cols = stmt_handle
            .col_buffers
            .iter()
            .map(|cb| cb.to_column(db_handle, tr_handle, &self.ibase, &self.charset))
            .collect::<Result<_, _>>()?;

        Ok(Some(cols))
    }

    fn execute2(
        &mut self,
        db_handle: &mut Self::DbHandle,
        tr_handle: &mut Self::TrHandle,
        stmt_handle: &mut Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<Vec<Column>, FbError> {
        let params = Params::new(
            db_handle,
            tr_handle,
            &self.ibase,
            &mut self.status,
            &mut stmt_handle.handle,
            params,
            &self.charset,
        )?;

        unsafe {
            if self.ibase.isc_dsql_execute2()(
                &mut self.status[0],
                tr_handle,
                &mut stmt_handle.handle,
                1,
                if let Some(xsqlda) = &params.xsqlda {
                    &**xsqlda
                } else {
                    ptr::null()
                },
                &*stmt_handle.xsqlda,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        // Just to make sure the params are not dropped too soon
        drop(params);

        let rcol = stmt_handle
            .col_buffers
            .iter()
            .map(|cb| cb.to_column(db_handle, tr_handle, &self.ibase, &self.charset))
            .collect::<Result<_, _>>()?;

        Ok(rcol)
    }
}

impl<T: LinkageMarker> FirebirdClientDbEvents for NativeFbClient<T> {
    fn wait_for_event(
        &mut self,
        db_handle: &mut Self::DbHandle,
        name: String,
    ) -> Result<(), FbError> {
        let mut event_buffer = ptr::null_mut();
        let mut result_buffer = ptr::null_mut();

        let name = CString::new(name.clone()).unwrap();
        let len = unsafe {
            self.ibase.isc_event_block()(
                &mut event_buffer,
                &mut result_buffer,
                1,
                name.as_ptr() as *mut c_char,
            )
        };
        debug_assert!(!event_buffer.is_null() && !result_buffer.is_null());

        // Preparing the event_buffer. Yes, I need call the isc_wait_for_event
        // before call isc_wait_for_event.
        //
        // PHP example: https://github.com/FirebirdSQL/php-firebird/blob/b6c288326678f7b8613f5a7d0648ad010c67674c/ibase_events.c#L126
        {
            unsafe {
                if self.ibase.isc_wait_for_event()(
                    &mut self.status[0],
                    db_handle,
                    len as i16,
                    event_buffer,
                    result_buffer,
                ) != 0
                {
                    self.ibase.isc_free()(event_buffer);
                    self.ibase.isc_free()(result_buffer);

                    return Err(self.status.as_error(&self.ibase));
                }
            }

            unsafe {
                self.ibase.isc_event_counts()(
                    &mut self.status[0],
                    len as i16,
                    event_buffer,
                    result_buffer,
                );
            }
        }

        unsafe {
            if self.ibase.isc_wait_for_event()(
                &mut self.status[0],
                db_handle,
                len as i16,
                event_buffer,
                result_buffer,
            ) != 0
            {
                self.ibase.isc_free()(event_buffer);
                self.ibase.isc_free()(result_buffer);

                return Err(self.status.as_error(&self.ibase));
            }

            self.ibase.isc_free()(event_buffer);
            self.ibase.isc_free()(result_buffer);
        }

        Ok(())
    }
}

impl<T: LinkageMarker> NativeFbClient<T> {
    /// Build the dpb and the connection string
    ///
    /// Used by attach database operations
    fn build_dpb(&mut self, config: &NativeFbAttachmentConfig) -> (Vec<u8>, String) {
        let user = &config.user;
        let mut password = None;
        let db_name = &config.db_name;

        let conn_string = match &config.remote {
            None => db_name.clone(),
            Some(remote_conf) => {
                password = Some(remote_conf.pass.as_str());
                format!(
                    "{}/{}:{}",
                    remote_conf.host.as_str(),
                    remote_conf.port,
                    db_name.as_str()
                )
            }
        };

        let dpb = {
            let mut dpb: Vec<u8> = Vec::with_capacity(64);

            dpb.extend(&[ibase::isc_dpb_version1 as u8]);

            dpb.extend(&[ibase::isc_dpb_user_name as u8, user.len() as u8]);
            dpb.extend(user.bytes());

            if let Some(pass_str) = password {
                dpb.extend(&[ibase::isc_dpb_password as u8, pass_str.len() as u8]);
                dpb.extend(pass_str.bytes());
            };

            let charset = self.charset.on_firebird.bytes();

            dpb.extend(&[ibase::isc_dpb_lc_ctype as u8, charset.len() as u8]);
            dpb.extend(charset);

            if let Some(role) = &config.role_name {
                dpb.extend(&[ibase::isc_dpb_sql_role_name as u8, role.len() as u8]);
                dpb.extend(role.bytes());
            }

            dpb
        };

        (dpb, conn_string)
    }
}
