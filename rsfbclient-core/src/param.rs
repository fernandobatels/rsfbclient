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
}

/// Implemented for types that can be sent as parameters
pub trait ToParam {
    fn to_param(self) -> Param;
}

impl ToParam for String {
    fn to_param(self) -> Param {
        Text(self)
    }
}

impl ToParam for i64 {
    fn to_param(self) -> Param {
        Integer(self)
    }
}

/// Implements AsParam for integers
macro_rules! impl_param_int {
    ( $( $t: ident ),+ ) => {
        $(
            impl ToParam for $t {
                fn to_param(self) -> Param {
                    (self as i64).to_param()
                }
            }
        )+
    };
}

impl_param_int!(i32, u32, i16, u16, i8, u8);

impl ToParam for f64 {
    fn to_param(self) -> Param {
        Floating(self)
    }
}

impl ToParam for f32 {
    fn to_param(self) -> Param {
        (self as f64).to_param()
    }
}

/// Implements for all nullable variants
impl<T> ToParam for Option<T>
where
    T: ToParam,
{
    fn to_param(self) -> Param {
        if let Some(v) = self {
            v.to_param()
        } else {
            Null
        }
    }
}

/// Implements for all borrowed variants (&str, Cow and etc)
impl<T, B> ToParam for &B
where
    B: ToOwned<Owned = T> + ?Sized,
    T: core::borrow::Borrow<B> + ToParam,
{
    fn to_param(self) -> Param {
        self.to_owned().to_param()
    }
}

/// Implement From / Into conversions
impl<T> From<T> for Param
where
    T: ToParam,
{
    fn from(param: T) -> Self {
        param.to_param()
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
            $( $t: ToParam, )+
        {
            fn to_params(self) -> Vec<Param> {
                let ( $($v,)+ ) = self;

                vec![ $(
                    $v.to_param(),
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
