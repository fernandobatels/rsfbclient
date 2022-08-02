//! The Firebird query builder

use super::backend::{Fb,FbReturningClause};
use diesel::AppearsOnTable;
use diesel::Column;
use diesel::Expression;
use diesel::insertable::ColumnInsertValue;
use diesel::insertable::DefaultableColumnInsertValue;
use diesel::insertable::InsertValues;
use diesel::query_builder::*;
use diesel::QueryResult;

pub struct FbQueryBuilder {
    query: String,
    pub has_cursor: bool,
}

impl FbQueryBuilder {
    pub fn new() -> Self {
        FbQueryBuilder {
            query: String::new(),
            has_cursor: true,
        }
    }
}

impl Default for FbQueryBuilder {
    fn default() -> Self {
        FbQueryBuilder::new()
    }
}

impl QueryBuilder<Fb> for FbQueryBuilder {
    fn push_sql(&mut self, sql: &str) {
        self.query.push_str(sql);

        if sql.trim().to_lowercase() == "returning" {
            self.has_cursor = false;
        }
    }

    fn push_identifier(&mut self, identifier: &str) -> QueryResult<()> {
        self.query.push_str(identifier);

        Ok(())
    }

    fn push_bind_param(&mut self) {
        self.query.push('?');
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
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Fb>) -> QueryResult<()> {
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
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Fb>) -> QueryResult<()> {
        out.push_sql(" FIRST ");
        self.limit_clause.0.walk_ast(out.reborrow())?;
        out.push_sql(" SKIP ");
        self.offset_clause.0.walk_ast(out.reborrow())?;
        out.push_sql(" ");
        Ok(())
    }
}

impl<F, S, D, W, O, LOf, G, H, LC> QueryFragment<Fb, crate::fb::backend::FbSelectStatementSyntax>
    for SelectStatement<F, S, D, W, O, LOf, G, H, LC>
where
    S: QueryFragment<Fb>,
    F: QueryFragment<Fb>,
    D: QueryFragment<Fb>,
    W: QueryFragment<Fb>,
    O: QueryFragment<Fb>,
    LOf: QueryFragment<Fb>,
    G: QueryFragment<Fb>,
    H: QueryFragment<Fb>,
    LC: QueryFragment<Fb>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Fb>) -> QueryResult<()> {
        out.push_sql("SELECT ");
        self.limit_offset.walk_ast(out.reborrow())?;
        self.distinct.walk_ast(out.reborrow())?;
        self.select.walk_ast(out.reborrow())?;
        self.from.walk_ast(out.reborrow())?;
        self.where_clause.walk_ast(out.reborrow())?;
        self.group_by.walk_ast(out.reborrow())?;
        self.having.walk_ast(out.reborrow())?;
        self.order.walk_ast(out.reborrow())?;
        self.locking.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl<'a, ST, QS, GB> QueryFragment<Fb, crate::fb::backend::FbSelectStatementSyntax>
    for BoxedSelectStatement<'a, ST, QS, Fb, GB>
where
    QS: QueryFragment<Fb>,
    BoxedLimitOffsetClause<'a, Fb>: QueryFragment<Fb>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Fb>) -> QueryResult<()> {
        out.push_sql("SELECT ");
        self.limit_offset.walk_ast(out.reborrow())?;
        self.distinct.walk_ast(out.reborrow())?;
        self.select.walk_ast(out.reborrow())?;
        self.from.walk_ast(out.reborrow())?;
        self.where_clause.walk_ast(out.reborrow())?;
        self.group_by.walk_ast(out.reborrow())?;
        self.having.walk_ast(out.reborrow())?;
        self.order.walk_ast(out.reborrow())?;
        Ok(())
    }
}

impl<Col, Expr> InsertValues<Col::Table, Fb>
    for DefaultableColumnInsertValue<ColumnInsertValue<Col, Expr>>
where
    Col: Column,
    Expr: Expression<SqlType = Col::SqlType> + AppearsOnTable<NoFromClause>,
    Self: QueryFragment<Fb>,
{
    fn column_names(&self, mut out: AstPass<'_, '_, Fb>) -> QueryResult<()> {
        if let Self::Expression(..) = *self {
            out.push_identifier(Col::NAME)?;
        }
        Ok(())
    }
}

impl<Col, Expr>
    QueryFragment<
        Fb,
        crate::backend::sql_dialect::default_keyword_for_insert::DoesNotSupportDefaultKeyword,
    > for DefaultableColumnInsertValue<ColumnInsertValue<Col, Expr>>
where
    Expr: QueryFragment<Fb>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Fb>) -> QueryResult<()> {
        if let Self::Expression(ref inner) = *self {
            inner.walk_ast(out.reborrow())?;
        }
        Ok(())
    }
}

impl<Expr> QueryFragment<Fb, FbReturningClause> for ReturningClause<Expr>
where
    Expr: QueryFragment<Fb>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, Fb>) -> QueryResult<()> {
        out.push_sql(" RETURNING ");
        self.0.walk_ast(out.reborrow())?;
        Ok(())
    }
}
