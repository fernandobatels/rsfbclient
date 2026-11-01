//! Parameter locations in executable SQL, excluding literals and comments.
use crate::FbError;
use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SqlParameter {
    pub range: Range<usize>,
    pub name: Option<String>,
}

pub(crate) fn sql_parameters(sql: &str) -> Result<Vec<SqlParameter>, FbError> {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut result = Vec::new();
    while i < bytes.len() {
        if bytes[i..].starts_with(b"--") {
            i += 2;
            while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            // Firebird 5.0.4 ends a comment at the first */; it does not nest.
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            if i == bytes.len() {
                return Err("Unterminated SQL block comment".into());
            }
            i += 2;
        } else if matches!(bytes[i], b'q' | b'Q')
            && bytes.get(i + 1) == Some(&b'\'')
            && (i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric() || matches!(bytes[i - 1], b'_' | b'$')))
        {
            let open = sql[i + 2..]
                .chars()
                .next()
                .ok_or("Unterminated SQL alternative literal")?;
            let close = match open {
                '[' => ']',
                '{' => '}',
                '(' => ')',
                '<' => '>',
                other => other,
            };
            i += 2 + open.len_utf8();
            let mut ended = false;
            while i < bytes.len() {
                let c = sql[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == close && bytes.get(i) == Some(&b'\'') {
                    i += 1;
                    ended = true;
                    break;
                }
            }
            if !ended {
                return Err("Unterminated SQL alternative literal".into());
            }
        } else if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            i += 1;
            let mut ended = false;
            while i < bytes.len() {
                if bytes[i] == quote {
                    i += 1;
                    if bytes.get(i) == Some(&quote) {
                        i += 1;
                    } else {
                        ended = true;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if !ended {
                return Err("Unterminated SQL quoted value".into());
            }
        } else if bytes[i] == b'?' {
            result.push(SqlParameter {
                range: i..i + 1,
                name: None,
            });
            i += 1;
        } else if bytes[i..].starts_with(b"::") {
            i += 2;
        } else if bytes[i] == b':' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let c = sql[i..].chars().next().unwrap();
                if c.is_alphanumeric() || matches!(c, '_' | '$') {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
            if i > start + 1 {
                result.push(SqlParameter {
                    range: start..i,
                    name: Some(sql[start + 1..i].into()),
                });
            }
        } else {
            i += sql[i..].chars().next().unwrap().len_utf8();
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markers_exclude_all_firebird_quoted_and_comment_forms() {
        let sql="select ?, ':hidden?', \"a:fake?\", q'{? '' :no}', Q'!? :no!', :real, :real, :Ж from t /* ? /* :no */ -- :no ?\r where x=:last";
        let markers = sql_parameters(sql).unwrap();
        assert_eq!(
            markers
                .iter()
                .map(|m| &sql[m.range.clone()])
                .collect::<Vec<_>>(),
            vec!["?", ":real", ":real", ":Ж", ":last"]
        );
    }
    #[test]
    fn doubled_quotes_and_unicode_delimiters_keep_boundaries() {
        let sql = "select 'it''s :no?', \"a\"\":no?\", q'Ж? :noЖ', :yes from t";
        let markers = sql_parameters(sql).unwrap();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].name.as_deref(), Some("yes"));
    }
    #[test]
    fn malformed_source_is_not_partially_rewritten() {
        for sql in ["? /*", "? 'text", "? q'[text", "? \"column"] {
            assert!(sql_parameters(sql).is_err(), "{sql}");
        }
    }
    #[test]
    fn named_conversion_preserves_data_and_repeated_values() {
        let parsed = crate::NamedParams::parse(
            "select :p,:p,':data',q'{:data}',\"a:data\" from t -- :ignored ' ?",
        )
        .unwrap();
        assert_eq!(
            parsed.sql,
            "select ?,?,':data',q'{:data}',\"a:data\" from t -- :ignored ' ?"
        );
        let p = crate::ParamsType::Named(std::collections::HashMap::from([(
            "p".into(),
            crate::SqlType::Text("payload".into()),
        )]));
        let converted = parsed.convert(p).unwrap();
        assert_eq!(converted.len(), 2);
        assert!(converted
            .iter()
            .all(|v| matches!(v,crate::SqlType::Text(s) if s=="payload")));
    }
}
