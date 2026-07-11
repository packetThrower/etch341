//! HII string-package parser: turns the string blocks that back a
//! Setup form into a `string-id → text` map. Scans a buffer for
//! `EFI_HII_STRING_PACKAGE_HDR` (package type 0x04), so it works on a
//! raw section payload with or without the surrounding package list.
//!
//! Only the UCS-2 string blocks are decoded (that's all Setup uses);
//! font/glyph and other block types are skipped by their length.
//! String IDs are 1-based and assigned in stream order per the spec.

use std::collections::HashMap;

const PKG_TYPE_STRINGS: u8 = 0x04;

// String block types (SIBT_*).
const SIBT_END: u8 = 0x00;
const SIBT_STRING_SCSU: u8 = 0x10;
const SIBT_STRING_SCSU_FONT: u8 = 0x11;
const SIBT_STRINGS_SCSU: u8 = 0x12;
const SIBT_STRINGS_SCSU_FONT: u8 = 0x13;
const SIBT_STRING_UCS2: u8 = 0x14;
const SIBT_STRING_UCS2_FONT: u8 = 0x15;
const SIBT_STRINGS_UCS2: u8 = 0x16;
const SIBT_STRINGS_UCS2_FONT: u8 = 0x17;
const SIBT_DUPLICATE: u8 = 0x20;
const SIBT_SKIP2: u8 = 0x21;
const SIBT_SKIP1: u8 = 0x22;
const SIBT_EXT1: u8 = 0x30;
const SIBT_EXT2: u8 = 0x31;
const SIBT_EXT4: u8 = 0x32;

