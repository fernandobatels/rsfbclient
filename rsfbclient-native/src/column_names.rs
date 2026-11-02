//! SQL-info aliases are length-delimited; unlike legacy XSQLVAR names, they
//! retain Firebird 5's complete UTF-8 identifiers.
use rsfbclient_core::{ibase, Charset, FbError};

pub(crate) fn parse_aliases_with_charset(
    data: &[u8],
    names: &mut [Option<String>],
    charset: &Charset,
) -> Result<bool, FbError> {
    let mut offset = 0;
    let mut column = None;
    while let Some(&item) = data.get(offset) {
        offset += 1;
        match item as u32 {
            ibase::isc_info_end => return Ok(false),
            ibase::isc_info_truncated => return Ok(true),
            ibase::isc_info_sql_select | ibase::isc_info_sql_describe_end => continue,
            _ => {}
        }
        let length = data
            .get(offset..offset + 2)
            .ok_or("Incomplete native column-name metadata length")?;
        offset += 2;
        let length = u16::from_le_bytes([length[0], length[1]]) as usize;
        let value = data
            .get(offset..offset + length)
            .ok_or("Incomplete native column-name metadata value")?;
        offset += length;
        match item as u32 {
            ibase::isc_info_sql_describe_vars | ibase::isc_info_sql_sqlda_seq => {
                if value.is_empty() || value.len() > 4 {
                    return Err("Invalid native column-name metadata number".into());
                }
                let number = value
                    .iter()
                    .enumerate()
                    .fold(0u32, |n, (i, b)| n | ((*b as u32) << (i * 8)))
                    as usize;
                if item as u32 == ibase::isc_info_sql_describe_vars {
                    if number != names.len() {
                        return Err("Native column-name metadata count changed".into());
                    }
                } else {
                    if number == 0 || number > names.len() {
                        return Err("Invalid native column-name metadata index".into());
                    }
                    column = Some(number - 1);
                }
            }
            ibase::isc_info_sql_alias => {
                let column = column.ok_or("Native column alias has no column index")?;
                names[column] = Some(charset.decode(value)?);
            }
            _ => return Err(format!("Unexpected native column-name metadata item {item}").into()),
        }
    }
    Err("Unterminated native column-name metadata".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_aliases(data: &[u8], names: &mut [Option<String>]) -> Result<bool, FbError> {
        parse_aliases_with_charset(data, names, &rsfbclient_core::charset::UTF_8)
    }
    #[test]
    fn attachment_character_set_decodes_non_utf8_aliases_without_replacement() {
        let mut bytes = header(1);
        value(&mut bytes, 9, &1u32.to_le_bytes());
        value(&mut bytes, 19, &[0xc6, 0xc6]);
        bytes.push(1);
        let mut names = vec![None];
        assert!(!parse_aliases_with_charset(
            &bytes,
            &mut names,
            &rsfbclient_core::charset::WIN_1251
        )
        .unwrap());
        assert_eq!(names, vec![Some("ЖЖ".into())]);
        assert!(parse_aliases(&bytes, &mut [None]).is_err());
    }
    fn value(output: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
        output.push(tag);
        output.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        output.extend_from_slice(bytes);
    }
    fn header(count: u32) -> Vec<u8> {
        let mut result = vec![4];
        value(&mut result, 7, &count.to_le_bytes());
        result
    }
    #[test]
    fn full_utf8_aliases_and_duplicate_aliases_keep_their_column_positions() {
        let name = "Ж".repeat(63);
        let mut bytes = header(2);
        for column in 1..=2u32 {
            value(&mut bytes, 9, &column.to_le_bytes());
            value(&mut bytes, 19, name.as_bytes());
            bytes.push(8);
        }
        bytes.push(1);
        let mut names = vec![None, None];
        assert!(!parse_aliases(&bytes, &mut names).unwrap());
        assert_eq!(names, vec![Some(name.clone()), Some(name)]);
    }
    #[test]
    fn a_truncated_page_can_be_completed_from_the_next_column() {
        let mut first = header(2);
        value(&mut first, 9, &1u32.to_le_bytes());
        value(&mut first, 19, b"first");
        first.extend([8, 2]);
        let mut names = vec![None, None];
        assert!(parse_aliases(&first, &mut names).unwrap());
        assert!(names[1].is_none());
        let mut next = header(2);
        value(&mut next, 9, &2u32.to_le_bytes());
        value(&mut next, 19, b"second");
        next.extend([8, 1]);
        assert!(!parse_aliases(&next, &mut names).unwrap());
        assert_eq!(names, vec![Some("first".into()), Some("second".into())]);
    }
    #[test]
    fn malformed_metadata_never_becomes_a_truncated_or_lossy_identifier() {
        for bytes in [
            vec![4, 7, 4],
            vec![4, 7, 4, 0, 2, 0],
            vec![4, 19, 1, 0, b'x'],
            vec![4, 9, 4, 0, 0, 0, 0, 0],
            vec![4, 9, 4, 0, 3, 0, 0, 0],
        ] {
            assert!(parse_aliases(&bytes, &mut [None, None]).is_err());
        }
        let mut bytes = header(1);
        value(&mut bytes, 9, &1u32.to_le_bytes());
        value(&mut bytes, 19, &[0xd0]);
        bytes.push(1);
        assert!(parse_aliases(&bytes, &mut [None]).is_err());
    }
}
