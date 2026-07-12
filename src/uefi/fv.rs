//! Firmware-volume walker: scan a raw flash image for UEFI firmware
//! volumes (`_FVH`), iterate their FFS files, recurse into sections
//! (decompressing LZMA via `lzma-rs`), and hand back every leaf
//! section's payload. Tolerant by design: anything unparseable is
//! recorded in `Walk::skipped` and the walk continues — a BIOS dump
//! full of vendor NVRAM volumes must never abort the whole scan.
//!
//! Layouts follow the UEFI Platform Initialization spec (vol. 3);
//! UEFITool's source is the cross-reference for real-world quirks.

/// One leaf section payload found somewhere in the image, with just
/// enough provenance for later phases (HII scan) and for debugging.
pub struct Leaf {
    pub file_guid: [u8; 16],
    pub file_type: u8,
    /// UCS-2 name from the file's UI section, when present.
    pub file_name: Option<String>,
    pub section_type: u8,
    pub data: Vec<u8>,
}

/// Result of walking an image.
#[derive(Default)]
pub struct Walk {
    pub fv_count: usize,
    pub file_count: usize,
    pub leaves: Vec<Leaf>,
    /// Human-readable notes for anything skipped (unsupported
    /// compression, malformed headers, …).
    pub skipped: Vec<String>,
}

// Section types (PI spec vol. 3).
const SECTION_COMPRESSION: u8 = 0x01;
const SECTION_GUID_DEFINED: u8 = 0x02;
const SECTION_USER_INTERFACE: u8 = 0x15;
const SECTION_FV_IMAGE: u8 = 0x17;
const SECTION_RAW: u8 = 0x19;

// FFS file types.
const FILE_RAW: u8 = 0x01;
const FILE_PAD: u8 = 0xF0;

// GUID-defined section GUIDs (stored little-endian, as on disk).
const GUID_LZMA: [u8; 16] = [
    0x98, 0x58, 0x4E, 0xEE, 0x14, 0x39, 0x59, 0x42, 0x9D, 0x6E, 0xDC, 0x7B, 0xD7, 0x94, 0x03, 0xCF,
];
const GUID_CRC32: [u8; 16] = [
    0xB0, 0xCD, 0x1B, 0xFC, 0x31, 0x7D, 0xAA, 0x49, 0x93, 0x6A, 0xA4, 0x60, 0x0D, 0x9D, 0xD0, 0x83,
];

const GUID_DEFINED_PROCESSING_REQUIRED: u16 = 0x01;
const MAX_DEPTH: usize = 16;

/// Walk a raw flash image: find every firmware volume and collect all
/// leaf sections. This is the Phase-1 entry point.
pub fn walk_image(image: &[u8]) -> Walk {
    let mut w = Walk::default();
    walk_buf(image, 0, &mut w);
    w
}

/// Scan `buf` for firmware volumes and walk each. Used both for the
/// top-level image and for nested FV-image sections.
fn walk_buf(buf: &[u8], depth: usize, w: &mut Walk) {
    if depth > MAX_DEPTH {
        w.skipped.push("max nesting depth exceeded".into());
        return;
    }
    let mut claimed_end = 0usize;
    for start in scan_fv_offsets(buf) {
        if start < claimed_end {
            continue; // inside a volume we already walked
        }
        let fv_len = u64::from_le_bytes(buf[start + 32..start + 40].try_into().unwrap()) as usize;
        claimed_end = start + fv_len;
        walk_fv(&buf[start..start + fv_len], depth, w);
    }
}

/// Find plausible FV start offsets: `_FVH` signature at +0x28, sane
/// revision + lengths, and a header whose u16 checksum sums to zero.
fn scan_fv_offsets(buf: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = find(&buf[pos..], b"_FVH") {
        let sig = pos + rel;
        pos = sig + 4;
        let Some(start) = sig.checked_sub(0x28) else {
            continue;
        };
        let Some(header) = buf.get(start..start + 56) else {
            continue;
        };
        let revision = header[55];
        let fv_len = u64::from_le_bytes(header[32..40].try_into().unwrap()) as usize;
        let header_len = u16::from_le_bytes(header[48..50].try_into().unwrap()) as usize;
        if !(1..=3).contains(&revision)
            || header_len < 56
            || !header_len.is_multiple_of(2)
            || fv_len < header_len
            || start + fv_len > buf.len()
        {
            continue;
        }
        let Some(full_header) = buf.get(start..start + header_len) else {
            continue;
        };
        let sum: u16 = full_header
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .fold(0u16, |a, v| a.wrapping_add(v));
        if sum == 0 {
            out.push(start);
        }
    }
    out
}

