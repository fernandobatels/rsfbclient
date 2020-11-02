//! Firebird value representation

use super::backend::Fb;
use diesel::deserialize::{FromSqlRow, Queryable};
use diesel::result::Error::DeserializationError;
use diesel::row::Row as DsRow;
use diesel::sql_types::HasSqlType;
use diesel::QueryResult;
use rsfbclient::Row as RsRow;
use rsfbclient::{Column, FbError, FromRow};
use std::boxed::Box;
use std::marker::PhantomData;

pub struct FbValue(pub Column);

pub struct FbRow {
    raw: RsRow,
    col_index: usize,
}

impl FromRow for FbRow {
    fn try_from(row: Vec<Column>) -> Result<Self, FbError>
    where
        Self: Sized,
    {
        Ok(Self {
            raw: RsRow::try_from(row)?,
            col_index: 0,
        })
    }
}

impl DsRow<Fb> for FbRow {
    fn take(&mut self) -> Option<&FbValue> {
        if let Some(col) = self.raw.cols.get(self.col_index) {
            self.col_index = self.col_index + 1;

            unsafe {
                return (col as *const _ as *const FbValue)
                    .as_ref()
                    .and_then(|v| Some(v));
            }
        }

        None
    }

    fn next_is_null(&self, count: usize) -> bool {
        self.raw.cols.len() > count
    }
}

pub struct Cursor<'a, ST, T> {
    raw: Box<dyn Iterator<Item = Result<FbRow, FbError>> + 'a>,
    _marker: PhantomData<(ST, T)>,
}

impl<'a, ST, T> From<Box<dyn Iterator<Item = Result<FbRow, FbError>> + 'a>> for Cursor<'a, ST, T> {
    fn from(raw: Box<dyn Iterator<Item = Result<FbRow, FbError>> + 'a>) -> Self {
        Self {
            raw: raw,
            _marker: PhantomData,
        }
    }
}

impl<'a, ST, T> Iterator for Cursor<'a, ST, T>
where
    Fb: HasSqlType<ST>,
    T: Queryable<ST, Fb>,
{
    type Item = QueryResult<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(rrow) = self.raw.next() {
            if let Ok(mut row) = rrow {
                let rs = T::Row::build_from_row(&mut row)
                    .map(T::build)
                    .map_err(DeserializationError);

                return Some(rs);
            }
        }

        None
    }
}
