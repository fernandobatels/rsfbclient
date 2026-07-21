//! Native service manager: a thin, safe wrapper over the
//! `isc_service_attach` / `isc_service_start` / `isc_service_query` /
//! `isc_service_detach` client entry points.
//!
//! This is the low-level half of the Services API: it owns the service
//! handle and moves parameter blocks and reply buffers across the FFI
//! boundary. The user-facing API (SPB construction, backup/restore
//! actions, reply parsing) lives in the `rsfbclient` crate's `services`
//! module, which drives this one.

use crate::{connection::LinkageMarker, ibase, ibase::IBase, status::Status};
use rsfbclient_core::FbError;

/// A native attachment to a Firebird server's service manager
/// (`service_mgr`).
///
/// Detaches automatically on drop.
pub struct NativeServiceManager<T: LinkageMarker> {
    ibase: T::L,
    status: Status,
    handle: ibase::isc_svc_handle,
}

#[cfg(feature = "linking")]
impl NativeServiceManager<crate::connection::DynLink> {
    /// Attach to `service_mgr` via the dynamically linked fbclient.
    pub fn attach_dyn_link(host: &str, port: u16, attach_spb: &[u8]) -> Result<Self, FbError> {
        Self::attach(ibase::IBaseLinking, host, port, attach_spb)
    }
}

#[cfg(feature = "dynamic_loading")]
impl NativeServiceManager<crate::connection::DynLoad> {
    /// Attach to `service_mgr`, loading the fbclient library from
    /// `lib_path` first.
    pub fn attach_dyn_load(
        lib_path: &str,
        host: &str,
        port: u16,
        attach_spb: &[u8],
    ) -> Result<Self, FbError> {
        let lib = ibase::IBaseDynLoading::with_client(lib_path.as_ref())
            .map_err(|e| FbError::from(e.to_string()))?;
        Self::attach(lib, host, port, attach_spb)
    }
}

impl<T: LinkageMarker> NativeServiceManager<T> {
    /// Attach to `service_mgr` on `host:port` with the supplied
    /// attach-SPB (credentials).
    ///
    /// `ibase` is the loaded/linked client library, obtained the same
    /// way the connection builders obtain theirs.
    fn attach(ibase: T::L, host: &str, port: u16, attach_spb: &[u8]) -> Result<Self, FbError> {
        let mut status: Status = Default::default();
        let mut handle: ibase::isc_svc_handle = 0;

        let conn_string = format!("{}/{}:service_mgr", host, port);

        unsafe {
            if ibase.isc_service_attach()(
                &mut status[0],
                conn_string.len() as u16,
                conn_string.as_ptr() as *const _,
                &mut handle,
                attach_spb.len() as u16,
                attach_spb.as_ptr() as *const _,
            ) != 0
            {
                return Err(status.as_error(&ibase));
            }
        }

        debug_assert_ne!(handle, 0);

        Ok(Self {
            ibase,
            status,
            handle,
        })
    }

    /// Start a service action (`isc_action_svc_*` request block).
    pub fn start(&mut self, request: &[u8]) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_service_start()(
                &mut self.status[0],
                &mut self.handle,
                std::ptr::null_mut(),
                request.len() as u16,
                request.as_ptr() as *const _,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    /// Run one information request against the service, filling `buffer`
    /// with the reply clumplets.
    pub fn query(
        &mut self,
        send_items: &[u8],
        receive_items: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), FbError> {
        unsafe {
            if self.ibase.isc_service_query()(
                &mut self.status[0],
                &mut self.handle,
                std::ptr::null_mut(),
                send_items.len() as u16,
                send_items.as_ptr() as *const _,
                receive_items.len() as u16,
                receive_items.as_ptr() as *const _,
                buffer.len() as u16,
                buffer.as_mut_ptr() as *mut _,
            ) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        Ok(())
    }

    /// Detach from the service manager. Called automatically on drop.
    pub fn detach(&mut self) -> Result<(), FbError> {
        unsafe {
            if self.handle != 0
                && self.ibase.isc_service_detach()(&mut self.status[0], &mut self.handle) != 0
            {
                return Err(self.status.as_error(&self.ibase));
            }
        }
        self.handle = 0;
        Ok(())
    }
}

impl<T: LinkageMarker> Drop for NativeServiceManager<T> {
    fn drop(&mut self) {
        self.detach().ok();
    }
}
