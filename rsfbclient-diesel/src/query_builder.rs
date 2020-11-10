//! The Firebird query builder

use super::backend::Fb;
use diesel::query_builder::*;
use diesel::{QueryResult, QuerySource};

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

impl<L> QueryFragment<Fb> for LimitOffsetClause<LimitClause<L>, NoOffsetClause>
where
    L: QueryFragment<Fb>,
{
    fn walk_ast(&self, mut out: AstPass<Fb>) -> QueryResult<()> {
        out.push_sql(" FIRST ");
        self.limit_clause.0.walk_ast(out.reborrow())?;
        out.push_sql(" ");
        Ok(())
    }
}

impl<L> QueryFragment<Fb> for LimitOffsetClause<LimitClause<L>, OffsetClause<L>>
where
    L: QueryFragment<Fb>,
{
    fn walk_ast(&self, mut out: AstPass<Fb>) -> QueryResult<()> {
        out.push_sql(" FIRST ");
        self.limit_clause.0.walk_ast(out.reborrow())?;
        out.push_sql(" SKIP ");
        self.offset_clause.0.walk_ast(out.reborrow())?;
        out.push_sql(" ");
        Ok(())
    }
}

impl<F, S, D, W, O, LOf, G, LC> QueryFragment<Fb> for SelectStatement<F, S, D, W, O, LOf, G, LC>
where
    S: SelectClauseQueryFragment<F, Fb>,
    F: QuerySource,
    F::FromClause: QueryFragment<Fb>,
    D: QueryFragment<Fb>,
    W: QueryFragment<Fb>,
    O: QueryFragment<Fb>,
    LOf: QueryFragment<Fb>,
    G: QueryFragment<Fb>,
    LC: QueryFragment<Fb>,
{
    fn walk_ast(&self, mut out: AstPass<Fb>) -> QueryResult<()> {
        out.push_sql("SELECT ");
        self.limit_offset.walk_ast(out.reborrow())?;
        self.distinct.walk_ast(out.reborrow())?;
        self.select.walk_ast(&self.from, out.reborrow())?;
        out.push_sql(" FROM ");
        self.from.from_clause().walk_ast(out.reborrow())?;
        self.where_clause.walk_ast(out.reborrow())?;
        self.group_by.walk_ast(out.reborrow())?;
        self.order.walk_ast(out.reborrow())?;
        self.locking.walk_ast(out.reborrow())?;
        Ok(())
    }
}