/// Walk one firmware volume: iterate FFS files, recurse sections.
fn walk_fv(fv: &[u8], depth: usize, w: &mut Walk) {
    w.fv_count += 1;
    let header_len = u16::from_le_bytes(fv[48..50].try_into().unwrap()) as usize;
    let ext_off = u16::from_le_bytes(fv[52..54].try_into().unwrap()) as usize;

    // Files start after the header — or after the extended header when
    // one is present — aligned to 8 bytes.
    let mut off = if ext_off != 0 {
        match fv.get(ext_off + 16..ext_off + 20) {
            Some(sz) => align8(ext_off + u32::from_le_bytes(sz.try_into().unwrap()) as usize),
            None => {
                w.skipped.push("FV ext header out of bounds".into());
                return;
            }
        }
    } else {
        align8(header_len)
    };

    while off + 24 <= fv.len() {
        let header = &fv[off..off + 24];
        if header.iter().all(|&b| b == 0xFF) {
            break; // erased free space — end of files
        }
        let file_guid: [u8; 16] = header[0..16].try_into().unwrap();
        let file_type = header[18];
        let attributes = header[19];
        let size24 = u32::from_le_bytes([header[20], header[21], header[22], 0]) as usize;

        // FFSv3 large file: real size is a u64 after the header.
        let (file_size, data_off) = if attributes & 0x01 != 0 && size24 == 0 {
            match fv.get(off + 24..off + 32) {
                Some(sz) => (u64::from_le_bytes(sz.try_into().unwrap()) as usize, 32),
                None => break,
            }
        } else {
            (size24, 24)
        };

        if file_size < data_off || off + file_size > fv.len() {
            w.skipped
                .push(format!("malformed FFS file at FV offset {off:#x}"));
            break;
        }
        w.file_count += 1;
        let body = &fv[off + data_off..off + file_size];

        match file_type {
            FILE_PAD => {}
            // Raw files have no section structure; surface the body
            // as a single raw leaf.
            FILE_RAW => w.leaves.push(Leaf {
                file_guid,
                file_type,
                file_name: None,
                section_type: SECTION_RAW,
                data: body.to_vec(),
            }),
            _ => {
                let mut file_leaves = Vec::new();
                parse_sections(body, depth, w, &mut file_leaves, file_guid, file_type);
                let name = file_leaves
                    .iter()
                    .find(|l| l.section_type == SECTION_USER_INTERFACE)
                    .map(|l| ucs2_to_string(&l.data));
                for mut leaf in file_leaves {
                    leaf.file_name = name.clone();
                    w.leaves.push(leaf);
                }
            }
        }
        off = align8(off + file_size);
    }
}

