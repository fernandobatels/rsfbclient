//! Sql parameter types and traits

use crate::{error::FbError, ibase, SqlType};
use regex::Regex;
use std::collections::HashMap;

pub use SqlType::*;

/// Max length that can be sent without creating a BLOB
pub const MAX_TEXT_LENGTH: usize = 32767;

impl SqlType {
    /// Return the sql type to coerce the data
    pub fn sql_type_and_subtype(&self) -> (u32, u32) {
        match self {
            Text(s) => {
                if s.len() > MAX_TEXT_LENGTH {
                    (ibase::SQL_BLOB + 1, 1)
                } else {
                    (ibase::SQL_TEXT + 1, 0)
                }
            }
            Integer(_) => (ibase::SQL_INT64 + 1, 0),
            Floating(_) => (ibase::SQL_DOUBLE + 1, 0),
            Timestamp(_) => (ibase::SQL_TIMESTAMP + 1, 0),
            Null => (ibase::SQL_TEXT + 1, 0),
            Binary(_) => (ibase::SQL_BLOB + 1, 0),
            Boolean(_) => (ibase::SQL_BOOLEAN + 1, 0),
        }
    }
}

/// Implemented for types that can be sent as parameters
pub trait IntoParam {
    fn into_param(self) -> SqlType;
}

impl IntoParam for Vec<u8> {
    fn into_param(self) -> SqlType {
        Binary(self)
    }
}

impl IntoParam for String {
    fn into_param(self) -> SqlType {
        Text(self)
    }
}

impl IntoParam for i64 {
    fn into_param(self) -> SqlType {
        Integer(self)
    }
}

impl IntoParam for bool {
    fn into_param(self) -> SqlType {
        Boolean(self)
    }
}

/// Implements AsParam for integers
macro_rules! impl_param_int {
    ( $( $t: ident ),+ ) => {
        $(
            impl IntoParam for $t {
                fn into_param(self) -> SqlType {
                    (self as i64).into_param()
                }
            }
        )+
    };
}

impl_param_int!(i32, u32, i16, u16, i8, u8);

impl IntoParam for f64 {
    fn into_param(self) -> SqlType {
        Floating(self)
    }
}

impl IntoParam for f32 {
    fn into_param(self) -> SqlType {
        (self as f64).into_param()
    }
}

/// Implements for all nullable variants
impl<T> IntoParam for Option<T>
where
    T: IntoParam,
{
    fn into_param(self) -> SqlType {
        if let Some(v) = self {
            v.into_param()
        } else {
            Null
        }
    }
}

/// Implements for all borrowed variants (&str, Cow and etc)
impl<T, B> IntoParam for &B
where
    B: ToOwned<Owned = T> + ?Sized,
    T: core::borrow::Borrow<B> + IntoParam,
{
    fn into_param(self) -> SqlType {
        self.to_owned().into_param()
    }
}

/// Implement From / Into conversions
impl<T> From<T> for SqlType
where
    T: IntoParam,
{
    fn from(param: T) -> Self {
        param.into_param()
    }
}

pub enum ParamsType {
    Unamed(Vec<SqlType>),

    Named(HashMap<String, SqlType>),
}

/// Implemented for types that represents a list of parameters
pub trait IntoParams {
    fn to_params(self) -> ParamsType;
}

/// Allow use of a vector instead of tuples, for when the number of parameters are unknow at compile time
/// or more parameters are needed than what can be used with the tuples
impl IntoParams for Vec<SqlType> {
    fn to_params(self) -> ParamsType {
        ParamsType::Unamed(self)
    }
}

/// Represents no parameters
impl IntoParams for () {
    fn to_params(self) -> ParamsType {
        ParamsType::Unamed(vec![])
    }
}

/// Generates IntoParams implementations for a tuple
macro_rules! impl_into_params {
    ($([$t: ident, $v: ident]),+) => {
        impl<$($t),+> IntoParams for ($($t,)+)
        where
            $( $t: IntoParam, )+
        {
            fn to_params(self) -> ParamsType {
                let ( $($v,)+ ) = self;

                ParamsType::Unamed(vec![ $(
                    $v.into_param(),
                )+ ])
            }
        }
    };
}

/// Generates FromRow implementations for various tuples
macro_rules! impls_into_params {
    ([$t: ident, $v: ident]) => {
        impl_into_params!([$t, $v]);
    };

    ([$t: ident, $v: ident], $([$ts: ident, $vs: ident]),+ ) => {
        impls_into_params!($([$ts, $vs]),+);

        impl_into_params!([$t, $v], $([$ts, $vs]),+);
    };
}

impls_into_params!(
    [A, a],
    [B, b],
    [C, c],
    [D, d],
    [E, e],
    [F, f],
    [G, g],
    [H, h],
    [I, i],
    [J, j],
    [K, k],
    [L, l],
    [M, m],
    [N, n],
    [O, o]
);

/// Named params implementation.
///
/// Works on top of firebird unamed
/// params '?'
pub struct NamedParams {
    pub sql: String,
    params_names: Vec<String>,
}

impl NamedParams {
    /// Parse the sql statement and return a
    /// named params instance
    pub fn parse(raw_sql: &str) -> Result<Self, FbError> {
        let rparams = Regex::new(r"(:[a-zA-Z0-9]{1,})")
            .map_err(|e| FbError::from(format!("Error on start the regex for named params: {}", e)))
            .unwrap();

        let mut params_names = vec![];
        let sql = rparams.replace_all(raw_sql, "?").to_string();

        for param in rparams.captures_iter(raw_sql) {
            params_names.push(param[1].to_string().replace(":", ""));
        }

        Ok(NamedParams { sql, params_names })
    }

    /// Re-sort/convert the params applying
    /// the named params support
    pub fn convert<P>(&self, params: P) -> Result<Vec<SqlType>, FbError>
    where
        P: IntoParams,
    {
        match params.to_params() {
            ParamsType::Named(names) => {
                let mut new_params = vec![];

                for qname in &self.params_names {
                    if let Some(param) = names.get(qname) {
                        new_params.push(param.clone());
                    } else {
                        return Err(FbError::from(format!(
                            "Param :{} not found in the provided struct",
                            qname
                        )));
                    }
                }

                Ok(new_params)
            }
            ParamsType::Unamed(p) => Ok(p),
        }
    }
}
