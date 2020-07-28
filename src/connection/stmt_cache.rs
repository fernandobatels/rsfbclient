//!
//! Rust Firebird Client
//!
//! Statement Cache
//!

use std::{collections::HashMap, mem};

use crate::{statement::StatementData, transaction::TransactionData, Connection, FbError};

/// Cache of prepared statements.
///
/// Must be emptied by calling `close_all` before dropping.
pub struct StmtCache {
    cache: HashMap<String, StatementData>,
}

pub struct StmtCacheData {
    pub(crate) sql: String,
    pub(crate) stmt: StatementData,
}

impl StmtCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: HashMap::with_capacity(capacity),
        }
    }

    /// Get a prepared statement from the cache, or prepare one
    pub fn get(
        &mut self,
        conn: &Connection,
        tr: &mut TransactionData,
        sql: &str,
    ) -> Result<StmtCacheData, FbError> {
        if let Some((sql, stmt)) = self.cache.remove_entry(sql) {
            Ok(StmtCacheData { sql, stmt })
        } else {
            Ok(StmtCacheData {
                sql: sql.to_string(),
                stmt: StatementData::prepare(conn, tr, sql)?,
            })
        }
    }

    /// Adds a prepared statement to the cache, closing the previous one for this sql
    /// or another if the cache is full
    pub fn insert(&mut self, conn: &Connection, data: StmtCacheData) -> Result<(), FbError> {
        if let Some(mut stmt) = self.cache.insert(data.sql, data.stmt) {
            stmt.close(conn)?;
        }

        Ok(())
    }

    /// Closes all statements in the cache.
    /// Needs to be called before dropping the cache.
    pub fn close_all(&mut self, conn: &Connection) {
        let cache = mem::replace(&mut self.cache, HashMap::new());

        for (_, mut stmt) in cache.into_iter() {
            stmt.close(conn).ok();
        }
    }
}
