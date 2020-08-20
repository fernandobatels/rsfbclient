//! Sql parameter types and traits

use crate::ibase;

use Param::*;

/// Sql parameter data
pub enum Param {
    Text(String),

    Integer(i64),

    Floating(f64),

    Timestamp(ibase::ISC_TIMESTAMP),

    Null,

    Binary(Vec<u8>),
}

impl Param {
    /// Return the sql type to coerce the data
    pub fn sql_type(&self) -> u32 {
        match self {
            Text(_) => ibase::SQL_TEXT + 1,
            Integer(_) => ibase::SQL_INT64 + 1,
            Floating(_) => ibase::SQL_DOUBLE + 1,
            Timestamp(_) => ibase::SQL_TIMESTAMP + 1,
            Null => ibase::SQL_TEXT + 1,
            Binary(_) => ibase::SQL_BLOB + 1,
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
        Integer(self)
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