/// Recurse a section stream, decompressing containers and collecting
/// leaves into `out` (so the caller can attach the file's UI name).
fn parse_sections(
    data: &[u8],
    depth: usize,
    w: &mut Walk,
    out: &mut Vec<Leaf>,
    file_guid: [u8; 16],
    file_type: u8,
) {
    if depth > MAX_DEPTH {
        w.skipped.push("max nesting depth exceeded".into());
        return;
    }
    let mut off = 0;
    while off + 4 <= data.len() {
        let size24 = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], 0]) as usize;
        let stype = data[off + 3];
        // Section2 (large section): 3-byte size saturated, real u32 follows.
        let (sec_size, hdr) = if size24 == 0xFF_FFFF {
            match data.get(off + 4..off + 8) {
                Some(sz) => (u32::from_le_bytes(sz.try_into().unwrap()) as usize, 8),
                None => break,
            }
        } else {
            (size24, 4)
        };
        if sec_size < hdr || off + sec_size > data.len() {
            w.skipped
                .push(format!("malformed section at offset {off:#x}"));
            break;
        }
        let body = &data[off + hdr..off + sec_size];

        match stype {
            SECTION_COMPRESSION => {
                if body.len() < 5 {
                    w.skipped.push("truncated compression section".into());
                } else {
                    let orig_len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                    match body[4] {
                        0 => parse_sections(&body[5..], depth + 1, w, out, file_guid, file_type),
                        // EFI_STANDARD_COMPRESSION — either the UEFI or the
                        // Tiano variant of the EDK2 Huffman+LZ codec (they
                        // differ only in a position-code bit width); try
                        // both and take whichever decodes.
                        1 => match efi_decompress(&body[5..], orig_len as usize) {
                            Some(dec) => {
                                parse_sections(&dec, depth + 1, w, out, file_guid, file_type)
                            }
                            None => w
                                .skipped
                                .push("EFI-compressed section failed to decode".into()),
                        },
                        // Insyde tags a plain LZMA1 (.lzma) stream as
                        // "customized compression" (type 2) instead of using
                        // a GUID-defined LZMA section. Same codec.
                        2 => match lzma_decompress(&body[5..]) {
                            Ok(dec) => {
                                parse_sections(&dec, depth + 1, w, out, file_guid, file_type)
                            }
                            Err(e) => w
                                .skipped
                                .push(format!("customized(LZMA) section failed: {e}")),
                        },
                        t => w
                            .skipped
                            .push(format!("compression type {t} (unsupported)")),
                    }
                }
            }
            SECTION_GUID_DEFINED => {
                if body.len() < 20 {
                    w.skipped.push("truncated GUID-defined section".into());
                } else {
                    let guid: [u8; 16] = body[0..16].try_into().unwrap();
                    // DataOffset counts from the section header, we index
                    // from the body start.
                    let data_off =
                        (u16::from_le_bytes([body[16], body[17]]) as usize).saturating_sub(hdr);
                    let attrs = u16::from_le_bytes([body[18], body[19]]);
                    let Some(inner) = body.get(data_off..) else {
                        w.skipped
                            .push("GUID-defined DataOffset out of bounds".into());
                        off = align4(off + sec_size);
                        continue;
                    };
                    if guid == GUID_LZMA {
                        match lzma_decompress(inner) {
                            Ok(dec) => {
                                parse_sections(&dec, depth + 1, w, out, file_guid, file_type)
                            }
                            Err(e) => w.skipped.push(format!("LZMA decompress failed: {e}")),
                        }
                    } else if guid == GUID_CRC32 || attrs & GUID_DEFINED_PROCESSING_REQUIRED == 0 {
                        // CRC32-wrapped or no processing required: the
                        // payload is plain sections.
                        parse_sections(inner, depth + 1, w, out, file_guid, file_type);
                    } else {
                        w.skipped.push(format!(
                            "GUID-defined section {} (unsupported)",
                            fmt_guid(&guid)
                        ));
                    }
                }
            }
            SECTION_FV_IMAGE => walk_buf(body, depth + 1, w),
            _ => out.push(Leaf {
                file_guid,
                file_type,
                file_name: None,
                section_type: stype,
                data: body.to_vec(),
            }),
        }
        off = align4(off + sec_size);
    }
}

