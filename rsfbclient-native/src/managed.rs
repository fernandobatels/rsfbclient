//! Owned native transactions, distributed prepare/recovery, and cancellable events.
//!
//! These APIs use the official client and support Firebird 2.5. A distributed
//! transaction owns all participating attachments and one transaction handle.
//! Applications must durably record their commit decision before committing a
//! prepared transaction. An ambiguous prepare/commit is never rolled back by Drop.

use crate::{
    connection::{transaction_buffer, IscTeb},
    ibase,
};
pub use crate::{DynLoad, LinkageMarker, NativeFbAttachmentConfig, NativeFbClient, RemoteConfig};
use rsfbclient_core::*;
use std::{ffi::c_void, mem::ManuallyDrop, sync::mpsc, time::Duration};

/// Result conversion for a managed transaction. Existing typed APIs are unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub enum RowConversion {
    #[default]
    Native,
    /// Firebird formats scalars as text, preserving NUMERIC/DECIMAL digits.
    /// Every BLOB subtype is returned as binary without charset conversion.
    TextAndBinary,
}

#[derive(Clone, Copy, PartialEq)]
enum TransactionState {
    Active,
    Prepared,
    Uncertain,
    Finished,
}

/// An owned, non-retaining transaction over one or more native attachments.
pub struct NativeTransaction<T: LinkageMarker> {
    client: NativeFbClient<T>,
    databases: Vec<ibase::isc_db_handle>,
    handle: ibase::isc_tr_handle,
    state: TransactionState,
    dialect: Dialect,
}

impl<T: LinkageMarker> NativeTransaction<T> {
    /// Attach all participants and start one transaction with a shared TPB.
    /// Failure releases attachments already opened. No database is created.
    pub fn start(
        mut client: NativeFbClient<T>,
        attachments: &[NativeFbAttachmentConfig],
        dialect: Dialect,
        configuration: TransactionConfiguration,
        conversion: RowConversion,
    ) -> Result<Self, FbError> {
        use crate::ibase::IBase;
        if attachments.is_empty() || attachments.len() > i16::MAX as usize {
            return Err("Invalid number of transaction attachments".into());
        }
        client.text_rows = matches!(conversion, RowConversion::TextAndBinary);
        let mut tx = Self {
            client,
            databases: Vec::new(),
            handle: 0,
            state: TransactionState::Active,
            dialect,
        };
        for config in attachments {
            tx.databases
                .push(tx.client.attach_database(config, dialect, false)?);
        }
        let tpb = transaction_buffer(configuration);
        let mut tebs: Vec<_> = tx
            .databases
            .iter_mut()
            .map(|database| IscTeb {
                db_handle: database,
                tpb_len: tpb.len() as i32,
                tpb_ptr: tpb.as_ptr(),
            })
            .collect();
        let code = unsafe {
            tx.client.ibase.isc_start_multiple()(
                &mut tx.client.status[0],
                &mut tx.handle,
                tebs.len() as i16,
                tebs.as_mut_ptr().cast(),
            )
        };
        tx.check(code)?;
        Ok(tx)
    }

    fn check(&self, code: ibase::ISC_STATUS) -> Result<(), FbError> {
        if code == 0 {
            Ok(())
        } else {
            Err(self.client.status.as_error(&self.client.ibase))
        }
    }

    fn validate_database(&self, database: usize) -> Result<(), FbError> {
        if database >= self.databases.len() {
            Err("Invalid database participant".into())
        } else {
            Ok(())
        }
    }

    /// Execute a statement on a participant, returning SELECT or RETURNING rows.
    /// Statements are always released, including on binding/execution/fetch errors.
    pub fn query(
        &mut self,
        database: usize,
        sql: &str,
        params: Vec<SqlType>,
    ) -> Result<Vec<Vec<Column>>, FbError> {
        self.validate_database(database)?;
        if self.handle == 0 || self.state != TransactionState::Active {
            return Err("Transaction is not active".into());
        }
        let (kind, mut statement) = self.client.prepare_statement(
            &mut self.databases[database],
            &mut self.handle,
            self.dialect,
            sql,
        )?;
        let result = (|| {
            let mut rows = Vec::new();
            if !matches!(kind, StmtType::Select | StmtType::SelectForUpd) && statement.has_output()
            {
                rows.push(self.client.execute2(
                    &mut self.databases[database],
                    &mut self.handle,
                    &mut statement,
                    params,
                )?);
            } else {
                self.client.execute(
                    &mut self.databases[database],
                    &mut self.handle,
                    &mut statement,
                    params,
                )?;
                if statement.has_output() {
                    while let Some(row) = self.client.fetch(
                        &mut self.databases[database],
                        &mut self.handle,
                        &mut statement,
                    )? {
                        rows.push(row);
                    }
                }
            }
            Ok(rows)
        })();
        let release = self.client.free_statement(&mut statement, FreeStmtOp::Drop);
        match result {
            Ok(rows) => {
                release?;
                Ok(rows)
            }
            Err(error) => Err(error),
        }
    }

