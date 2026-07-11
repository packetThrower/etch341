//! AMI NVAR variable-store parser: finds `NVAR` entry runs anywhere in
//! a flash image and returns `variable-name → current data`, which is
//! where Setup reads its live values from. Reads the whole raw image
//! (the store often sits outside the FV structure the FFS walker
//! covers), so it operates independently of `fv.rs`.
//!
//! Format reference: LongSoft/UEFITool (nvram.cpp). Only what the
//! Setup explorer needs — named, non-data-only entries — is decoded;
//! the GUID store and extended headers are skipped.

use std::collections::HashMap;

const SIG: &[u8; 4] = b"NVAR";

/// NVAR_ENTRY_HEADER: Signature(4) Size(2) Next(3) Attributes(1).
const HDR: usize = 10;

// Entry attribute bits.
const ATTR_RUNTIME: u8 = 0x01;
const ATTR_ASCII_NAME: u8 = 0x02;
const ATTR_GUID: u8 = 0x04;
const ATTR_DATA_ONLY: u8 = 0x08;
const ATTR_EXT_HEADER: u8 = 0x10;
const ATTR_HW_ERROR: u8 = 0x20;
const ATTR_AUTH_WRITE: u8 = 0x40;
const ATTR_VALID: u8 = 0x80;

/// Parse all NVAR stores in `image`; later valid entries win, matching
/// the update-in-place chain semantics closely enough for reads.
pub fn parse(image: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut vars = HashMap::new();
    for start in find_stores(image) {
        parse_store(image, start, &mut vars);
    }
    vars
}

/// The signature `NVAR` also appears by chance inside strings and
/// compressed data. A real store is a long contiguous run of entries;
/// coincidental hits form runs of one or two. Require a minimum run
/// length to tell them apart.
const MIN_RUN: usize = 8;

