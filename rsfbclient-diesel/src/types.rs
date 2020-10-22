//! Types implementation of Firebird support

use super::backend::Fb;
use diesel::sql_types::{self, HasSqlType};
use rsfbclient::SqlType;

pub struct FbValue;

impl HasSqlType<sql_types::SmallInt> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::Integer> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::BigInt> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::Float> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::Double> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
    }
}

impl HasSqlType<sql_types::VarChar> for Fb {
    fn metadata(_: &Self::MetadataLookup) -> Self::TypeMetadata {
        todo!()
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