    /// Prepare every participant. The application persists its own recovery log.
    /// Description bytes are stored in RDB$TRANSACTIONS for recovery identification.
    pub fn prepare(&mut self, description: &[u8]) -> Result<(), FbError> {
        use crate::ibase::IBase;
        if self.handle == 0
            || self.state != TransactionState::Active
            || description.is_empty()
            || description.len() > u16::MAX as usize
        {
            return Err("Invalid prepare state or description".into());
        }
        self.state = TransactionState::Uncertain;
        let code = unsafe {
            self.client.ibase.isc_prepare_transaction2()(
                &mut self.client.status[0],
                &mut self.handle,
                description.len() as u16,
                description.as_ptr(),
            )
        };
        self.check(code)?;
        self.state = TransactionState::Prepared;
        Ok(())
    }

    /// Real (non-retaining) commit. An error may mean the outcome is uncertain.
    pub fn commit(&mut self) -> Result<(), FbError> {
        self.finish(TrOp::Commit)
    }

    /// Roll back an active or successfully prepared transaction. After an
    /// ambiguous operation, use explicit recovery rather than retrying here.
    pub fn rollback(&mut self) -> Result<(), FbError> {
        self.finish(TrOp::Rollback)
    }

    fn finish(&mut self, operation: TrOp) -> Result<(), FbError> {
        if self.handle == 0
            || !matches!(
                self.state,
                TransactionState::Active | TransactionState::Prepared
            )
        {
            return Err("Transaction is finished or requires recovery".into());
        }
        self.state = TransactionState::Uncertain;
        self.client
            .transaction_operation(&mut self.handle, operation)?;
        self.state = TransactionState::Finished;
        Ok(())
    }

    /// Resolve one limbo participant using its Firebird transaction ID.
    /// The caller must verify the saved identity and durable decision first.
    pub fn resolve_limbo(&mut self, database: usize, id: u32, commit: bool) -> Result<(), FbError> {
        use crate::ibase::IBase;
        self.validate_database(database)?;
        let mut handle = 0;
        let bytes = id.to_le_bytes();
        let code = unsafe {
            self.client.ibase.isc_reconnect_transaction()(
                &mut self.client.status[0],
                &mut self.databases[database],
                &mut handle,
                bytes.len() as i16,
                bytes.as_ptr().cast(),
            )
        };
        self.check(code)?;
        // Never perform an implicit rollback after an ambiguous resolution.
        self.client.transaction_operation(
            &mut handle,
            if commit { TrOp::Commit } else { TrOp::Rollback },
        )
    }

    /// Return all 32-bit limbo IDs. A truncated/error response is rejected.
    pub fn limbo_ids(&mut self, database: usize) -> Result<Vec<u32>, FbError> {
        self.validate_database(database)?;
        let bytes = database_info(&mut self.client, &mut self.databases[database], 16)?;
        parse_limbo(&bytes)
    }
}

impl<T: LinkageMarker> Drop for NativeTransaction<T> {
    fn drop(&mut self) {
        if self.handle != 0 && self.state == TransactionState::Active {
            let _ = self
                .client
                .transaction_operation(&mut self.handle, TrOp::Rollback);
        }
        for database in &mut self.databases {
            let _ = self.client.detach_database(database);
        }
    }
}

fn database_info<T: LinkageMarker>(
    client: &mut NativeFbClient<T>,
    database: &mut ibase::isc_db_handle,
    item: u8,
) -> Result<Vec<u8>, FbError> {
    use crate::ibase::IBase;
    let items = [item, 1];
    let mut buffer = vec![0u8; i16::MAX as usize];
    let code = unsafe {
        client.ibase.isc_database_info()(
            &mut client.status[0],
            database,
            items.len() as i16,
            items.as_ptr().cast(),
            buffer.len() as i16,
            buffer.as_mut_ptr().cast(),
        )
    };
    if code != 0 {
        return Err(client.status.as_error(&client.ibase));
    }
    Ok(buffer)
}

fn parse_limbo(buffer: &[u8]) -> Result<Vec<u32>, FbError> {
    let mut offset = 0;
    let mut ids = Vec::new();
    while offset < buffer.len() {
        let tag = buffer[offset];
        offset += 1;
        if tag == 1 {
            return Ok(ids);
        }
        if tag != 16 || offset + 2 > buffer.len() {
            return Err("Incomplete limbo response".into());
        }
        let length = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as usize;
        offset += 2;
        if !(1..=4).contains(&length) || offset + length > buffer.len() {
            return Err("Invalid limbo ID length".into());
        }
        let mut bytes = [0; 4];
        bytes[..length].copy_from_slice(&buffer[offset..offset + length]);
        ids.push(u32::from_le_bytes(bytes));
        offset += length;
    }
    Err("Missing limbo response terminator".into())
}