/// Find store starts: offsets that begin a contiguous run of at least
/// `MIN_RUN` NVAR entries. Skips past each accepted run so a stray
/// `NVAR` byte *inside* a real store can't spawn a misaligned second
/// store.
fn find_stores(image: &[u8]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + HDR <= image.len() {
        if &image[i..i + 4] == SIG {
            let (count, end) = walk_extent(image, i);
            if count >= MIN_RUN {
                starts.push(i);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    starts
}

/// Follow contiguous NVAR entries from `start`; return (entry count,
/// offset just past the last entry).
fn walk_extent(image: &[u8], start: usize) -> (usize, usize) {
    let mut off = start;
    let mut count = 0;
    while off + HDR <= image.len() && &image[off..off + 4] == SIG {
        let size = u16::from_le_bytes([image[off + 4], image[off + 5]]) as usize;
        if size < HDR || off + size > image.len() {
            break;
        }
        off += size;
        count += 1;
    }
    (count, off)
}

fn parse_store(image: &[u8], start: usize, vars: &mut HashMap<String, Vec<u8>>) {
    let mut off = start;
    while off + HDR <= image.len() && &image[off..off + 4] == SIG {
        let size = u16::from_le_bytes([image[off + 4], image[off + 5]]) as usize;
        if size < HDR || off + size > image.len() {
            break;
        }
        let entry = &image[off..off + size];
        let attrs = entry[9];

        // Only decode valid, named entries. First occurrence wins:
        // find_stores yields the primary store before its backup, so
        // the live copy isn't overwritten by the backup's stale one.
        // Data-only entries are update-chain continuations without a
        // name; skipping them can leave a value stale on boards that
        // use delta updates (noted as a known limit — this AMI build
        // rewrites whole variables, so the named entry is current).
        if attrs & ATTR_VALID != 0
            && attrs & ATTR_DATA_ONLY == 0
            && let Some((name, data)) = parse_entry(entry, attrs)
        {
            vars.entry(name).or_insert(data);
        }
        off += size;
    }
}

/// Entry layout after the 10-byte header: [GUID(16) or guid-index(1)],
/// then Name (ASCII or UCS-2, NUL-terminated), then data.
fn parse_entry(entry: &[u8], attrs: u8) -> Option<(String, Vec<u8>)> {
    let mut p = HDR;
    p += if attrs & ATTR_GUID != 0 { 16 } else { 1 };

    let (name, name_len) = if attrs & ATTR_ASCII_NAME != 0 {
        let end = entry.get(p..)?.iter().position(|&c| c == 0)?;
        (
            String::from_utf8_lossy(&entry[p..p + end]).into_owned(),
            end + 1,
        )
    } else {
        // UCS-2 name.
        let bytes = entry.get(p..)?;
        let mut units = Vec::new();
        let mut i = 0;
        while i + 2 <= bytes.len() {
            let u = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
            i += 2;
            if u == 0 {
                break;
            }
            units.push(u);
        }
        (String::from_utf16_lossy(&units), i)
    };
    p += name_len;

    if name.is_empty() {
        return None;
    }
    // Data is the remainder. Extended-header entries carry a small
    // trailer we don't parse; leave it in rather than mis-trim, since
    // Setup values sit at the front of the blob.
    let data = entry.get(p..)?.to_vec();
    Some((name, data))
}

/// Read a little-endian value of `width` bytes at `offset` in a
/// variable's data — the Setup-value lookup used by the join.
pub fn read_at(data: &[u8], offset: usize, width: u8) -> Option<u64> {
    let w = width as usize;
    let s = data.get(offset..offset + w)?;
    let mut v = [0u8; 8];
    v[..w].copy_from_slice(s);
    Some(u64::from_le_bytes(v))
}

// Silence unused-constant warnings for the attribute bits we match on
// only sometimes but keep documented as a set.
const _: u8 = ATTR_RUNTIME | ATTR_EXT_HEADER | ATTR_HW_ERROR | ATTR_AUTH_WRITE;

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one NVAR entry with an ASCII name + inline GUID.
    fn entry(name: &str, data: &[u8], valid: bool, data_only: bool) -> Vec<u8> {
        let mut attrs = ATTR_ASCII_NAME | ATTR_GUID;
        if valid {
            attrs |= ATTR_VALID;
        }
        if data_only {
            attrs |= ATTR_DATA_ONLY;
        }
        let mut e = Vec::new();
        e.extend_from_slice(SIG);
        e.extend_from_slice(&[0, 0]); // size, patched below
        e.extend_from_slice(&[0xFF, 0xFF, 0xFF]); // next (none)
        e.push(attrs);
        e.extend_from_slice(&[0xAA; 16]); // guid
        e.extend_from_slice(name.as_bytes());
        e.push(0);
        e.extend_from_slice(data);
        let size = e.len() as u16;
        e[4..6].copy_from_slice(&size.to_le_bytes());
        e
    }

    /// Concatenate entries into a store, padding with filler entries to
    /// clear the `MIN_RUN` run-length threshold that rejects stray
    /// coincidental `NVAR` bytes.
    fn store(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut s = Vec::new();
        for e in entries {
            s.extend_from_slice(e);
        }
        for k in 0..MIN_RUN.saturating_sub(entries.len()) {
            s.extend_from_slice(&entry(&format!("_pad{k}"), &[0], true, false));
        }
        s
    }

    #[test]
    fn parses_named_entries_and_reads_values() {
        let mut img = vec![0xFF; 64]; // padding before the store
        img.extend_from_slice(&store(&[
            entry("Setup", &[0x11, 0x22, 0x33, 0x44], true, false),
            entry("Other", &[0x01], true, false),
        ]));
        img.extend(vec![0xFF; 32]);

        let vars = parse(&img);
        assert_eq!(vars["Setup"], vec![0x11, 0x22, 0x33, 0x44]);
        assert_eq!(read_at(&vars["Setup"], 0, 1), Some(0x11));
        assert_eq!(read_at(&vars["Setup"], 2, 2), Some(0x4433));
        assert!(vars.contains_key("Other"));
    }

    #[test]
    fn skips_invalid_and_data_only_entries() {
        let img = store(&[
            entry("Ghost", &[0x00], false, false), // invalid
            entry("Chain", &[0x00], true, true),   // data-only
            entry("Real", &[0x99], true, false),
        ]);
        let vars = parse(&img);
        assert!(!vars.contains_key("Ghost"));
        assert!(!vars.contains_key("Chain"));
        assert_eq!(vars["Real"], vec![0x99]);
    }

    #[test]
    fn first_entry_wins_over_backup() {
        // The primary store is walked first; its live copy must not be
        // clobbered by a later (backup) entry of the same name.
        let img = store(&[
            entry("Setup", &[0x01], true, false),
            entry("Setup", &[0x02], true, false),
        ]);
        let vars = parse(&img);
        assert_eq!(vars["Setup"], vec![0x01]);
    }
}
