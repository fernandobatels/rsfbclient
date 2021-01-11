//! Firebird value representation

use super::backend::Fb;
use diesel::backend::RawValue;
use diesel::row::{Field, PartialRow, Row as DsRow, RowIndex};
use rsfbclient::Column;
use rsfbclient::Row as RsRow;
use std::ops::Range;

pub struct FbValue<'a> {
    pub raw: &'a Column,
}

pub struct FbField<'a> {
    raw: &'a Column,
}

impl<'a> Field<'a, Fb> for FbField<'a> {
    fn field_name(&self) -> Option<&'a str> {
        Some(self.raw.name.as_str())
    }

    fn value(&self) -> Option<RawValue<'a, Fb>> {
        if self.raw.value.is_null() {
            return None;
        }

        Some(FbValue { raw: self.raw })
    }
}

pub struct FbRow<'a> {
    raw: &'a RsRow,
}

impl<'a> FbRow<'a> {
    pub fn new(row: &'a RsRow) -> Self {
        Self { raw: row }
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
            return Some(Self::Field { raw: col });
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
            .find(|(_idx, col)| col.name.to_lowercase() == field_name.to_lowercase())
            .map(|(idx, _col)| idx)
    }
}
