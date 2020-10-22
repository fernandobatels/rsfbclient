//! The Firebird query builder

use super::backend::Fb;
use diesel::query_builder::QueryBuilder;
use diesel::QueryResult;

pub struct FbQueryBuilder;

#[allow(unused_variables)]
impl QueryBuilder<Fb> for FbQueryBuilder {
    fn push_sql(&mut self, sql: &str) {
        todo!()
    }

    fn push_identifier(&mut self, identifier: &str) -> QueryResult<()> {
        todo!()
    }

    fn push_bind_param(&mut self) {
        todo!()
    }

    fn finish(self) -> String {
        todo!()
    }
}
