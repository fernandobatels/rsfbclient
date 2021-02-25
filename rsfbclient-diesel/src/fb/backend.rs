//! The Firebird backend

use super::query_builder::FbQueryBuilder;
use super::types::SupportedType;
use super::value::FbValue;
use byteorder::NetworkEndian;
use diesel::backend::*;
use diesel::query_builder::bind_collector::RawBytesBindCollector;
use diesel::sql_types::TypeMetadata;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Fb;

impl Backend for Fb {
    type QueryBuilder = FbQueryBuilder;
    type ByteOrder = NetworkEndian;
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

impl UsesAnsiSavepointSyntax for Fb {}
impl SupportsReturningClause for Fb {}