struct EventContext(mpsc::Sender<Vec<u8>>);
unsafe extern "C" fn callback(context: *mut c_void, length: u16, bytes: *const u8) {
    if context.is_null() || bytes.is_null() || length > 256 {
        return;
    }
    let context = unsafe { &*(context as *const EventContext) };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length as usize) }.to_vec();
    let _ = context.0.send(bytes);
}

/// An event subscription owning a dedicated attachment. Drop cancels it and
/// detaches before freeing callback data. Waiting supports a finite timeout,
/// allowing a service to stop even if no more events arrive.
pub struct NativeEvents<T: LinkageMarker> {
    client: ManuallyDrop<NativeFbClient<T>>,
    database: ibase::isc_db_handle,
    id: i32,
    buffer: Vec<u8>,
    context: ManuallyDrop<Box<EventContext>>,
    receiver: mpsc::Receiver<Vec<u8>>,
}

impl<T: LinkageMarker> NativeEvents<T> {
    /// Subscribe to one ASCII event name (1..=127 bytes, excluding NUL).
    /// The initial callback establishes the baseline; it is not proof that
    /// an application transaction has just committed.
    pub fn subscribe(
        mut client: NativeFbClient<T>,
        config: &NativeFbAttachmentConfig,
        dialect: Dialect,
        name: &str,
    ) -> Result<Self, FbError> {
        if name.is_empty() || !name.is_ascii() || name.len() > 127 || name.as_bytes().contains(&0) {
            return Err("Invalid event name".into());
        }
        let (sender, receiver) = mpsc::channel();
        let mut buffer = vec![1, name.len() as u8];
        buffer.extend(name.as_bytes());
        buffer.extend([0; 4]);
        let database = client.attach_database(config, dialect, false)?;
        let mut events = Self {
            client: ManuallyDrop::new(client),
            database,
            id: 0,
            buffer,
            context: ManuallyDrop::new(Box::new(EventContext(sender))),
            receiver,
        };
        events.queue()?;
        Ok(events)
    }

    fn queue(&mut self) -> Result<(), FbError> {
        use crate::ibase::IBase;
        let code = unsafe {
            self.client.ibase.isc_que_events()(
                &mut self.client.status[0],
                &mut self.database,
                &mut self.id,
                self.buffer.len() as i16,
                self.buffer.as_ptr(),
                Some(callback),
                (&mut **self.context as *mut EventContext).cast(),
            )
        };
        if code != 0 {
            return Err(self.client.status.as_error(&self.client.ibase));
        }
        Ok(())
    }

    /// True on a notification, including the initial Firebird baseline callback.
    /// Re-arms before returning, so the caller may safely re-read the database.
    pub fn wait(&mut self, duration: Duration) -> Result<bool, FbError> {
        match self.receiver.recv_timeout(duration) {
            Ok(buffer) => {
                if buffer.len() != self.buffer.len() {
                    return Err("Invalid event response".into());
                }
                self.buffer = buffer;
                self.queue()?;
                Ok(true)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(false),
            Err(_) => Err("Event subscription closed".into()),
        }
    }

    /// Health-check the event attachment, without polling application settings.
    pub fn ping(&mut self) -> Result<(), FbError> {
        database_info(&mut self.client, &mut self.database, 12).map(|_| ())
    }
}

impl<T: LinkageMarker> Drop for NativeEvents<T> {
    fn drop(&mut self) {
        use crate::ibase::IBase;
        if self.id != 0 {
            unsafe {
                self.client.ibase.isc_cancel_events()(
                    &mut self.client.status[0],
                    &mut self.database,
                    &mut self.id,
                );
            }
        }
        if self.client.detach_database(&mut self.database).is_ok() {
            // Detach synchronizes with the native event thread. If it fails,
            // retain both callback storage and the loaded library: a late
            // callback must never access freed memory or unloaded client code.
            unsafe {
                ManuallyDrop::drop(&mut self.context);
                ManuallyDrop::drop(&mut self.client);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(all(target_os = "windows", target_pointer_width = "32"))]
    fn win32_firebird_header_layout() {
        assert_eq!(std::mem::size_of::<ibase::ISC_STATUS>(), 4);
        assert_eq!(std::mem::size_of::<ibase::isc_db_handle>(), 4);
        assert_eq!(std::mem::size_of::<IscTeb>(), 12);
        assert_eq!(std::mem::size_of::<ibase::XSQLVAR>(), 152);
    }
    #[test]
    fn limbo_parser_rejects_truncation_and_invalid_ids() {
        assert_eq!(parse_limbo(&[16, 4, 0, 7, 0, 0, 0, 1]).unwrap(), vec![7]);
        assert!(parse_limbo(&[1]).unwrap().is_empty());
        for bytes in [
            &[2][..],
            &[16, 5, 0, 1, 2, 3, 4, 5, 1],
            &[16, 4, 0, 1],
            &[16, 0, 0, 1],
            &[],
        ] {
            assert!(parse_limbo(bytes).is_err());
        }
    }
}
