use crate::{ibase, Charset, FbError, StmtType};

/// An input parameter reported before client-side type conversion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParameterDesc {
    /// SQL type code without the nullable bit.
    pub sql_type: u32,
    /// Character set for character fields or subtype for BLOBs.
    pub sub_type: i16,
    /// Scale of a fixed-point number.
    pub scale: i16,
    /// Length of the value in bytes.
    pub length: i16,
    /// Whether the value can be NULL.
    pub nullable: bool,
}

/// Description of a prepared output column, including its origin when known.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnDesc {
    /// Result-set name or alias.
    pub name: String,
    /// Underlying field, when this column refers to a field.
    pub field: Option<String>,
    /// Relation, view or procedure which supplies the field.
    pub relation: Option<String>,
    /// Context alias of the relation, when available.
    pub relation_alias: Option<String>,
    /// Owner of the relation, when available.
    pub owner: Option<String>,
    /// SQL type code without the nullable bit.
    pub sql_type: u32,
    /// Character set for character fields or subtype for BLOBs.
    pub sub_type: i16,
    /// Scale of a fixed-point number.
    pub scale: i16,
    /// Length of the value in bytes.
    pub length: i16,
    /// Whether the value can be NULL.
    pub nullable: bool,
}

/// Metadata for a prepared statement. No cursor needs to be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementDescription {
    pub statement_type: StmtType,
    /// Input parameters in binding order, including repeated named markers.
    pub parameters: Vec<ParameterDesc>,
    /// Result columns in projection order.
    pub columns: Vec<ColumnDesc>,
}

impl From<ColumnDesc> for ParameterDesc {
    fn from(value: ColumnDesc) -> Self {
        Self {
            sql_type: value.sql_type,
            sub_type: value.sub_type,
            scale: value.scale,
            length: value.length,
            nullable: value.nullable,
        }
    }
}

/// Read the statement's input and output descriptors using SQL-info.
///
/// Kept here so the native and wire adapters share the same paging and decoder.
#[doc(hidden)]
pub fn read_statement_descriptors(
    charset: &Charset,
    mut info: impl FnMut(&[u8]) -> Result<Vec<u8>, FbError>,
) -> Result<(Vec<ParameterDesc>, Vec<ColumnDesc>), FbError> {
    let inputs = read_descriptors(ibase::isc_info_sql_bind, charset, &mut info)?;
    let outputs = read_descriptors(ibase::isc_info_sql_select, charset, &mut info)?;
    Ok((
        inputs.into_iter().map(ParameterDesc::from).collect(),
        outputs,
    ))
}

fn read_descriptors(
    kind: u32,
    charset: &Charset,
    info: &mut impl FnMut(&[u8]) -> Result<Vec<u8>, FbError>,
) -> Result<Vec<ColumnDesc>, FbError> {
    let type_items = [
        ibase::isc_info_sql_sqlda_seq as u8,
        ibase::isc_info_sql_type as u8,
        ibase::isc_info_sql_sub_type as u8,
        ibase::isc_info_sql_scale as u8,
        ibase::isc_info_sql_length as u8,
        ibase::isc_info_sql_null_ind as u8,
    ];
    let column_items = [
        ibase::isc_info_sql_field as u8,
        ibase::isc_info_sql_relation as u8,
        ibase::isc_info_sql_owner as u8,
        ibase::isc_info_sql_alias as u8,
    ];
    let mut descriptors = Vec::new();
    let mut expected_count = None;

    loop {
        let before = descriptors.len();
        let start = u16::try_from(before + 1).map_err(|_| invalid_info())?;
        let [low, high] = start.to_le_bytes();
        let mut request = vec![
            ibase::isc_info_sql_sqlda_start as u8,
            2,
            low,
            high,
            kind as u8,
            ibase::isc_info_sql_describe_vars as u8,
        ];
        request.extend_from_slice(&type_items);
        if kind == ibase::isc_info_sql_select {
            request.extend_from_slice(&column_items);
        }
        request.push(ibase::isc_info_sql_describe_end as u8);

        let buffer = info(&request)?;
        let data = buffer
            .strip_prefix(&[kind as u8, ibase::isc_info_sql_describe_vars as u8])
            .ok_or_else(invalid_info)?;
        let (count_bytes, data) = take_item(data)?;
        let count = usize::try_from(integer(count_bytes)?).map_err(|_| invalid_info())?;
        if *expected_count.get_or_insert(count) != count || count > u16::MAX as usize {
            return Err(invalid_info());
        }
        let truncated = parse_page(data, charset, &mut descriptors)?;
        if descriptors.len() == count {
            return Ok(descriptors);
        }
        if !truncated || descriptors.len() <= before || descriptors.len() > count {
            return Err(invalid_info());
        }
    }
}

