//! Go string semantics the ported code compares and prints with: `strings.ToLower`,
//! `strings.EqualFold` and `strconv.IsPrint` (re-exported from `cpa_core::gostr`, which
//! owns Go's Unicode tables so the config and credential loaders share them),
//! `strings.TrimSpace` and `strconv.Quote` (`%q`).

use crate::json::decode_rune;
pub use cpa_core::gostr::{GoStr, is_print, to_lower_rune, to_upper_rune};

/// `strings.TrimSpace` on a Go byte string: Unicode white space at both ends; invalid
/// UTF-8 is never space.
pub fn trim_space(b: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < b.len() {
        match decode_rune(&b[start..]) {
            (Some(c), n) if c.is_whitespace() => start += n,
            _ => break,
        }
    }
    let mut end = b.len();
    while end > start {
        let mut i = end - 1;
        while i > start && end - i < 4 && b[i] & 0xC0 == 0x80 {
            i -= 1;
        }
        match decode_rune(&b[i..end]) {
            (Some(c), n) if i + n == end && c.is_whitespace() => end = i,
            _ => break,
        }
    }
    &b[start..end]
}

/// `strings.ToLower` on a Go byte string. Invalid bytes become U+FFFD as in Go.
pub fn lower_bytes(b: &[u8]) -> String {
    String::from_utf8_lossy(b).go_lower()
}

/// `strconv.Quote`, which is what `%q` prints for a Go string (bytes; invalid UTF-8
/// prints as `\xNN`).
pub fn quote(s: impl AsRef<[u8]>) -> String {
    let mut s = s.as_ref();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    while !s.is_empty() {
        let (c, width) = decode_rune(s);
        let Some(c) = c else {
            out.push_str(&format!("\\x{:02x}", s[0]));
            s = &s[1..];
            continue;
        };
        s = &s[width..];
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if is_print(c) => out.push(c),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            c if c < ' ' || c == '\u{7f}' => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push('"');
    out
}

/// Go `url.QueryEscape`: unreserved bytes kept, space as `+`, everything else `%XX`.
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Go `url.Values.Encode` for single-valued pairs: sorted by key, `key=value` joined with
/// `&`, both query-escaped.
pub fn values_encode(pairs: &[(&str, &str)]) -> String {
    let mut sorted: Vec<&(&str, &str)> = pairs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    sorted
        .iter()
        .map(|(k, v)| format!("{}={}", query_escape(k), query_escape(v)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_escape_and_values_encode_follow_go() {
        // url.QueryEscape("a b/c?d=é~") and url.Values{"z": {"1"}, "a b": {"x&y"}}.Encode().
        assert_eq!(super::query_escape("a b/c?d=\u{e9}~"), "a+b%2Fc%3Fd%3D%C3%A9~");
        assert_eq!(super::values_encode(&[("z", "1"), ("a b", "x&y")]), "a+b=x%26y&z=1");
    }

    use super::*;

    // Expected values from Go 1.26 (strconv.Quote, strings.TrimSpace).
    #[test]
    fn matches_go_quote_rules() {
        assert_eq!(quote("a\nb"), r#""a\nb""#);
        assert_eq!(quote("\u{a0}"), r#""\u00a0""#);
        assert_eq!(quote("é\u{7f}\u{1}\""), r#""é\x7f\x01\"""#);
        assert_eq!(quote("\u{e0000}"), r#""\U000e0000""#);
        assert_eq!(quote("\u{1f600}"), "\"\u{1f600}\"");
        assert_eq!(quote(b"a\xffb\xef\xbf\xbd"), "\"a\\xffb\u{fffd}\"");
        assert_eq!(trim_space(b"\xc2\xa0 a b\t\xe3\x80\x80"), b"a b");
        assert_eq!(trim_space(b" \xff "), b"\xff");
        assert_eq!(trim_space(b"\xe3\x80"), b"\xe3\x80", "a truncated rune is not space");
    }
}
