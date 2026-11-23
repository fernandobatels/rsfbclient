//! Cross-thread cancellation and interruption of database attachments.
use super::{simple::TypeConnectionContainer, Connection, SimpleConnection};
use crate::FbError;

use std::sync::{Arc, Mutex};

/// One-shot callback that interrupts a disposable attachment.
pub type AttachmentInterrupt = Box<dyn FnOnce() -> Result<(), FbError> + Send>;
type StatementInterrupt = Box<dyn Fn() -> Result<(), FbError> + Send>;

struct StatementInterrupts {
    cancel: StatementInterrupt,
    abort: StatementInterrupt,
}

struct StatementControl {
    interrupts: Mutex<Option<StatementInterrupts>>,
    aborted: std::sync::atomic::AtomicBool,
}

/// A cross-thread control for cancelling work on a single attachment.
/// Revoke it when the operation finishes, before reusing the attachment.
#[derive(Clone)]
pub struct StatementCancelHandle(Arc<StatementControl>);

impl StatementCancelHandle {
    /// Request cancellation while this attachment and command still own the handle.
    pub fn request(&self) -> Result<bool, FbError> {
        let slot = self
            .0
            .interrupts
            .lock()
            .map_err(|_| "Statement cancellation lock is unavailable")?;
        if let Some(interrupts) = slot.as_ref() {
            (interrupts.cancel)()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Forcibly disconnect this command's attachment, discarding its transaction.
    /// The callback is consumed while holding the revocation lock, so a stale
    /// handle cannot disconnect a later command or outlive its attachment.
    pub fn abort(&self) -> Result<bool, FbError> {
        let mut slot = self
            .0
            .interrupts
            .lock()
            .map_err(|_| "Statement cancellation lock is unavailable")?;
        let Some(interrupts) = slot.take() else {
            return Ok(false);
        };
        (interrupts.abort)()?;
        self.0
            .aborted
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(true)
    }

    pub fn was_aborted(&self) -> bool {
        self.0.aborted.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Disable all outstanding copies, waiting for any in-flight callback.
    pub fn revoke(&self) {
        self.0
            .interrupts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

fn install_statement_cancel<C: rsfbclient_core::FirebirdClient>(
    connection: &mut Connection<C>,
    interrupt: StatementInterrupt,
    abort: StatementInterrupt,
) -> StatementCancelHandle {
    if let Some(old) = connection.statement_cancel.take() {
        old.revoke();
    }
    let handle = StatementCancelHandle(Arc::new(StatementControl {
        interrupts: Mutex::new(Some(StatementInterrupts {
            cancel: interrupt,
            abort,
        })),
        aborted: std::sync::atomic::AtomicBool::new(false),
    }));
    connection.statement_cancel = Some(handle.clone());
    handle
}

impl SimpleConnection {
    /// Request cancellation of an active operation without disconnecting.
    pub fn statement_cancel_handle(&mut self) -> Result<StatementCancelHandle, FbError> {
        let handle = match &mut self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(connection) => {
                let interrupt = unsafe {
                    connection.cli.attachment_cancel(
                        connection.handle,
                        rsfbclient_core::ibase::fb_cancel_raise as u16,
                    )?
                };
                let abort = unsafe {
                    connection.cli.attachment_cancel(
                        connection.handle,
                        rsfbclient_core::ibase::fb_cancel_abort as u16,
                    )?
                };
                install_statement_cancel(connection, interrupt, abort)
            }
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(connection) => {
                let interrupt = unsafe {
                    connection.cli.attachment_cancel(
                        connection.handle,
                        rsfbclient_core::ibase::fb_cancel_raise as u16,
                    )?
                };
                let abort = unsafe {
                    connection.cli.attachment_cancel(
                        connection.handle,
                        rsfbclient_core::ibase::fb_cancel_abort as u16,
                    )?
                };
                install_statement_cancel(connection, interrupt, abort)
            }
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(connection) => {
                let interrupt = connection.cli.statement_interrupt()?;
                let socket = connection.cli.clone_socket()?;
                let abort = Box::new(move || {
                    socket.shutdown(std::net::Shutdown::Both)?;
                    Ok(())
                });
                install_statement_cancel(connection, interrupt, abort)
            }
        };
        Ok(handle)
    }

    /// Interrupt work on a disposable connection from another thread.
    ///
    /// # Safety
    /// The callback must be discarded or finished before the connection is
    /// closed, dropped, or replaced.
    pub unsafe fn disposable_interrupt(&self) -> Result<AttachmentInterrupt, FbError> {
        match &self.inner {
            #[cfg(feature = "linking")]
            TypeConnectionContainer::NativeDynLink(connection) => unsafe {
                let abort = connection.cli.attachment_cancel(
                    connection.handle,
                    rsfbclient_core::ibase::fb_cancel_abort as u16,
                )?;
                Ok(Box::new(move || abort()))
            },
            #[cfg(feature = "dynamic_loading")]
            TypeConnectionContainer::NativeDynLoad(connection) => unsafe {
                let abort = connection.cli.attachment_cancel(
                    connection.handle,
                    rsfbclient_core::ibase::fb_cancel_abort as u16,
                )?;
                Ok(Box::new(move || abort()))
            },
            #[cfg(feature = "pure_rust")]
            TypeConnectionContainer::PureRust(connection) => {
                let socket = connection.cli.clone_socket()?;
                Ok(Box::new(move || {
                    socket.shutdown(std::net::Shutdown::Both)?;
                    Ok(())
                }))
            }
        }
    }
}
