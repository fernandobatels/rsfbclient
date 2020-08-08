//! `FirebirdConnection` implementation for the native fbclient

use rsfbclient_core::*;

use crate::{ibase::IBase, status::Status};

/// Client that wraps the native fbclient library
pub struct NativeFbClient {
    host: String,
    port: u16,
    ibase: IBase,
    status: Status,
}

impl NativeFbClient {
    #[cfg(not(feature = "dynamic_loading"))]
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            ibase: IBase,
            status: Default::default(),
        }
    }

    #[cfg(feature = "dynamic_loading")]
    pub fn new(host: String, port: u16, lib_path: String) -> Result<Self, FbError> {
        Self {
            host,
            port,
            ibase: IBase::new(lib_path).map_err(|e| FbError {
                code: -1,
                msg: e.to_string(),
            })?,
            status: Default::default(),
        }
    }
}

impl FirebirdClient for NativeFbClient {
    type DbHandle = ibase::isc_db_handle;
    type TrHandle = ibase::isc_tr_handle;
    type StmtHandle = ibase::isc_stmt_handle;

    fn attach_database(
        &mut self,
        db_name: &str,
        user: &str,
        pass: &str,
    ) -> Result<Self::DbHandle, FbError> {
        let mut handle = 0;

        let dpb = {
            let mut dpb: Vec<u8> = Vec::with_capacity(64);

            dpb.extend(&[ibase::isc_dpb_version1 as u8]);

            dpb.extend(&[ibase::isc_dpb_user_name as u8, user.len() as u8]);
            dpb.extend(user.bytes());

            dpb.extend(&[ibase::isc_dpb_password as u8, pass.len() as u8]);
            dpb.extend(pass.bytes());

            // Makes the database convert the strings to utf-8, allowing non ascii characters
            let charset = b"UTF8";

            dpb.extend(&[ibase::isc_dpb_lc_ctype as u8, charset.len() as u8]);
            dpb.extend(charset);

            dpb
        };

        let conn_string = format!("{}/{}:{}", self.host, self.port, db_name);

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

    fn detach_database(&mut self, db_handle: Self::DbHandle) -> Result<(), FbError> {
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

    fn drop_database(&mut self, db_handle: Self::DbHandle) -> Result<(), FbError> {
        let mut handle = db_handle;
        unsafe {
            if self.ibase.isc_drop_database()(&mut self.status[0], &mut handle) != 0 {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

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
        tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(), FbError> {
        todo!()
    }
    fn prepare_statement(
        &mut self,
        db_handle: Self::DbHandle,
        tr_handle: Self::TrHandle,
        dialect: Dialect,
        sql: &str,
    ) -> Result<(StmtType, Self::StmtHandle), FbError> {
        todo!()
    }
    fn free_statement(
        &mut self,
        stmt_handle: Self::StmtHandle,
        op: FreeStmtOp,
    ) -> Result<(), FbError> {
        todo!()
    }
    fn execute(
        &mut self,
        tr_handle: Self::TrHandle,
        stmt_handle: Self::StmtHandle,
        params: &[Param],
    ) -> Result<(), FbError> {
        todo!()
    }
    fn fetch(
        &mut self,
        stmt_handle: Self::StmtHandle,
    ) -> Result<Option<Vec<Option<Column>>>, FbError> {
        todo!()
    }
}
