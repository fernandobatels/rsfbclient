//! The Firebird backend

use crate::query_builder::FbQueryBuilder;
use crate::types::SupportedType;
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
    type RawValue = SupportedType;
}

impl TypeMetadata for Fb {
    type TypeMetadata = SupportedType;
    // TODO: add firebird domains support
    type MetadataLookup = ();
}

impl UsesAnsiSavepointSyntax for Fb {}
impl SupportsDefaultKeyword for Fb {}
impl SupportsReturningClause for Fb {}
