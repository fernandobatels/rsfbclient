//! `FirebirdConnection` implementation for the native fbclient

use crate::{ibase::IBase, params::Params, row::ColumnBuffer, status::Status, xsqlda::XSqlDa};
use rsfbclient_core::*;
use std::{collections::HashMap, convert::TryFrom, ptr};

type NativeDbHandle = ibase::isc_db_handle;
type NativeTrHandle = ibase::isc_tr_handle;
type NativeStmtHandle = ibase::isc_stmt_handle;

/// Client that wraps the native fbclient library
pub struct NativeFbClient {
    ibase: IBase,
    status: Status,
    /// Output xsqldas and column buffers for the prepared statements
    stmt_data_map: HashMap<ibase::isc_tr_handle, (XSqlDa, Vec<ColumnBuffer>)>,
    charset: Charset,
}

#[derive(Clone, Default)]
pub struct RemoteConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
}

#[derive(Clone, Default)]
pub struct NativeFbAttachmentConfig {
    pub db_name: String,
    pub user: String,
    pub remote: Option<RemoteConfig>,
}

impl NativeFbClient {
    #[cfg(feature = "linking")]
    pub fn new_static_linked(charset: Charset) -> Result<Self, FbError> {
        Ok(Self {
            ibase: IBase::Linking,
            status: Default::default(),
            stmt_data_map: Default::default(),
            charset,
        })
    }

    #[cfg(feature = "dynamic_loading")]
    pub fn new_dyn_linked(charset: Charset, lib_path: &str) -> Result<Self, FbError> {
        let dyn_client =
            IBase::with_client(lib_path.to_string()).map_err(|e| FbError::from(e.to_string()))?;

        Ok(Self {
            ibase: dyn_client,
            status: Default::default(),
            stmt_data_map: Default::default(),
            charset,
        })
    }
}

impl FirebirdClientDbOps for NativeFbClient {
    type DbHandle = NativeDbHandle;
    type AttachmentConfig = NativeFbAttachmentConfig;

    fn attach_database(
        &mut self,
        config: &Self::AttachmentConfig,
    ) -> Result<NativeDbHandle, FbError> {
        let user = &config.user;
        let mut password = None;
        let db_name = &config.db_name;
        let maybe_remote = &config.remote;

        let conn_string = match maybe_remote {
            None => db_name.clone(),
            Some(remote_conf) => {
                password = Some(remote_conf.password.as_str());
                format!(
                    "{}/{}:{}",
                    remote_conf.host.as_str(),
                    remote_conf.port,
                    db_name.as_str()
                )
            }
        };

        let mut handle = 0;

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

            dpb
        };

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

