//! Firebird value representation

use super::backend::Fb;
use diesel::backend::RawValue;
use diesel::row::{Field, PartialRow, Row as DsRow, RowIndex};
use rsfbclient::Row as RsRow;
use rsfbclient::{Column, FbError, FromRow};
use std::ffi::CStr;
use std::marker::PhantomData;
use std::ops::Range;

pub struct FbValue<'a> {
    pub raw: Column,
    _marker: PhantomData<&'a ()>,
}

pub struct FbField<'a> {
    raw: Column,
    _marker: PhantomData<&'a ()>,
}

impl<'a> Field<'a, Fb> for FbField<'a> {
    fn field_name(&self) -> Option<&'a str> {
        let name_ptr = self.raw.name.as_ptr();

        unsafe {
            Some(
                CStr::from_ptr(name_ptr as *const _)
                    .to_str()
                    .expect("Error on get the field name"),
            )
        }
    }

    fn value(&self) -> Option<RawValue<'a, Fb>> {
        if self.raw.value.is_null() {
            return None;
        }

        Some(FbValue {
            raw: self.raw.clone(),
            _marker: PhantomData,
        })
    }
}

pub struct FbRow<'a> {
    raw: RsRow,
    _marker: PhantomData<&'a ()>,
}

impl<'a> FromRow for FbRow<'a> {
    fn try_from(row: Vec<Column>) -> Result<Self, FbError>
    where
        Self: Sized,
    {
        Ok(Self {
            raw: RsRow::try_from(row)?,
            _marker: PhantomData,
        })
    }
}

impl<'a> DsRow<'a, Fb> for FbRow<'a> {
    type Field = FbField<'a>;
    type InnerPartialRow = Self;

    fn get<I>(&self, idx: I) -> Option<Self::Field>
    where
        Self: RowIndex<I>,
    {
        let idx = self.idx(idx)?;
        if let Some(col) = self.raw.cols.get(idx) {
            return Some(Self::Field {
                raw: col.clone(),
                _marker: PhantomData,
            });
        }

        None
    }

    fn field_count(&self) -> usize {
        self.raw.cols.len()
    }

    fn partial_row(&self, range: Range<usize>) -> PartialRow<Self::InnerPartialRow> {
        PartialRow::new(self, range)
    }
}

impl<'a> RowIndex<usize> for FbRow<'a> {
    fn idx(&self, idx: usize) -> Option<usize> {
        if idx < self.raw.cols.len() {
            Some(idx)
        } else {
            None
        }
    }
}

impl<'a, 'b> RowIndex<&'a str> for FbRow<'b> {
    fn idx(&self, field_name: &'a str) -> Option<usize> {
        self.raw
            .cols
            .iter()
            .enumerate()
            .find(|(_idx, col)| col.name == field_name)
            .map(|(idx, _col)| idx)
    }
}
