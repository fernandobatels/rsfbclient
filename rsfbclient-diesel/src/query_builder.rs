//! The Firebird query builder

use super::backend::Fb;
use diesel::query_builder::*;
use diesel::QueryResult;

pub struct FbQueryBuilder {
    query: String,
}

impl FbQueryBuilder {
    pub fn new() -> Self {
        FbQueryBuilder {
            query: String::new(),
        }
    }
}

impl QueryBuilder<Fb> for FbQueryBuilder {
    fn push_sql(&mut self, sql: &str) {
        self.query.push_str(sql);
    }

    fn push_identifier(&mut self, identifier: &str) -> QueryResult<()> {
        self.query.push_str(identifier);

        Ok(())
    }

    fn push_bind_param(&mut self) {
        self.query.push_str("?");
    }

    fn finish(self) -> String {
        self.query
    }
}

impl QueryFragment<Fb> for LimitOffsetClause<NoLimitClause, NoOffsetClause> {
    fn walk_ast(&self, _out: AstPass<Fb>) -> QueryResult<()> {
        Ok(())
    }
}
