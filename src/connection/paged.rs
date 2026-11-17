//! Fetch consecutive pages from a single execution of a SELECT.
use super::{Connection, FbError, FirebirdClient, StatementData};
use rsfbclient_core::{Row, SqlType, StmtType};
use std::sync::{Arc, Weak};

/// A paged SELECT in the connection and transaction that opened it.
///
/// Pass this cursor to the corresponding connection to fetch or close it.
/// Several cursors may be open in the same transaction. Ending the transaction
/// or disconnecting invalidates them all. Dropping a cursor without closing
/// it releases its statement on the next paged open or at transaction end.
#[must_use = "pass the cursor to fetch_query_page or close_paged_query"]
pub struct PagedQuery {
    id: Arc<()>,
    columns: Vec<String>,
    pending: Option<Result<Row, FbError>>,
    exhausted: bool,
}

impl PagedQuery {
    /// Prepared output column names, including for an empty result.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }
}

// Keep only handles that the connection must release, even if a caller drops
// its cursor or ends the transaction first. Paging state belongs to PagedQuery.
pub(super) struct PagedStatement<C: FirebirdClient> {
    id: Weak<()>,
    statement: StatementData<C>,
}

impl<C: FirebirdClient> Connection<C> {
    /// Open a SELECT cursor in the explicitly active default transaction.
    pub fn open_paged_query(
        &mut self,
        sql: &str,
        params: Vec<SqlType>,
    ) -> Result<PagedQuery, FbError> {
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
        Ok(PagedQuery {
            id,
            columns,
            pending: None,
            exhausted: false,
        })
    }

    /// Fetch at most `limit` new rows; retain one lookahead row for the next call.
    /// The flag indicates that another call can return a row or a pending error.
    /// A terminal fetch error is returned after any rows already collected.
    /// Reporting the error releases the cursor without re-executing its SELECT.
    pub fn fetch_query_page(
        &mut self,
        cursor: &mut PagedQuery,
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

    /// Close this cursor and free its prepared statement.
    pub fn close_paged_query(&mut self, cursor: &mut PagedQuery) -> Result<(), FbError> {
        let index = self.paged_statement_index(cursor)?;
        cursor.exhausted = true;
        cursor.pending = None;
        self.paged_statements
            .swap_remove(index)
            .statement
            .close(self)
    }

    fn paged_statement_index(&self, cursor: &PagedQuery) -> Result<usize, FbError> {
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