fn parse_page(
    mut data: &[u8],
    charset: &Charset,
    out: &mut Vec<ColumnDesc>,
) -> Result<bool, FbError> {
    let mut current: Option<ColumnDesc> = None;
    while let Some((&kind, rest)) = data.split_first() {
        data = rest;
        match u32::from(kind) {
            ibase::isc_info_end if current.is_none() => return Ok(false),
            ibase::isc_info_truncated => return Ok(true),
            ibase::isc_info_sql_describe_end => {
                let mut column = current.take().ok_or_else(invalid_info)?;
                if column.sql_type == 0 {
                    return Err(invalid_info());
                }
                if column.relation.is_none() {
                    column.field = None;
                }
                out.push(column);
            }
            ibase::isc_info_sql_sqlda_seq => {
                let (value, rest) = take_item(data)?;
                data = rest;
                if current.is_some() || integer(value)? as usize != out.len() + 1 {
                    return Err(invalid_info());
                }
                current = Some(ColumnDesc::default());
            }
            kind => {
                let (value, rest) = take_item(data)?;
                data = rest;
                let desc = current.as_mut().ok_or_else(invalid_info)?;
                match kind {
                    ibase::isc_info_sql_type => {
                        let sql_type = integer(value)? as u32;
                        desc.sql_type = sql_type & !1;
                        desc.nullable |= sql_type & 1 != 0;
                    }
                    ibase::isc_info_sql_sub_type => desc.sub_type = integer(value)? as i16,
                    ibase::isc_info_sql_scale => desc.scale = integer(value)? as i16,
                    ibase::isc_info_sql_length => desc.length = integer(value)? as i16,
                    ibase::isc_info_sql_null_ind => desc.nullable |= integer(value)? != 0,
                    ibase::isc_info_sql_alias => desc.name = charset.decode(value)?,
                    ibase::isc_info_sql_field if !value.is_empty() => {
                        desc.field = Some(charset.decode(value)?);
                    }
                    ibase::isc_info_sql_relation if !value.is_empty() => {
                        desc.relation = Some(charset.decode(value)?);
                    }
                    ibase::isc_info_sql_owner if !value.is_empty() => {
                        desc.owner = Some(charset.decode(value)?);
                    }
                    _ => {}
                }
            }
        }
    }
    Err(invalid_info())
}

fn take_item(data: &[u8]) -> Result<(&[u8], &[u8]), FbError> {
    if let [low, high, payload @ ..] = data {
        let len = u16::from_le_bytes([*low, *high]) as usize;
        return payload.split_at_checked(len).ok_or_else(invalid_info);
    }
    Err(invalid_info())
}

fn integer(value: &[u8]) -> Result<i32, FbError> {
    if value.is_empty() || value.len() > 4 {
        return Err(invalid_info());
    }
    let fill = if value[value.len() - 1] & 0x80 != 0 {
        0xff
    } else {
        0
    };
    let mut bytes = [fill; 4];
    bytes[..value.len()].copy_from_slice(value);
    Ok(i32::from_le_bytes(bytes))
}

fn invalid_info() -> FbError {
    "Invalid prepared statement metadata".into()
}