fn lzma_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut std::io::Cursor::new(data), &mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Decode an EFI_STANDARD_COMPRESSION payload (the codec's own
/// CompSize+OrigSize header must lead `src`). `orig_len` is the
/// section's declared uncompressed length. Tries the Tiano then UEFI
/// variant — the section header doesn't say which, and they share the
/// stream format apart from one bit-width. Returns None if neither
/// decodes.
fn efi_decompress(src: &[u8], orig_len: usize) -> Option<Vec<u8>> {
    use uefi_decompress::{DecompressionAlgorithm as Algo, decompress_into_with_algo};
    for algo in [Algo::TianoDecompress, Algo::UefiDecompress] {
        let mut dst = vec![0u8; orig_len];
        if decompress_into_with_algo(src, &mut dst, algo).is_ok() {
            return Some(dst);
        }
    }
    None
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}
fn align8(x: usize) -> usize {
    (x + 7) & !7
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Decode a NUL-terminated little-endian UCS-2 string.
fn ucs2_to_string(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// `AABBCCDD-EEFF-...` display form of an on-disk (mixed-endian) GUID.
pub fn fmt_guid(g: &[u8; 16]) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        u32::from_le_bytes(g[0..4].try_into().unwrap()),
        u16::from_le_bytes([g[4], g[5]]),
        u16::from_le_bytes([g[6], g[7]]),
        g[8],
        g[9],
        g[10],
        g[11],
        g[12],
        g[13],
        g[14],
        g[15],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_GUID: [u8; 16] = [0x11; 16];

    /// Build a section: 3-byte size + type + body.
    fn section(stype: u8, body: &[u8]) -> Vec<u8> {
        let size = body.len() + 4;
        assert!(size < 0xFF_FFFF);
        let mut s = vec![
            (size & 0xFF) as u8,
            ((size >> 8) & 0xFF) as u8,
            ((size >> 16) & 0xFF) as u8,
            stype,
        ];
        s.extend_from_slice(body);
        s
    }

    /// Concatenate sections with 4-byte alignment padding.
    fn section_stream(sections: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for s in sections {
            while out.len() % 4 != 0 {
                out.push(0);
            }
            out.extend_from_slice(s);
        }
        out
    }

    /// Build an FFS file (24-byte header + body).
    fn ffs_file(guid: [u8; 16], ftype: u8, body: &[u8]) -> Vec<u8> {
        let size = 24 + body.len();
        let mut f = Vec::new();
        f.extend_from_slice(&guid);
        f.extend_from_slice(&[0, 0]); // integrity check
        f.push(ftype);
        f.push(0); // attributes
        f.extend_from_slice(&[
            (size & 0xFF) as u8,
            ((size >> 8) & 0xFF) as u8,
            ((size >> 16) & 0xFF) as u8,
        ]);
        f.push(0xF8); // state
        f.extend_from_slice(body);
        f
    }

    /// Build a minimal firmware volume containing `files`.
    fn fv(files: &[Vec<u8>]) -> Vec<u8> {
        let header_len = 64usize; // 56-byte header + one 8-byte block-map terminator
        let mut body = Vec::new();
        for f in files {
            while body.len() % 8 != 0 {
                body.push(0xFF);
            }
            body.extend_from_slice(f);
        }
        let fv_len = header_len + body.len();

        let mut h = vec![0u8; header_len];
        // [0..16] zero vector, [16..32] filesystem GUID (FFSv2)
        h[16..32].copy_from_slice(&[
            0x78, 0xE5, 0x8C, 0x8C, 0x3D, 0x8A, 0x1C, 0x4F, 0x99, 0x35, 0x89, 0x61, 0x85, 0xC3,
            0x2D, 0xD3,
        ]);
        h[32..40].copy_from_slice(&(fv_len as u64).to_le_bytes());
        h[40..44].copy_from_slice(b"_FVH");
        h[48..50].copy_from_slice(&(header_len as u16).to_le_bytes());
        h[55] = 2; // revision
        // Fix up the header checksum so the u16 sum is zero.
        let sum: u16 = h
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .fold(0u16, |a, v| a.wrapping_add(v));
        h[50..52].copy_from_slice(&(0u16.wrapping_sub(sum)).to_le_bytes());

        h.extend_from_slice(&body);
        h
    }

    fn ui_section(name: &str) -> Vec<u8> {
        let mut body: Vec<u8> = name.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        body.extend_from_slice(&[0, 0]);
        section(SECTION_USER_INTERFACE, &body)
    }

    #[test]
    fn walks_plain_file_with_ui_name() {
        let payload = b"hello uefi".to_vec();
        let body = section_stream(&[section(SECTION_RAW, &payload), ui_section("TestFile")]);
        let image = fv(&[ffs_file(TEST_GUID, 0x07, &body)]);

        let w = walk_image(&image);
        assert_eq!(w.fv_count, 1);
        assert_eq!(w.file_count, 1);
        assert!(w.skipped.is_empty(), "skipped: {:?}", w.skipped);
        let raw = w
            .leaves
            .iter()
            .find(|l| l.section_type == SECTION_RAW)
            .unwrap();
        assert_eq!(raw.data, payload);
        assert_eq!(raw.file_name.as_deref(), Some("TestFile"));
    }

    #[test]
    fn decompresses_lzma_guid_section() {
        let payload = vec![0xAB; 300];
        let inner = section_stream(&[section(SECTION_RAW, &payload)]);
        let mut compressed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(&inner), &mut compressed).unwrap();

        let mut body = Vec::new();
        body.extend_from_slice(&GUID_LZMA);
        body.extend_from_slice(&24u16.to_le_bytes()); // DataOffset (4 hdr + 20 guid-def)
        body.extend_from_slice(&GUID_DEFINED_PROCESSING_REQUIRED.to_le_bytes());
        body.extend_from_slice(&compressed);
        let image = fv(&[ffs_file(
            TEST_GUID,
            0x07,
            &section_stream(&[section(SECTION_GUID_DEFINED, &body)]),
        )]);

        let w = walk_image(&image);
        assert!(w.skipped.is_empty(), "skipped: {:?}", w.skipped);
        assert_eq!(w.leaves.len(), 1);
        assert_eq!(w.leaves[0].data, payload);
    }

    #[test]
    fn decompresses_type2_customized_lzma_section() {
        // Insyde ships LZMA1 in a compression section tagged type 2.
        let payload = vec![0xCD; 300];
        let inner = section_stream(&[section(SECTION_RAW, &payload)]);
        let mut compressed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(&inner), &mut compressed).unwrap();

        let mut body = (inner.len() as u32).to_le_bytes().to_vec();
        body.push(2); // customized (LZMA) compression
        body.extend_from_slice(&compressed);
        let image = fv(&[ffs_file(
            TEST_GUID,
            0x07,
            &section_stream(&[section(SECTION_COMPRESSION, &body)]),
        )]);

        let w = walk_image(&image);
        assert!(w.skipped.is_empty(), "skipped: {:?}", w.skipped);
        assert_eq!(w.leaves.len(), 1);
        assert_eq!(w.leaves[0].data, payload);
    }

    #[test]
    fn uncompressed_wrapper_recurses() {
        let payload = b"wrapped".to_vec();
        let inner = section_stream(&[section(SECTION_RAW, &payload)]);
        let mut body = (inner.len() as u32).to_le_bytes().to_vec();
        body.push(0); // compression type: none
        body.extend_from_slice(&inner);
        let image = fv(&[ffs_file(
            TEST_GUID,
            0x07,
            &section_stream(&[section(SECTION_COMPRESSION, &body)]),
        )]);

        let w = walk_image(&image);
        assert_eq!(w.leaves.len(), 1);
        assert_eq!(w.leaves[0].data, payload);
    }

    #[test]
    fn undecodable_compressed_section_is_skipped_not_fatal() {
        // Garbage that can't decode as either EFI/Tiano variant: the
        // walk must record a skip and keep going, not abort. The codec
        // stream claims an absurd compressed size (> the data present),
        // which both variants reject up front.
        let mut body = 100u32.to_le_bytes().to_vec();
        body.push(1); // EFI_STANDARD_COMPRESSION
        body.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // CompSize (absurd)
        body.extend_from_slice(&100u32.to_le_bytes()); // OrigSize
        body.extend_from_slice(&[0; 8]);
        let image = fv(&[ffs_file(
            TEST_GUID,
            0x07,
            &section_stream(&[section(SECTION_COMPRESSION, &body)]),
        )]);

        let w = walk_image(&image);
        assert_eq!(w.file_count, 1);
        assert!(w.skipped.iter().any(|s| s.contains("failed to decode")));
    }

    #[test]
    fn nested_fv_image_section() {
        let payload = b"inner payload".to_vec();
        let inner_fv = fv(&[ffs_file(
            [0x22; 16],
            0x07,
            &section_stream(&[section(SECTION_RAW, &payload)]),
        )]);
        let outer = fv(&[ffs_file(
            TEST_GUID,
            0x0B, // FV image file
            &section_stream(&[section(SECTION_FV_IMAGE, &inner_fv)]),
        )]);

        let w = walk_image(&outer);
        assert_eq!(w.fv_count, 2);
        assert_eq!(w.leaves.len(), 1);
        assert_eq!(w.leaves[0].data, payload);
    }

    #[test]
    fn bad_checksum_fv_rejected() {
        let mut image = fv(&[ffs_file(TEST_GUID, 0x07, &[])]);
        image[50] ^= 0xFF; // corrupt the checksum
        let w = walk_image(&image);
        assert_eq!(w.fv_count, 0);
    }

    #[test]
    fn garbage_between_volumes_tolerated() {
        let payload = b"x".to_vec();
        let one = fv(&[ffs_file(
            TEST_GUID,
            0x07,
            &section_stream(&[section(SECTION_RAW, &payload)]),
        )]);
        let mut image = vec![0xFF; 512];
        image.extend_from_slice(&one);
        image.extend(vec![0xA5; 300]);
        image.extend_from_slice(&one);
        let w = walk_image(&image);
        assert_eq!(w.fv_count, 2);
        assert_eq!(w.leaves.len(), 2);
    }

    /// Run against a real dump: ETCH341_UEFI_TEST_IMAGE=/path cargo test -- --ignored
    #[test]
    #[ignore]
    fn real_dump_smoke() {
        let path = std::env::var("ETCH341_UEFI_TEST_IMAGE").expect("set ETCH341_UEFI_TEST_IMAGE");
        let image = std::fs::read(path).unwrap();
        let w = walk_image(&image);
        println!(
            "FVs: {}, files: {}, leaves: {}, skipped: {}",
            w.fv_count,
            w.file_count,
            w.leaves.len(),
            w.skipped.len()
        );
        for s in &w.skipped {
            println!("  skipped: {s}");
        }
        for l in w.leaves.iter().filter(|l| l.file_name.is_some()).take(20) {
            println!(
                "  {} ({})",
                l.file_name.as_deref().unwrap(),
                fmt_guid(&l.file_guid)
            );
        }
        assert!(w.fv_count > 0, "no firmware volumes found");
    }
}
