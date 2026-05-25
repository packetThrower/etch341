//! Inspect / search primitives shared by the CLI and the GUI:
//! string extraction, pattern parsing, and byte-level search. All
//! functions are pure (no I/O, no hardware) so they're cheap to
//! test and call from anywhere.

/// Interpret a search needle as either hex bytes (when condensed
/// form is all-hex-digits + even length) or ASCII (otherwise).
/// Mirrors the heuristic of an in-place hex editor's find-bar:
///   `55 AA`         → `[0x55, 0xAA]`
///   `NVIDIA`        → ASCII bytes for "NVIDIA"
///   `ABCD`          → `[0xAB, 0xCD]` (even-length, all hex wins)
///   `ABC`           → ASCII bytes for "ABC" (odd-length defaults to ASCII)
pub fn parse_hex_needle(s: &str) -> Vec<u8> {
    let condensed: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !condensed.is_empty()
        && condensed.len().is_multiple_of(2)
        && condensed.chars().all(|c| c.is_ascii_hexdigit())
    {
        (0..condensed.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&condensed[i..i + 2], 16).ok())
            .collect()
    } else {
        s.as_bytes().to_vec()
    }
}

/// Case-insensitive byte match for ASCII letters; exact for everything
/// else. Used so a search for `power` highlights `Power` runs without
/// silently doing the wrong thing for binary patterns like `55 AA`
/// where bytes aren't letters at all.
pub fn byte_match_ci(a: u8, b: u8) -> bool {
    if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// Walk the byte slice and emit runs of printable ASCII (0x20..=0x7E)
/// at least `min_len` characters long. Each entry is `(offset, run)`.
pub fn extract_strings(bytes: &[u8], min_len: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut buf = String::new();
    for (i, &b) in bytes.iter().enumerate() {
        if (0x20..=0x7E).contains(&b) {
            if start.is_none() {
                start = Some(i);
            }
            buf.push(b as char);
        } else if !buf.is_empty() {
            if buf.len() >= min_len {
                out.push((start.unwrap(), std::mem::take(&mut buf)));
            } else {
                buf.clear();
            }
            start = None;
        }
    }
    if buf.len() >= min_len {
        out.push((start.unwrap(), buf));
    }
    out
}

/// Find every offset in `haystack` where `needle` occurs (overlapping
/// matches included). Uses [`byte_match_ci`] so ASCII patterns are
/// case-folded but binary patterns stay exact. Empty needle returns
/// an empty result.
pub fn find_pattern(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    'outer: for start in 0..=(haystack.len() - needle.len()) {
        for (i, &n) in needle.iter().enumerate() {
            if !byte_match_ci(haystack[start + i], n) {
                continue 'outer;
            }
        }
        out.push(start);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_needle_picks_hex_for_even_hexdigit_input() {
        assert_eq!(parse_hex_needle("55AA"), vec![0x55, 0xAA]);
        assert_eq!(parse_hex_needle("55 AA"), vec![0x55, 0xAA]);
        assert_eq!(parse_hex_needle("DEADBEEF"), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn parse_hex_needle_falls_back_to_ascii_for_odd_or_non_hex() {
        assert_eq!(parse_hex_needle("ABC"), b"ABC".to_vec()); // odd-length
        assert_eq!(parse_hex_needle("NVIDIA"), b"NVIDIA".to_vec()); // not hex
        assert_eq!(parse_hex_needle(""), Vec::<u8>::new());
    }

    #[test]
    fn extract_strings_finds_runs() {
        let bytes = b"\x00\x00Hello\x00World!\x00\x00";
        let r = extract_strings(bytes, 4);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], (2, "Hello".to_string()));
        assert_eq!(r[1], (8, "World!".to_string()));
    }

    #[test]
    fn extract_strings_respects_min_len() {
        let bytes = b"abc\x00longerstring\x00xy";
        let r = extract_strings(bytes, 4);
        assert_eq!(r, vec![(4, "longerstring".to_string())]);
    }

    #[test]
    fn extract_strings_keeps_trailing_run_without_terminator() {
        let bytes = b"trailing";
        let r = extract_strings(bytes, 4);
        assert_eq!(r, vec![(0, "trailing".to_string())]);
    }

    #[test]
    fn find_pattern_ascii_is_case_insensitive() {
        let h = b"the quick Brown fox";
        assert_eq!(find_pattern(h, b"BROWN"), vec![10]);
        assert_eq!(find_pattern(h, b"brown"), vec![10]);
    }

    #[test]
    fn find_pattern_binary_is_case_sensitive() {
        let h = &[0x00, 0x55, 0xAA, 0xFF, 0x55, 0xAA, 0x55, 0xAA];
        assert_eq!(find_pattern(h, &[0x55, 0xAA]), vec![1, 4, 6]);
    }

    #[test]
    fn find_pattern_empty_needle_returns_empty() {
        assert!(find_pattern(b"abc", b"").is_empty());
    }

    #[test]
    fn find_pattern_needle_larger_than_haystack_is_empty() {
        assert!(find_pattern(b"ab", b"abc").is_empty());
    }

    #[test]
    fn find_pattern_overlapping_matches() {
        let h = b"aaaa";
        assert_eq!(find_pattern(h, b"aa"), vec![0, 1, 2]);
    }
}
