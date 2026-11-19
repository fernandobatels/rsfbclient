//! Fetch consecutive pages from a single execution of a SELECT.
use super::{Connection, FbError, FirebirdClient, StatementData};
use rsfbclient_core::{Row, SqlType, StmtType};
use std::sync::{Arc, Weak};

/// A SELECT cursor that borrows the connection for consecutive page reads.
///
/// The statement is closed when the cursor is dropped or explicitly closed.
/// Use [`Self::pause`] to release the borrow while keeping the cursor open.
#[must_use = "dropping the cursor closes its statement"]
pub struct PagedQuery<'a> {
    connection: &'a mut dyn PageConnection,
    state: Option<PausedPagedQuery>,
}

impl<'a> PagedQuery<'a> {
    pub(super) fn new(connection: &'a mut dyn PageConnection, state: PausedPagedQuery) -> Self {
        Self {
            connection,
            state: Some(state),
        }
    }

    /// Prepared output column names, including for an empty result.
    pub fn columns(&self) -> &[String] {
        &self.state.as_ref().unwrap().columns
    }

    /// Fetch up to `limit` rows without re-executing the SELECT.
    ///
    /// The flag indicates that another call can return a row or a pending error.
    /// A terminal fetch error is returned after any rows already collected.
    pub fn fetch(&mut self, limit: usize) -> Result<(Vec<Row>, bool), FbError> {
        self.connection
            .fetch_page(self.state.as_mut().unwrap(), limit)
    }

    /// Release the connection borrow without closing the statement.
    ///
    /// Resume the cursor on the same connection before ending its transaction.
    pub fn pause(mut self) -> PausedPagedQuery {
        self.state.take().unwrap()
    }

    /// Close the cursor and release its prepared statement.
    pub fn close(mut self) -> Result<(), FbError> {
        self.close_statement()
    }

    fn close_statement(&mut self) -> Result<(), FbError> {
        self.state
            .take()
            .map_or(Ok(()), |mut state| self.connection.close_page(&mut state))
    }
}

impl Drop for PagedQuery<'_> {
    fn drop(&mut self) {
        let _ = self.close_statement();
    }
}

/// A paused SELECT cursor that can be resumed on its original connection.
///
/// A commit, rollback or disconnect invalidates it. If the token is dropped
/// without resuming it, the statement is released on the next paged open or
/// at transaction end.
#[must_use = "resume this cursor on its original connection"]
pub struct PausedPagedQuery {
    id: Arc<()>,
    columns: Vec<String>,
    pending: Option<Result<Row, FbError>>,
    exhausted: bool,
}

pub(super) trait PageConnection: Send {
    fn start_page(&mut self, sql: &str, params: Vec<SqlType>) -> Result<PausedPagedQuery, FbError>;
    fn fetch_page(
        &mut self,
        cursor: &mut PausedPagedQuery,
        limit: usize,
    ) -> Result<(Vec<Row>, bool), FbError>;
    fn close_page(&mut self, cursor: &mut PausedPagedQuery) -> Result<(), FbError>;
}

// Keep only handles that the connection must release, even if a caller drops
// its cursor or ends the transaction first. Paging state belongs to PagedQuery.
pub(super) struct PagedStatement<C: FirebirdClient> {
    id: Weak<()>,
    statement: StatementData<C>,
}

impl<C: FirebirdClient> Connection<C> {
    /// Open a SELECT cursor in the explicitly active default transaction.
    pub fn query_paged(
        &mut self,
        sql: &str,
        params: Vec<SqlType>,
    ) -> Result<PagedQuery<'_>, FbError> {
        let state = self.start_page(sql, params)?;
        Ok(self.resume_paged_query(state))
    }

    /// Resume a cursor paused on this connection.
    ///
    /// Fetching a cursor from a different connection or an ended transaction
    /// reports that the cursor is closed.
    pub fn resume_paged_query(&mut self, cursor: PausedPagedQuery) -> PagedQuery<'_> {
        PagedQuery::new(self, cursor)
    }
}

