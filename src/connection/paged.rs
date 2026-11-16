//! A connection-owned SELECT cursor that survives individual page requests.
use super::{Connection, FbError, FirebirdClient, StatementData};
use rsfbclient_core::{Row, SqlType, StmtType};

pub(crate) struct PagedQuery<C: FirebirdClient> {
    statement: StatementData<C>,
    pending: Option<Result<Row, FbError>>,
    exhausted: bool,
}

impl<C: FirebirdClient> Connection<C> {
    /// Open one SELECT cursor in the explicitly active default transaction.
    /// Replaces the previous paged cursor without reusing or re-executing it.
    pub fn open_paged_query(
        &mut self,
        sql: &str,
        params: Vec<SqlType>,
    ) -> Result<Vec<String>, FbError> {
        self.close_paged_query()?;
        if !self.in_transaction {
            return Err("Paged queries require an explicitly active transaction".into());
        }
        let (cursor, columns) = self.use_transaction(self.def_confs_tr, |tr| {
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
            Ok((
                PagedQuery {
                    statement,
                    pending: None,
                    exhausted: false,
                },
                columns,
            ))
        })?;
        self.paged_query = Some(cursor);
        Ok(columns)
    }

    /// Fetch at most `limit` new rows; retain one lookahead row for the next call.
    /// The flag indicates that another call can return a row or a pending error.
    /// A terminal fetch error is returned after any rows already collected.
    /// Reporting the error releases the cursor without re-executing its SELECT.
    pub fn fetch_query_page(&mut self, limit: usize) -> Result<(Vec<Row>, bool), FbError> {
        if limit == 0 {
            return Err("The query page size must be greater than zero".into());
        }
        let mut cursor = self
            .paged_query
            .take()
            .ok_or("The SELECT cursor is no longer open")?;
        if cursor.exhausted {
            self.paged_query = Some(cursor);
            return Err("The SELECT cursor is no longer open".into());
        }
        let result = self.use_transaction(self.def_confs_tr, |tr| {
            let mut rows = Vec::new();
            while rows.len() < limit {
                let next = match cursor.pending.take() {
                    Some(row) => row.map(Some),
                    None => cursor
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
            // If the lookahead finds an error, return the completed page first.
            // The next page call reports the error and releases the statement.
            cursor.pending = cursor
                .statement
                .fetch(tr.conn, &mut tr.data)
                .map(|row| row.map(|cols| Row { cols }))
                .transpose();
            Ok((rows, cursor.pending.is_some()))
        });
        match result {
            Ok((rows, true)) => {
                self.paged_query = Some(cursor);
                Ok((rows, true))
            }
            Ok((rows, false)) => {
                if let Err(error) = cursor.statement.close_cursor(self) {
                    cursor.statement.close(self).ok();
                    return Err(error);
                }
                // The cursor has ended. Keep this exact prepared handle until
                // replacement/transaction completion so MON$ can report the
                // executed plan, including for an empty result. Never re-fetch
                // or re-prepare the user's SELECT to obtain diagnostics.
                cursor.exhausted = true;
                self.paged_query = Some(cursor);
                Ok((rows, false))
            }
            Err(error) => {
                cursor.statement.close(self).ok();
                Err(error)
            }
        }
    }

    /// Close the retained cursor before ending its transaction or connection.
    pub fn close_paged_query(&mut self) -> Result<(), FbError> {
        if let Some(mut cursor) = self.paged_query.take() {
            cursor.statement.close(self)?;
        }
        Ok(())
    }
}
