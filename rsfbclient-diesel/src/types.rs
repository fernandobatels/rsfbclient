//! Types implementation of Firebird support

use super::backend::Fb;
use bytes::Buf;
use bytes::Bytes;
use diesel::sql_types::{self, HasSqlType};
use rsfbclient::SqlType;

/// Supported types by the diesel
/// Firebird implementation
pub enum SupportedType {
    Text,
    SmallInt,
    Int,
    BigInt,
    Float,
    Double
}

impl SupportedType {
    pub fn to_param(self, source_val: Option<Vec<u8>>) -> SqlType {
        if let Some(val) = source_val {
            match self {
                SupportedType::Text => {
                    SqlType::Text(String::from_utf8(val).expect("Invalid UTF-8"))
                }
                SupportedType::SmallInt => {
                    SqlType::Integer(Bytes::copy_from_slice(&val).get_i16().into())
                }
                SupportedType::Int => {
                    SqlType::Integer(Bytes::copy_from_slice(&val).get_i32().into())
                }
                SupportedType::BigInt => {
                    SqlType::Integer(Bytes::copy_from_slice(&val).get_i64().into())
                }
                SupportedType::Float => {
                    SqlType::Floating(Bytes::copy_from_slice(&val).get_f32().into())
                }
                SupportedType::Double => {
                    SqlType::Floating(Bytes::copy_from_slice(&val).get_f64().into())
                }
            }
        } else {
            SqlType::Null
        }
    }
}

impl HasSqlType<sql_types::SmallInt> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::SmallInt
    }
}

impl HasSqlType<sql_types::Integer> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Int
    }
}

impl HasSqlType<sql_types::BigInt> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::BigInt
    }
}

impl HasSqlType<sql_types::Float> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Float
    }
}

impl HasSqlType<sql_types::Double> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Double
    }
}

impl HasSqlType<sql_types::VarChar> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        SupportedType::Text
    }
}

impl HasSqlType<sql_types::Binary> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::Date> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::Time> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::Timestamp> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}