impl<C: FirebirdClient> PageConnection for Connection<C> {
    fn start_page(&mut self, sql: &str, params: Vec<SqlType>) -> Result<PausedPagedQuery, FbError> {
        if !self.in_transaction {
            return Err("Paged queries require an explicitly active transaction".into());
        }
        self.close_abandoned_paged_statements()?;

        let (statement, columns) = self.use_transaction(self.def_confs_tr, |tr| {
            let mut statement = StatementData::prepare(tr.conn, &mut tr.data, sql, false)?;
            let result = if matches!(
                statement.stmt_type,
                StmtType::Select | StmtType::SelectForUpd
            ) {
                statement.query(tr.conn, &mut tr.data, params)
            } else {
                Err("Paged queries require a SELECT statement".into())
            };
            if let Err(error) = result {
                statement.close(tr.conn).ok();
                return Err(error);
            }
            let columns = tr.conn.cli.output_column_names(&statement.handle);
            Ok((statement, columns))
        })?;
        let id = Arc::new(());
        self.paged_statements.push(PagedStatement {
            id: Arc::downgrade(&id),
            statement,
        });
        Ok(PausedPagedQuery {
            id,
            columns,
            pending: None,
            exhausted: false,
        })
    }

    fn fetch_page(
        &mut self,
        cursor: &mut PausedPagedQuery,
        limit: usize,
    ) -> Result<(Vec<Row>, bool), FbError> {
        if limit == 0 {
            return Err("The query page size must be greater than zero".into());
        }
        if cursor.exhausted {
            return Err("The SELECT cursor is no longer open".into());
        }
        let index = self.paged_statement_index(cursor)?;
        let mut entry = self.paged_statements.swap_remove(index);
        let result = self.use_transaction(self.def_confs_tr, |tr| {
            let mut rows = Vec::new();
            while rows.len() < limit {
                let next = match cursor.pending.take() {
                    Some(row) => row.map(Some),
                    None => entry
                        .statement
                        .fetch(tr.conn, &mut tr.data)
                        .map(|row| row.map(|cols| Row { cols })),
                };
                match next {
                    Ok(Some(row)) => rows.push(row),
                    Ok(None) => return Ok((rows, false)),
                    Err(error) if rows.is_empty() => return Err(error),
                    Err(error) => {
                        cursor.pending = Some(Err(error));
                        return Ok((rows, true));
                    }
                }
            }
            // Deliver a completed page before any error found during lookahead.
            cursor.pending = entry
                .statement
                .fetch(tr.conn, &mut tr.data)
                .map(|row| row.map(|cols| Row { cols }))
                .transpose();
            Ok((rows, cursor.pending.is_some()))
        });
        match result {
            Ok((rows, more)) => {
                if !more {
                    cursor.exhausted = true;
                    if let Err(error) = entry.statement.close_cursor(self) {
                        entry.statement.close(self).ok();
                        return Err(error);
                    }
                    // Keep the executed statement available to MON$ until the
                    // caller closes it, even for an empty result.
                }
                self.paged_statements.push(entry);
                Ok((rows, more))
            }
            Err(error) => {
                cursor.exhausted = true;
                entry.statement.close(self).ok();
                Err(error)
            }
        }
    }

    fn close_page(&mut self, cursor: &mut PausedPagedQuery) -> Result<(), FbError> {
        let index = self.paged_statement_index(cursor)?;
        cursor.exhausted = true;
        cursor.pending = None;
        self.paged_statements
            .swap_remove(index)
            .statement
            .close(self)
    }
}

impl<C: FirebirdClient> Connection<C> {
    fn paged_statement_index(&self, cursor: &PausedPagedQuery) -> Result<usize, FbError> {
        self.paged_statements
            .iter()
            .position(|entry| entry.id.as_ptr() == Arc::as_ptr(&cursor.id))
            .ok_or_else(|| "The SELECT cursor is no longer open on this connection".into())
    }

    fn close_abandoned_paged_statements(&mut self) -> Result<(), FbError> {
        let mut index = 0;
        while index < self.paged_statements.len() {
            if self.paged_statements[index].id.strong_count() == 0 {
                self.paged_statements
                    .swap_remove(index)
                    .statement
                    .close(self)?;
            } else {
                index += 1;
            }
        }
        Ok(())
    }

    pub(super) fn close_paged_statements(&mut self) {
        for mut entry in std::mem::take(&mut self.paged_statements) {
            entry.statement.close(self).ok();
        }
    }
}
