//! The access plan of a prepared statement, as the server reports it through
//! `isc_dsql_sql_info` / `op_info_sql` (`isc_info_sql_get_plan` and, from
//! Firebird 3.0, `isc_info_sql_explain_plan`). Both backends answer with the
//! same info buffer, so the parsing lives here.

use crate::{ibase, FbError};

/// The plan Firebird chose for a prepared statement, obtained on request
/// through [`crate::FirebirdClientSqlOps::statement_plan`]. Nothing is
/// executed to get it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StmtPlan {
    /// The one-line legacy plan (`PLAN (T NATURAL)`), as `isql` prints it.
    /// `None` for a statement that has none: DDL, `COMMIT`, `SET TRANSACTION`…
    pub plan: Option<String>,
    /// The explained plan: an indented tree, one node per line
    /// (`-> Table "T" Full Scan`). `None` when the statement has no plan, or
    /// the server predates it (Firebird 2.5 does not know the item).
    pub explained: Option<String>,
}

/// The plan items in one info answer, before the text is decoded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanInfo {
    /// `isc_info_sql_get_plan`, when the server wrote it.
    pub plan: Option<Vec<u8>>,
    /// `isc_info_sql_explain_plan`, when the server wrote it.
    pub explained: Option<Vec<u8>>,
    /// The answer ended with `isc_info_truncated`: the buffer was too small
    /// for everything asked. Firebird 3+ still writes as much of the plan as
    /// fits; 2.5 writes nothing for it. Asking one item per call gives each
    /// the whole buffer.
    pub truncated: bool,
}

/// The two items to ask for, in the order the server answers them.
pub const PLAN_INFO_ITEMS: [u8; 2] = [
    ibase::isc_info_sql_get_plan as u8,
    ibase::isc_info_sql_explain_plan as u8,
];

/// Read a `[get_plan, explain_plan]` info answer. Each item is framed
/// `item, u16le length, bytes`; the answer ends with `isc_info_end` or
/// `isc_info_truncated`. An item the server does not know (the explained plan
/// on 2.5) comes back as `isc_info_error` naming it — that plan stays `None`.
pub fn parse_plan_info(buf: &[u8]) -> PlanInfo {
    let mut out = PlanInfo::default();
    let mut i = 0;
    while i < buf.len() {
        let item = buf[i] as u32;
        i += 1;
        match item {
            ibase::isc_info_end => return out,
            ibase::isc_info_truncated => {
                out.truncated = true;
                return out;
            }
            _ => {}
        }
        if i + 2 > buf.len() {
            out.truncated = true;
            return out;
        }
        let len = u16::from_le_bytes([buf[i], buf[i + 1]]) as usize;
        i += 2;
        if i + len > buf.len() {
            out.truncated = true;
            return out;
        }
        let payload = &buf[i..i + len];
        i += len;
        match item {
            ibase::isc_info_sql_get_plan => out.plan = Some(payload.to_vec()),
            ibase::isc_info_sql_explain_plan => out.explained = Some(payload.to_vec()),
            // isc_info_error (unknown item) and anything else: skipped.
            _ => {}
        }
    }
    // Ran off the end without a terminator: the buffer was cut.
    out.truncated = true;
    out
}

impl StmtPlan {
    /// Decode the raw items with the connection charset. The server frames the
    /// legacy plan with a leading newline (and 2.5 a trailing NUL); the
    /// explained one starts with a newline too, so only the line ends are
    /// trimmed — the indentation of its lines is the tree. An empty text is no
    /// plan.
    pub fn from_info(
        info: PlanInfo,
        decode: impl Fn(&[u8]) -> Result<String, FbError>,
    ) -> Result<StmtPlan, FbError> {
        let text = |raw: Option<Vec<u8>>| -> Result<Option<String>, FbError> {
            let Some(raw) = raw else { return Ok(None) };
            let s = decode(&raw)?;
            let s = s
                .trim_start_matches(['\r', '\n'])
                .trim_end_matches(['\0', ' ', '\r', '\n', '\t']);
            Ok(if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            })
        };
        Ok(StmtPlan {
            plan: text(info.plan)?,
            explained: text(info.explained)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![code as u8];
        v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn utf8(b: &[u8]) -> Result<String, FbError> {
        Ok(String::from_utf8_lossy(b).into_owned())
    }

    #[test]
    fn both_items_on_firebird_3() {
        let mut buf = item(ibase::isc_info_sql_get_plan, b"\nPLAN (T NATURAL)");
        buf.extend(item(
            ibase::isc_info_sql_explain_plan,
            b"\nSelect Expression\n    -> Table \"T\" Full Scan",
        ));
        buf.push(ibase::isc_info_end as u8);

        let info = parse_plan_info(&buf);
        assert!(!info.truncated);
        let plan = StmtPlan::from_info(info, utf8).unwrap();
        assert_eq!(plan.plan.as_deref(), Some("PLAN (T NATURAL)"));
        assert_eq!(
            plan.explained.as_deref(),
            Some("Select Expression\n    -> Table \"T\" Full Scan")
        );
    }

    #[test]
    fn firebird_2_5_does_not_know_the_explained_plan() {
        // 2.5 answers an unknown item with isc_info_error naming it, and
        // ends its legacy plan with a NUL.
        let mut buf = item(ibase::isc_info_sql_get_plan, b"\nPLAN (T INDEX (PK_T))\0");
        buf.extend(item(
            ibase::isc_info_error,
            &[ibase::isc_info_sql_explain_plan as u8],
        ));
        buf.push(ibase::isc_info_end as u8);

        let plan = StmtPlan::from_info(parse_plan_info(&buf), utf8).unwrap();
        assert_eq!(plan.plan.as_deref(), Some("PLAN (T INDEX (PK_T))"));
        assert_eq!(plan.explained, None);
    }

    #[test]
    fn a_statement_without_a_plan_writes_nothing() {
        // DDL / COMMIT: the server skips both items.
        let buf = [ibase::isc_info_end as u8];
        let plan = StmtPlan::from_info(parse_plan_info(&buf), utf8).unwrap();
        assert_eq!(plan, StmtPlan::default());

        // Or writes them empty.
        let mut buf = item(ibase::isc_info_sql_get_plan, b"");
        buf.push(ibase::isc_info_end as u8);
        let plan = StmtPlan::from_info(parse_plan_info(&buf), utf8).unwrap();
        assert_eq!(plan, StmtPlan::default());
    }

    #[test]
    fn truncation_keeps_what_came_and_flags_it() {
        let mut buf = item(ibase::isc_info_sql_get_plan, b"\nPLAN (T NATURAL)");
        buf.push(ibase::isc_info_truncated as u8);
        let info = parse_plan_info(&buf);
        assert!(info.truncated);
        assert_eq!(info.plan.as_deref(), Some(&b"\nPLAN (T NATURAL)"[..]));
        assert_eq!(info.explained, None);

        // A payload cut short by the buffer end is truncation too.
        let mut buf = item(ibase::isc_info_sql_explain_plan, b"\nSelect Expression");
        buf.truncate(buf.len() - 4);
        let info = parse_plan_info(&buf);
        assert!(info.truncated);
        assert_eq!(info.explained, None);

        // No terminator at all.
        assert!(parse_plan_info(&[]).truncated);
    }

    #[test]
    fn decode_errors_surface() {
        let mut buf = item(ibase::isc_info_sql_get_plan, b"x");
        buf.push(ibase::isc_info_end as u8);
        let r = StmtPlan::from_info(parse_plan_info(&buf), |_| Err(FbError::from("bad charset")));
        assert!(r.is_err());
    }
}
