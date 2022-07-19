//! Sql column types and traits

use crate::{
    error::{err_column_null, err_type_conv},
    FbError, SqlType,
};

pub use SqlType::*;

/// A database row
pub struct Row {
    pub cols: Vec<Column>,
}

impl Row {
    /// Get the column value by the index
    pub fn get<T>(&self, idx: usize) -> Result<T, FbError>
    where
        Column: ColumnToVal<T>,
    {
        if let Some(col) = self.cols.get(idx) {
            col.clone().to_val()
        } else {
            Err("This index doesn't exists".into())
        }
    }

    /// Get the values for all columns
    pub fn get_all<T>(self) -> Result<T, FbError>
    where
        T: FromRow,
    {
        <T as FromRow>::try_from(self.cols)
    }
}

#[derive(Debug, Clone)]
pub struct Column {
    pub value: SqlType,
    pub name: String,
}

impl Column {
    pub fn new(name: String, value: SqlType) -> Self {
        Column { name, value }
    }
}

#[allow(clippy::wrong_self_convention)]
/// Define the conversion from the buffer to a value
pub trait ColumnToVal<T> {
    fn to_val(self) -> Result<T, FbError>
    where
        Self: std::marker::Sized;
}

impl ColumnToVal<String> for Column {
    fn to_val(self) -> Result<String, FbError> {
        match self.value {
            Text(t) => Ok(t),

            Integer(i) => Ok(i.to_string()),

            Floating(f) => Ok(f.to_string()),

            Timestamp(ts) => Ok(ts.to_string()),

            Binary(_) => Err("This is a binary column. You cannot use string to access".into()),

            Boolean(bo) => Ok(bo.to_string()),

            Null => Err(err_column_null("String")),
        }
    }
}

impl ColumnToVal<i64> for Column {
    fn to_val(self) -> Result<i64, FbError> {
        match self.value {
            Integer(i) => Ok(i),

            Null => Err(err_column_null("i64")),

            col => err_type_conv(col, "i64"),
        }
    }
}

impl ColumnToVal<i32> for Column {
    fn to_val(self) -> Result<i32, FbError> {
        ColumnToVal::<i64>::to_val(self).map(|i| i as i32)
    }
}

impl ColumnToVal<i16> for Column {
    fn to_val(self) -> Result<i16, FbError> {
        ColumnToVal::<i64>::to_val(self).map(|i| i as i16)
    }
}

impl ColumnToVal<f64> for Column {
    fn to_val(self) -> Result<f64, FbError> {
        match self.value {
            Floating(f) => Ok(f),

            Null => Err(err_column_null("f64")),

            col => err_type_conv(col, "f64"),
        }
    }
}

impl ColumnToVal<f32> for Column {
    fn to_val(self) -> Result<f32, FbError> {
        ColumnToVal::<f64>::to_val(self).map(|i| i as f32)
    }
}

impl ColumnToVal<Vec<u8>> for Column {
    fn to_val(self) -> Result<Vec<u8>, FbError> {
        match self.value {
            Binary(b) => Ok(b),

            Null => Err(err_column_null("Vec<u8>")),

            col => err_type_conv(col, "Vec<u8>"),
        }
    }
}

impl ColumnToVal<bool> for Column {
    fn to_val(self) -> Result<bool, FbError> {
        match self.value {
            Boolean(bo) => Ok(bo),

            Null => Err(err_column_null("bool")),

            col => err_type_conv(col, "bool"),
        }
    }
}

/// Implements for all nullable variants
impl<T> ColumnToVal<Option<T>> for Column
where
    Column: ColumnToVal<T>,
{
    fn to_val(self) -> Result<Option<T>, FbError> {
        if self.value.is_null() {
            return Ok(None);
        }

        Ok(Some(self.to_val()?))
    }
}

/// Implemented for types that represents a list of values of columns
pub trait FromRow {
    fn try_from(row: Vec<Column>) -> Result<Self, FbError>
    where
        Self: std::marker::Sized;
}

/// Allow use of a vector instead of tuples, for when the number of columns are unknow at compile time
/// or more columns are needed than what can be used with the tuples
impl FromRow for Row {
    fn try_from(row: Vec<Column>) -> Result<Self, FbError>
    where
        Self: Sized,
    {
        Ok(Row { cols: row })
    }
}

/// For no columns
impl FromRow for () {
    fn try_from(_row: Vec<Column>) -> Result<Self, FbError>
    where
        Self: Sized,
    {
        Ok(())
    }
}

/// Generates FromRow implementations for a tuple
macro_rules! impl_from_row {
    ($($t: ident),+) => {
        impl<'a, $($t),+> FromRow for ($($t,)+)
        where
            $( Column: ColumnToVal<$t>, )+
        {
            fn try_from(row: Vec<Column>) -> Result<Self, FbError> {
                let len = row.len();
                let mut iter = row.into_iter();

                Ok(( $(
                    ColumnToVal::<$t>::to_val(
                        iter
                            .next()
                            .ok_or_else(|| {
                                FbError::Other(
                                    format!("The sql returned less columns than the {} expected", len),
                                )
                            })?
                    )?,
                )+ ))
            }
        }
    };
}

/// Generates FromRow implementations for various tuples
macro_rules! impls_from_row {
    ($t: ident) => {
        impl_from_row!($t);
    };

    ($t: ident, $($ts: ident),+ ) => {
        impls_from_row!($($ts),+);

        impl_from_row!($t, $($ts),+);
    };
}

impls_from_row!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
