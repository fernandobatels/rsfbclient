//! Firebird value representation

use super::backend::Fb;
use diesel::backend::RawValue;
use diesel::row::{Field, FieldRet, PartialRow, Row as DsRow, RowGatWorkaround, RowIndex};
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

pub struct FbRow {
    raw: RsRow,
}

impl FbRow {
    pub fn new(row: RsRow) -> Self {
        Self { raw: row }
    }
}

impl<'b> RowGatWorkaround<'b, Fb> for FbRow {
    type Field = FbField<'b>;
}

impl<'a> DsRow<'a, Fb> for FbRow {
    type InnerPartialRow = Self;

    fn get<'b, I>(&'b self, idx: I) -> Option<FieldRet<'b, Self, Fb>>
    where
        'a: 'b,
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

impl RowIndex<usize> for FbRow {
    fn idx(&self, idx: usize) -> Option<usize> {
        if idx < self.raw.cols.len() {
            Some(idx)
        } else {
            None
        }
    }
}

impl<'a> RowIndex<&'a str> for FbRow {
    fn idx(&self, field_name: &'a str) -> Option<usize> {
        self.raw
            .cols
            .iter()
            .position(|col| col.name.to_lowercase() == field_name.to_lowercase())
    }
}
