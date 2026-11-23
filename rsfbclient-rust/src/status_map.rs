//! SQLCODE and SQLSTATE fallbacks for servers that omit them on the wire.
use rsfbclient_core::ibase;
use std::sync::OnceLock;

pub(crate) fn lookup(code: u32) -> Option<(i32, &'static str)> {
    static CATALOG: OnceLock<Vec<(u32, i32, &str)>> = OnceLock::new();
    let entries = CATALOG.get_or_init(|| {
        include_str!("firebird_status_map.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                Some((
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                    fields.next()?,
                ))
            })
            .collect()
    });
    let index = entries.binary_search_by_key(&code, |entry| entry.0).ok()?;
    let (_, sql_code, sql_state) = entries[index];
    Some((sql_code, sql_state))
}

pub(crate) fn sql_state(codes: &[u32]) -> &'static str {
    // Match fb_sqlstate's precedence. A specific state beats generic errors,
    // while informational entries and SQLCODE markers carry no error state.
    let mut state = "HY000";
    for &code in codes {
        if matches!(code, ibase::isc_random | ibase::isc_sqlerr) {
            continue;
        }
        if let Some((_, next)) = lookup(code) {
            if next != "00000" {
                state = next;
                if !matches!(state, "22000" | "42000" | "HY000") {
                    break;
                }
            }
        }
    }
    state
}