/// One parsed language's strings.
pub struct StringPackage {
    pub language: String,
    pub strings: HashMap<u16, String>,
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u16le(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// Read a NUL-terminated little-endian UCS-2 string starting at `off`;
/// returns (text, bytes_consumed_including_terminator).
fn ucs2_z(b: &[u8], off: usize) -> (String, usize) {
    let mut units = Vec::new();
    let mut i = off;
    while i + 2 <= b.len() {
        let u = u16::from_le_bytes([b[i], b[i + 1]]);
        i += 2;
        if u == 0 {
            break;
        }
        units.push(u);
    }
    (String::from_utf16_lossy(&units), i - off)
}

/// Find and parse every string package in `buf`. Multiple languages
/// come back as separate entries. HII package lists are commonly
/// embedded inside a PE32 driver, so the byte `0x04` occurs by chance
/// often — a candidate is only accepted if it parses to a real
/// language and at least one string, and only then is its length
/// trusted to skip ahead.
pub fn parse(buf: &[u8]) -> Vec<StringPackage> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 12 <= buf.len() {
        let hdr = u32le(buf, i).unwrap();
        let ptype = (hdr >> 24) as u8;
        let plen = (hdr & 0x00FF_FFFF) as usize;
        if ptype == PKG_TYPE_STRINGS
            && (46..=buf.len() - i).contains(&plen)
            && let Some(pkg) = parse_string_package(&buf[i..i + plen])
            && !pkg.strings.is_empty()
        {
            out.push(pkg);
            i += plen;
            continue;
        }
        i += 1;
    }
    out
}

/// A real HII language tag: `en-US`, `fr-FR`, `x-UEFI-OEM`, … — ASCII
/// alphanumerics and hyphens, starting with a letter. Rejects the
/// garbage that a coincidental `0x04` header decodes to.
fn plausible_language(s: &str) -> bool {
    (2..=40).contains(&s.len())
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

/// Parse one `EFI_HII_STRING_PACKAGE_HDR` payload.
fn parse_string_package(p: &[u8]) -> Option<StringPackage> {
    // Header: [0..4] pkg header, [4..8] HdrSize, [8..12] StringInfoOffset,
    // [12..44] LanguageWindow, [44..46] LanguageName(string-id),
    // [46..] Language (CHAR8, NUL-terminated).
    let string_info_off = u32le(p, 8)? as usize;
    let language = {
        let end = p[46..].iter().position(|&c| c == 0).map(|e| 46 + e)?;
        let s = std::str::from_utf8(p.get(46..end)?).ok()?;
        if !plausible_language(s) {
            return None;
        }
        s.to_string()
    };
    // StringInfoOffset must land past the header and inside the package,
    // on a known block type — else this was a false 0x04 header.
    if !(46..p.len()).contains(&string_info_off) {
        return None;
    }
    if !matches!(
        p[string_info_off],
        SIBT_END
            | SIBT_STRING_UCS2
            | SIBT_STRING_UCS2_FONT
            | SIBT_STRINGS_UCS2
            | SIBT_DUPLICATE
            | SIBT_SKIP1
            | SIBT_SKIP2
            | SIBT_EXT1
            | SIBT_EXT2
            | SIBT_EXT4
    ) {
        return None;
    }

    let mut strings = HashMap::new();
    let mut next_id: u16 = 1;
    let mut off = string_info_off;
    while off < p.len() {
        let block = p[off];
        match block {
            SIBT_END => break,
            SIBT_STRING_UCS2 => {
                let (s, n) = ucs2_z(p, off + 1);
                strings.insert(next_id, s);
                next_id = next_id.wrapping_add(1);
                off += 1 + n;
            }
            SIBT_STRING_UCS2_FONT => {
                // 1 block + 1 font id, then the string.
                let (s, n) = ucs2_z(p, off + 2);
                strings.insert(next_id, s);
                next_id = next_id.wrapping_add(1);
                off += 2 + n;
            }
            SIBT_STRINGS_UCS2 => {
                let count = u16le(p, off + 1)?;
                let mut cur = off + 3;
                for _ in 0..count {
                    let (s, n) = ucs2_z(p, cur);
                    strings.insert(next_id, s);
                    next_id = next_id.wrapping_add(1);
                    cur += n;
                }
                off = cur;
            }
            SIBT_DUPLICATE => {
                // Duplicates an existing string but still consumes an ID.
                next_id = next_id.wrapping_add(1);
                off += 3;
            }
            SIBT_SKIP1 => {
                next_id = next_id.wrapping_add(p.get(off + 1).copied()? as u16);
                off += 2;
            }
            SIBT_SKIP2 => {
                next_id = next_id.wrapping_add(u16le(p, off + 1)?);
                off += 3;
            }
            // Extension blocks: skip by their declared length.
            SIBT_EXT1 => off += p.get(off + 1).copied()? as usize,
            SIBT_EXT2 => off += u16le(p, off + 1)? as usize,
            SIBT_EXT4 => off += u32le(p, off + 1)? as usize,
            // SCSU (compressed) and font blocks: we don't decode these,
            // and they have no simple length — stop rather than desync.
            SIBT_STRING_SCSU
            | SIBT_STRING_SCSU_FONT
            | SIBT_STRINGS_SCSU
            | SIBT_STRINGS_SCSU_FONT
            | SIBT_STRINGS_UCS2_FONT => break,
            _ => break,
        }
    }
    Some(StringPackage { language, strings })
}

/// Pick the English package's string map, falling back to the first
/// package present. Returns an empty map if there are no packages.
pub fn english_strings(packages: &[StringPackage]) -> HashMap<u16, String> {
    packages
        .iter()
        .find(|p| p.language.starts_with("en"))
        .or_else(|| packages.first())
        .map(|p| p.strings.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ucs2(s: &str) -> Vec<u8> {
        let mut v: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        v.extend_from_slice(&[0, 0]);
        v
    }

    /// Build a minimal string package: header + language + string blocks.
    fn string_pkg(lang: &str, blocks: &[u8]) -> Vec<u8> {
        let lang_field = {
            let mut l = lang.as_bytes().to_vec();
            l.push(0);
            l
        };
        let header_fixed = 46; // through LanguageName
        let string_info_off = header_fixed + lang_field.len();
        let mut p = vec![0u8; string_info_off];
        p[4..8].copy_from_slice(&(header_fixed as u32).to_le_bytes()); // HdrSize
        p[8..12].copy_from_slice(&(string_info_off as u32).to_le_bytes());
        p[46..46 + lang_field.len()].copy_from_slice(&lang_field);
        p.extend_from_slice(blocks);
        p.push(SIBT_END);

        let plen = p.len();
        let hdr = (plen as u32) | ((PKG_TYPE_STRINGS as u32) << 24);
        p[0..4].copy_from_slice(&hdr.to_le_bytes());
        p
    }

    #[test]
    fn parses_ucs2_strings_with_ids() {
        let mut blocks = Vec::new();
        blocks.push(SIBT_STRING_UCS2);
        blocks.extend_from_slice(&ucs2("Wake on LAN")); // id 1
        blocks.push(SIBT_STRING_UCS2);
        blocks.extend_from_slice(&ucs2("Disabled")); // id 2
        blocks.push(SIBT_SKIP1);
        blocks.push(2); // skip ids 3,4
        blocks.push(SIBT_STRING_UCS2);
        blocks.extend_from_slice(&ucs2("Enabled")); // id 5
        let pkg = string_pkg("en-US", &blocks);

        let parsed = parse(&pkg);
        assert_eq!(parsed.len(), 1);
        let m = english_strings(&parsed);
        assert_eq!(m[&1], "Wake on LAN");
        assert_eq!(m[&2], "Disabled");
        assert_eq!(m[&5], "Enabled");
        assert!(!m.contains_key(&3));
    }

    #[test]
    fn strings_ucs2_block_assigns_sequential_ids() {
        let mut blocks = vec![SIBT_STRINGS_UCS2];
        blocks.extend_from_slice(&2u16.to_le_bytes());
        blocks.extend_from_slice(&ucs2("First"));
        blocks.extend_from_slice(&ucs2("Second"));
        let pkg = string_pkg("en-US", &blocks);
        let m = english_strings(&parse(&pkg));
        assert_eq!(m[&1], "First");
        assert_eq!(m[&2], "Second");
    }

    #[test]
    fn prefers_english_over_other_languages() {
        let mut fr = vec![SIBT_STRING_UCS2];
        fr.extend_from_slice(&ucs2("Désactivé"));
        let mut en = vec![SIBT_STRING_UCS2];
        en.extend_from_slice(&ucs2("Disabled"));
        let mut buf = string_pkg("fr-FR", &fr);
        buf.extend_from_slice(&string_pkg("en-US", &en));
        let m = english_strings(&parse(&buf));
        assert_eq!(m[&1], "Disabled");
    }
}
