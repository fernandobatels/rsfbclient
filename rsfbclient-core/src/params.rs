//! Sql parameter types and traits

use crate::error::FbError;
use crate::ibase;

use regex::Regex;
use Param::*;

/// Max length that can be sent without creating a BLOB
pub const MAX_TEXT_LENGTH: usize = 32767;

/// Sql parameter data
pub enum Param {
    Text(String),

    Integer(i64, Option<String>),

    Floating(f64),

    Timestamp(ibase::ISC_TIMESTAMP),

    Null,

    Binary(Vec<u8>),

    /// Only works in fb >= 3.0
    Boolean(bool),
}

impl Param {
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
            Integer(_, _) => (ibase::SQL_INT64 + 1, 0),
            Floating(_) => (ibase::SQL_DOUBLE + 1, 0),
            Timestamp(_) => (ibase::SQL_TIMESTAMP + 1, 0),
            Null => (ibase::SQL_TEXT + 1, 0),
            Binary(_) => (ibase::SQL_BLOB + 1, 0),
            Boolean(_) => (ibase::SQL_BOOLEAN + 1, 0),
        }
    }

    /// Return true if null
    pub fn is_null(&self) -> bool {
        if let Self::Null = self {
            true
        } else {
            false
        }
    }
}

/// Implemented for types that can be sent as parameters
pub trait IntoParam {
    fn into_param(self) -> Param;
}

impl IntoParam for Vec<u8> {
    fn into_param(self) -> Param {
        Binary(self)
    }
}

impl IntoParam for String {
    fn into_param(self) -> Param {
        Text(self)
    }
}

impl IntoParam for i64 {
    fn into_param(self) -> Param {
        Integer(self, None)
    }
}

impl IntoParam for bool {
    fn into_param(self) -> Param {
        Boolean(self)
    }
}

/// Implements AsParam for integers
macro_rules! impl_param_int {
    ( $( $t: ident ),+ ) => {
        $(
            impl IntoParam for $t {
                fn into_param(self) -> Param {
                    (self as i64).into_param()
                }
            }
        )+
    };
}

impl_param_int!(i32, u32, i16, u16, i8, u8);

impl IntoParam for f64 {
    fn into_param(self) -> Param {
        Floating(self)
    }
}

impl IntoParam for f32 {
    fn into_param(self) -> Param {
        (self as f64).into_param()
    }
}

/// Implements for all nullable variants
impl<T> IntoParam for Option<T>
where
    T: IntoParam,
{
    fn into_param(self) -> Param {
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
    fn into_param(self) -> Param {
        self.to_owned().into_param()
    }
}

/// Implement From / Into conversions
impl<T> From<T> for Param
where
    T: IntoParam,
{
    fn from(param: T) -> Self {
        param.into_param()
    }
}

/// Implemented for types that represents a list of parameters
pub trait IntoParams {
    fn to_params(self) -> Vec<Param>;
}

/// Allow use of a vector instead of tuples, for when the number of parameters are unknow at compile time
/// or more parameters are needed than what can be used with the tuples
impl IntoParams for Vec<Param> {
    fn to_params(self) -> Vec<Param> {
        self
    }
}

/// Represents no parameters
impl IntoParams for () {
    fn to_params(self) -> Vec<Param> {
        vec![]
    }
}

/// Generates IntoParams implementations for a tuple
macro_rules! impl_into_params {
    ($([$t: ident, $v: ident]),+) => {
        impl<$($t),+> IntoParams for ($($t,)+)
        where
            $( $t: IntoParam, )+
        {
            fn to_params(self) -> Vec<Param> {
                let ( $($v,)+ ) = self;

                vec![ $(
                    $v.into_param(),
                )+ ]
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

pub struct NamedParams {}

pub struct NamedParamPosition(String, usize);

impl NamedParams {
    /// Extract the named params and prepare
    /// the sql query to be used by firebird
    pub fn extract(raw_sql: &str) -> Result<(String, Vec<NamedParamPosition>), FbError> {
        let rparams = Regex::new(r"(:[a-zA-Z]{1,})")
            .map_err(|e| FbError::from(format!("Error on start the regex for named params: {}", e)))
            .unwrap();

        let mut pinfos = vec![];
        let sql = rparams.replace_all(raw_sql, "?").to_string();

        let mut i = 0;
        for param in rparams.captures_iter(raw_sql) {
            pinfos.push(NamedParamPosition(param[1].to_string(), i as usize));
            i = i + 1;
        }

        Ok((sql, pinfos))
    }

    /// Re-sort the params applying the named
    /// params support
    pub fn resort<P>(params: P, infos: Vec<NamedParamPosition>) -> Vec<Param>
    where
        P: IntoParams,
    {
        todo!();
    }
}