    fn detach_database(&mut self, db_handle: NativeDbHandle) -> Result<(), FbError> {
        let mut handle = db_handle;
        unsafe {
            // Close the connection, if the handle is valid
            if handle != 0
                && self.ibase.isc_detach_database()(&mut self.status[0], &mut handle) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    fn drop_database(&mut self, db_handle: NativeDbHandle) -> Result<(), FbError> {
        let mut handle = db_handle;
        unsafe {
            if self.ibase.isc_drop_database()(&mut self.status[0], &mut handle) != 0 {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }
}

impl FirebirdClientSqlOps for NativeFbClient {
    type DbHandle = NativeDbHandle;
    type TrHandle = NativeTrHandle;
    type StmtHandle = NativeStmtHandle;

    fn begin_transaction(
        &mut self,
        mut db_handle: Self::DbHandle,
        isolation_level: TrIsolationLevel,
    ) -> Result<Self::TrHandle, FbError> {
        let mut handle = 0;

        // Transaction parameter buffer
        let tpb = [ibase::isc_tpb_version3 as u8, isolation_level as u8];

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
                    db_handle: &mut db_handle,
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
        tr_handle: Self::TrHandle,
        op: TrOp,
    ) -> Result<(), FbError> {
        let mut handle = tr_handle;
        unsafe {
            if match op {
                TrOp::Commit => {
                    self.ibase.isc_commit_transaction()(&mut self.status[0], &mut handle)
                }
                TrOp::CommitRetaining => {
                    self.ibase.isc_commit_retaining()(&mut self.status[0], &mut handle)
                }
                TrOp::Rollback => {
                    self.ibase.isc_rollback_transaction()(&mut self.status[0], &mut handle)
                }
                TrOp::RollbackRetaining => {
                    self.ibase.isc_rollback_retaining()(&mut self.status[0], &mut handle)
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
        mut db_handle: Self::DbHandle,
        mut tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError> {
        let sql = self.charset.encode(sql)?;

        unsafe {
            if self.ibase.isc_dsql_execute_immediate()(
                &mut self.status[0],
                &mut db_handle,
                &mut tr_handle,
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
        mut db_handle: Self::DbHandle,
        mut tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, Self::StmtHandle), FbError> {
        let sql = self.charset.encode(sql)?;

        let mut handle = 0;

        let mut xsqlda = XSqlDa::new(1);

        let mut stmt_type = 0;

        unsafe {
            if self.ibase.isc_dsql_allocate_statement()(
                &mut self.status[0],
                &mut db_handle,
                &mut handle,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }

            if self.ibase.isc_dsql_prepare()(
                &mut self.status[0],
                &mut tr_handle,
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
                let xcol = xsqlda.get_xsqlvar_mut(col as usize).unwrap();

                ColumnBuffer::from_xsqlvar(xcol)
            })
            .collect::<Result<_, _>>()?;

        self.stmt_data_map.insert(handle, (xsqlda, col_buffers));

        Ok((stmt_type, handle))
    }

    fn free_statement(
        &mut self,
        mut stmt_handle: Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_dsql_free_statement()(
                &mut self.status[0],
                &mut stmt_handle,
                op as u16,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }

        if op == FreeStmtOp::Drop {
            self.stmt_data_map.remove(&stmt_handle);
        }

        Ok(())
    }

    fn execute(
        &mut self,
        mut db_handle: Self::DbHandle,
        mut tr_handle: Self::TrHandle,
        mut stmt_handle: Self::StmtHandle,
        params: Vec<SqlType>,
    ) -> Result<(), FbError> {
        let _ = self
            .stmt_data_map
            .get(&stmt_handle)
            .ok_or_else(|| FbError::from("Tried to fetch a dropped statement"))?;

        let params = Params::new(
            &mut db_handle,
            &mut tr_handle,
            &self.ibase,
            &mut self.status,
            &mut stmt_handle,
            params,
            &self.charset,
        )?;

        unsafe {
            if self.ibase.isc_dsql_execute()(
                &mut self.status[0],
                &mut tr_handle,
                &mut stmt_handle,
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

        Ok(())
    }

    fn fetch(
        &mut self,
        mut db_handle: Self::DbHandle,
        mut tr_handle: Self::TrHandle,
        mut stmt_handle: Self::StmtHandle,
    ) -> Result<Option<Vec<Column>>, FbError> {
        let (xsqlda, col_buf) = self
            .stmt_data_map
            .get(&stmt_handle)
            .ok_or_else(|| FbError::from("Tried to fetch a dropped statement"))?;

        unsafe {
            let fetch_status =
                self.ibase.isc_dsql_fetch()(&mut self.status[0], &mut stmt_handle, 1, &**xsqlda);

            // 100 indicates that no more rows: http://docwiki.embarcadero.com/InterBase/2020/en/Isc_dsql_fetch()
            if fetch_status == 100 {
                return Ok(None);
            }

            if fetch_status != 0 {
                return Err(self.status.as_error(&self.ibase));
            };
        }

        let cols = col_buf
            .iter()
            .map(|cb| cb.to_column(&mut db_handle, &mut tr_handle, &self.ibase, &self.charset))
            .collect::<Result<_, _>>()?;

        Ok(Some(cols))
    }
}
