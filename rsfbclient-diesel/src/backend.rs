//! The Firebird backend

use crate::query_builder::FbQueryBuilder;
use byteorder::NetworkEndian;
use diesel::backend::Backend;
use diesel::backend::UsesAnsiSavepointSyntax;
use diesel::query_builder::bind_collector::RawBytesBindCollector;
use diesel::sql_types::TypeMetadata;
use rsfbclient::SqlType;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Fb;

impl Backend for Fb {
    type QueryBuilder = FbQueryBuilder;
    type ByteOrder = NetworkEndian;
    type BindCollector = RawBytesBindCollector<Fb>;
    type RawValue = SqlType;
}

impl TypeMetadata for Fb {
    type TypeMetadata = SqlType;
    // TODO: add firebird domains support
    type MetadataLookup = ();
}

impl UsesAnsiSavepointSyntax for Fb {}
