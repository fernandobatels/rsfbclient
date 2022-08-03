//! The Firebird backend

use super::query_builder::FbQueryBuilder;
use super::types::SupportedType;
use super::value::FbValue;
use diesel::backend::*;
use diesel::query_builder::bind_collector::RawBytesBindCollector;
use diesel::sql_types::TypeMetadata;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Fb;

impl Backend for Fb {
    type QueryBuilder = FbQueryBuilder;
}

impl TrustedBackend for Fb {}
impl DieselReserveSpecialization for Fb {}

impl<'a> HasBindCollector<'a> for Fb {
    type BindCollector = RawBytesBindCollector<Fb>;
}

impl<'a> HasRawValue<'a> for Fb {
    type RawValue = FbValue<'a>;
}

impl TypeMetadata for Fb {
    type TypeMetadata = SupportedType;
    // TODO: add firebird domains support
    type MetadataLookup = ();
}

pub struct FbSelectStatementSyntax;

#[derive(Debug, Copy, Clone)]
pub struct FbReturningClause;

impl SqlDialect for Fb {
    type ReturningClause = FbReturningClause;

    type OnConflictClause = sql_dialect::on_conflict_clause::DoesNotSupportOnConflictClause;

    type InsertWithDefaultKeyword =
        sql_dialect::default_keyword_for_insert::DoesNotSupportDefaultKeyword;

    type BatchInsertSupport = sql_dialect::batch_insert_support::DoesNotSupportBatchInsert;

    type DefaultValueClauseForInsert = sql_dialect::default_value_clause::AnsiDefaultValueClause;

    type EmptyFromClauseSyntax = sql_dialect::from_clause_syntax::AnsiSqlFromClauseSyntax;

    type ExistsSyntax = sql_dialect::exists_syntax::AnsiSqlExistsSyntax;

    type ArrayComparison = sql_dialect::array_comparison::AnsiSqlArrayComparison;
    type SelectStatementSyntax = FbSelectStatementSyntax;
}
