//! Sql parameter types and traits

use crate::{error::FbError, ibase, SqlType};
use regex::{Captures, Regex};
use std::collections::HashMap;

pub use SqlType::*;

/// Max length that can be sent without creating a BLOB
pub const MAX_TEXT_LENGTH: usize = 32767;

impl SqlType {
    /// Convert the sql value to interbase format
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

/// Implements `IntoParam` for all nullable variants
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

/// Implements `IntoParam` for all borrowed variants (&str, Cow and etc)
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

/// Parameters type
pub enum ParamsType {
    /// Positional parameters, using '?'. This is the default option.
    ///
    /// Firebird provides direct support for this kind of parameter, which this crate makes use of.
    Positional(Vec<SqlType>),

    /// Named parameters, using the common `:`-prefixed `:param` syntax.
    ///
    /// Support for this kind of parameter is provided by this library.
    ///
    /// Currently only a naive regex-based approach is used, to support very basic
    /// select, insert, etc statements
    ///
    /// **CAUTION!**
    /// Named parameter support is still very preliminary.
    /// Use of named parameters may currently give unexpected results. Please test your queries carefully
    /// when using this feature.
    ///
    /// In particular, the simple regex-based parser is known to definitely to have trouble with:
    ///   * occurences of apostrophe (`'`) anywhere except as string literal delimiters (for example, in comments)
    ///   * statements with closed variable bindings (which uses the `:var` syntax) (for example, in PSQL via `EXECUTE BLOCK` or `EXECUTE PROCEDURE`)
    ///
    ///
    /// This crate provides a [derive macro](prelude/derive.IntoParams.html) for supplying arguments via the fields of a struct and their labels.
    Named(HashMap<String, SqlType>),
}

impl ParamsType {
    pub fn named(&self) -> bool {
        match self {
            ParamsType::Positional(_) => false,
            ParamsType::Named(_) => true,
        }
    }
}

/// Types with an associated boolean flag function, `named()` indiciating support for named or positional parameters.
///
///
/// With both named (as a struct field) or positional (as a Vector or tuple element) parameters, `Option<T>`, with `T` an `IntoParam`,  may be used to indicate a nullable argument, wherein the `None` variant provides a `null` value.
///
/// This crate provides a [derive macro](prelude/derive.IntoParams.html) for supplying arguments via the fields of a struct and their labels.
pub trait IntoParams {
    fn to_params(self) -> ParamsType;
}

impl IntoParams for ParamsType {
    fn to_params(self) -> ParamsType {
        self
    }
}

/// Allow use of a vector instead of tuples, for run-time-determined parameter count, or
/// for when there are too many parameters to use one of the provided tuple implementations
impl IntoParams for Vec<SqlType> {
    fn to_params(self) -> ParamsType {
        ParamsType::Positional(self)
    }
}

/// Represents 0 parameters
impl IntoParams for () {
    fn to_params(self) -> ParamsType {
        ParamsType::Positional(vec![])
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

                ParamsType::Positional(vec![ $(
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
/// Works on top of firebird positional parameters (`?`)
pub struct NamedParams {
    pub sql: String,
    params_names: Vec<String>,
}

impl NamedParams {
    /// Parse the sql statement and return a
    /// structure representing the named parameters found
    pub fn parse(raw_sql: &str) -> Result<Self, FbError> {
        let rparams = Regex::new(r#"('[^']*')|:\w+"#)
            .map_err(|e| FbError::from(format!("Error on start the regex for named params: {}", e)))
            .unwrap();

        let mut params_names = vec![];
        let sql = rparams
            .replace_all(raw_sql, |caps: &Captures| match caps.get(1) {
                Some(same) => same.as_str().to_string(),
                None => "?".to_string(),
            })
            .to_string();

        for params in rparams.captures_iter(raw_sql) {
            for param in params
                .iter()
                .filter(|p| p.is_some())
                .map(|p| p.unwrap().as_str())
                .filter(|p| p.starts_with(':'))
            {
                params_names.push(param.replace(":", ""));
            }
        }

        Ok(NamedParams { sql, params_names })
    }

    /// Returns the sql as is, disabling named parameter function
    pub fn empty(raw_sql: &str) -> Self {
        Self {
            sql: raw_sql.to_string(),
            params_names: Default::default(),
        }
    }

    /// Re-sort/convert the parameters, applying
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
            ParamsType::Positional(p) => Ok(p),
        }
    }
}
