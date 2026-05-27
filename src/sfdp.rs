//! Serial Flash Discoverable Parameters (JESD216) parser.
//!
//! SFDP is a self-describing block of metadata that modern SPI NOR
//! chips carry in a separate address space (read via opcode `0x5A`).
//! Parsing it lets etch341 derive a chip's size / page / sector
//! parameters without a DB lookup. The standard's primary table
//! (the JEDEC Basic Flash Parameter Table, or "BFPT") is what we
//! decode here; vendor-specific extension tables are visible via
//! the header walk but their contents aren't interpreted.
//!
//! Spec references in the field-level comments below are to
//! JESD216D (the most widely-implemented revision; later revisions
//! extend BFPT to 23 DWORDs but the first 9 we care about are
//! stable).
//!
//! Wire layout:
//!
//!   offset 0x00 : SFDP Header                  (8 bytes)
//!   offset 0x08 : 1st Parameter Header         (8 bytes)
//!   offset 0x10 : 2nd Parameter Header         (8 bytes) if NPH ≥ 1
//!   ...
//!   <BFPT pointer> : BFPT body                 (≥ 9 DWORDs)
//!   <other table pointers> : vendor tables     (variable)
//!
//! Each header points at its own body somewhere in the SFDP
//! address space; the bodies are not required to be contiguous.

/// Top-level SFDP header at offset 0x00 of the SFDP address space.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    /// True when bytes 0..4 spell "SFDP" (0x50 0x44 0x46 0x53 in
    /// little-endian / 'S' 'F' 'D' 'P' in transmit order). Chips
    /// without SFDP support return `0xFF` for every byte; the
    /// caller treats a `false` here as "no SFDP".
    pub valid: bool,
    pub minor_rev: u8,
    pub major_rev: u8,
    /// Number of Parameter Headers minus one. The 1st header
    /// always exists at offset 0x08 and is always BFPT; later
    /// headers describe vendor / extension tables.
    pub nph: u8,
}

/// One Parameter Header (8 bytes). Each header announces a
/// parameter table living somewhere in the SFDP address space.
#[derive(Debug, Clone, Copy)]
pub struct ParameterHeader {
    /// 16-bit Parameter ID composed as `(msb << 8) | lsb`. JEDEC's
    /// Basic Flash Parameter Table is `0xFF00` (MSB `0xFF`, LSB
    /// `0x00`). Vendor extension IDs sit in the `0x00xx`..`0xFExx`
    /// range; the spec assigns them centrally.
    pub id: u16,
    pub minor_rev: u8,
    pub major_rev: u8,
    /// Table body length in DWORDs (4-byte words).
    pub length_dwords: u8,
    /// 24-bit offset of the table body within SFDP address space.
    pub ptr: u32,
}

impl ParameterHeader {
    /// JEDEC Basic Flash Parameter Table identifier.
    pub const BFPT_ID: u16 = 0xFF00;
}

/// Address-width capability advertised by BFPT DWORD 1 bits 18-19.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addressing {
    /// 3-byte addressing only (chips ≤ 16 MB).
    ThreeByteOnly,
    /// Either mode; default after power-up is 3-byte.
    Either,
    /// 4-byte addressing only (some > 16 MB chips).
    FourByteOnly,
    /// Reserved encoding — unexpected but possible if a vendor
    /// ships a non-conforming table.
    Reserved,
}

/// One entry from BFPT's erase-type table (DWORDs 8-9 carry four
/// entries). A zero `size_bytes` / `opcode` pair means the entry
/// is unused.
#[derive(Debug, Clone, Copy)]
pub struct EraseType {
    pub size_bytes: u32,
    pub opcode: u8,
}

/// Decoded JEDEC Basic Flash Parameter Table.
#[derive(Debug, Clone)]
pub struct Bfpt {
    /// Total chip capacity in bytes, derived from DWORD 2.
    pub size_bytes: u64,
    /// Page size for Page Program (DWORD 11 bits 0-3, encoded as
    /// `2^N`). Almost always 256 on real silicon.
    pub page_size: u32,
    /// What address widths the chip supports.
    pub addressing: Addressing,
    /// 4 KB sector erase opcode (DWORD 1 byte 1). `0xFF` means
    /// the chip doesn't advertise a 4 KB erase.
    pub erase_4k_opcode: u8,
    /// Up to four erase types (DWORDs 8-9). Sorted as they
    /// appear in the table; unused slots have `size_bytes == 0`.
    pub erase_types: [EraseType; 4],
}

/// Result of parsing a fresh SFDP dump.
#[derive(Debug, Clone)]
pub struct Sfdp {
    pub header: Header,
    pub parameter_headers: Vec<ParameterHeader>,
    /// Decoded BFPT body if a `(BFPT_ID, ≥ 1 DWORD)` entry was
    /// present in the parameter headers and its pointed-at body
    /// was within the dumped region. `None` for non-SFDP chips
    /// or oddly truncated tables.
    pub bfpt: Option<Bfpt>,
}

/// Parse a contiguous SFDP dump starting at SFDP offset 0. `data`
/// should be large enough to cover both the header walk and the
/// BFPT body (the spec doesn't require BFPT to follow the headers
/// immediately, but in practice it always does, and 256 bytes is
/// enough for every BFPT revision through JESD216F).
pub fn parse(data: &[u8]) -> Sfdp {
    let header = parse_header(data);
    if !header.valid {
        return Sfdp {
            header,
            parameter_headers: Vec::new(),
            bfpt: None,
        };
    }
    // The spec uses NPH = "number of headers minus one", so the
    // total count is `nph + 1`. Be defensive: a corrupt header
    // could claim hundreds of headers; cap the walk against
    // what the buffer actually holds.
    let count = (header.nph as usize).saturating_add(1);
    let mut headers = Vec::with_capacity(count);
    for i in 0..count {
        let off = 8 + i * 8;
        if off + 8 > data.len() {
            break;
        }
        headers.push(parse_parameter_header(&data[off..off + 8]));
    }
    let bfpt = headers
        .iter()
        .find(|h| h.id == ParameterHeader::BFPT_ID && h.length_dwords >= 1)
        .and_then(|h| parse_bfpt(data, *h));
    Sfdp {
        header,
        parameter_headers: headers,
        bfpt,
    }
}

fn parse_header(data: &[u8]) -> Header {
    if data.len() < 8 {
        return Header {
            valid: false,
            minor_rev: 0,
            major_rev: 0,
            nph: 0,
        };
    }
    // "SFDP" magic transmitted as 'S' 'F' 'D' 'P' — bytes 0..4 of
    // the SFDP address space. Non-SFDP chips return 0xFF for every
    // byte, so the magic check doubles as a presence detector.
    let valid = &data[0..4] == b"SFDP";
    Header {
        valid,
        minor_rev: data[4],
        major_rev: data[5],
        nph: data[6],
    }
}

fn parse_parameter_header(bytes: &[u8]) -> ParameterHeader {
    // Parameter Header layout per JESD216:
    //   byte 0: ID LSB
    //   byte 1: minor rev
    //   byte 2: major rev
    //   byte 3: length (DWORDs)
    //   bytes 4-6: pointer (24-bit, little-endian)
    //   byte 7: ID MSB
    let id_lsb = bytes[0];
    let id_msb = bytes[7];
    let id = (u16::from(id_msb) << 8) | u16::from(id_lsb);
    let ptr = u32::from(bytes[4]) | (u32::from(bytes[5]) << 8) | (u32::from(bytes[6]) << 16);
    ParameterHeader {
        id,
        minor_rev: bytes[1],
        major_rev: bytes[2],
        length_dwords: bytes[3],
        ptr,
    }
}

fn dword_at(data: &[u8], offset: u32) -> Option<u32> {
    let o = offset as usize;
    if o + 4 > data.len() {
        return None;
    }
    Some(
        u32::from(data[o])
            | (u32::from(data[o + 1]) << 8)
            | (u32::from(data[o + 2]) << 16)
            | (u32::from(data[o + 3]) << 24),
    )
}

fn parse_bfpt(data: &[u8], header: ParameterHeader) -> Option<Bfpt> {
    // We need at least 9 DWORDs for the fields we care about
    // (DWORD 8/9 for erase types, DWORD 11 for page size — but
    // DWORD 11 is offset 0x28 = 10 DWORDs in, so we need ≥ 11).
    // Older JESD216 BFPTs only had 9 DWORDs; on those the page
    // size field is missing and we fall back to 256 (universal
    // SPI NOR convention).
    let dw1 = dword_at(data, header.ptr)?;
    let dw2 = dword_at(data, header.ptr + 4)?;

    // ---- DWORD 1 ----
    // bits 8-15: 4 KB Sector Erase Opcode. 0xFF means "not
    // supported" per spec.
    let erase_4k_opcode = ((dw1 >> 8) & 0xFF) as u8;
    // bits 18-19: address bytes.
    //   00 = 3-byte only, 01 = either (default 3), 10 = 4-byte only.
    let addressing = match (dw1 >> 17) & 0b11 {
        0b00 => Addressing::ThreeByteOnly,
        0b01 => Addressing::Either,
        0b10 => Addressing::FourByteOnly,
        _ => Addressing::Reserved,
    };

    // ---- DWORD 2 ----
    // Memory density encoding:
    //   bit 31 = 0: low 31 bits are (capacity_in_bits - 1). Used by
    //               chips ≤ 2 Gbit; nearly all NOR.
    //   bit 31 = 1: low 31 bits are N where capacity = 2^N bits.
    //               Used for larger chips.
    let size_bits = if dw2 & 0x8000_0000 == 0 {
        u64::from(dw2) + 1
    } else {
        1u64 << (dw2 & 0x7FFF_FFFF)
    };
    let size_bytes = size_bits / 8;

    // ---- DWORDs 8-9 (offset +28 .. +35): four Erase Types ----
    // Each entry is (size = 2^N bytes, opcode); a zero entry is
    // unused. Walk both DWORDs; missing-from-dump fields default
    // to "unused".
    let mut erase_types = [EraseType {
        size_bytes: 0,
        opcode: 0,
    }; 4];
    for (i, slot) in erase_types.iter_mut().enumerate() {
        // Pairs live at DWORD 8 [low, high] and DWORD 9 [low, high].
        let dw_idx = 8 + i / 2;
        let dw_off = header.ptr + (dw_idx as u32 - 1) * 4;
        let Some(dw) = dword_at(data, dw_off) else {
            break;
        };
        let half_shift = if i % 2 == 0 { 0 } else { 16 };
        let half = (dw >> half_shift) & 0xFFFF;
        let n = half & 0xFF;
        let op = ((half >> 8) & 0xFF) as u8;
        if n == 0 && op == 0 {
            continue;
        }
        // Erase size of `2^N` bytes. `N = 0` would mean 1 byte which
        // isn't meaningful for sector erase; treat it as unused.
        if n > 0 && n < 32 {
            slot.size_bytes = 1u32 << n;
            slot.opcode = op;
        }
    }

    // ---- DWORD 11 (offset +40): page size + program/erase
    // timings. Bits 0-3 encode page_size = 2^N. Older JESD216
    // tables may not carry DWORD 11; default to 256 (universal
    // SPI NOR convention) when it's missing.
    let page_size = match dword_at(data, header.ptr + 40) {
        Some(dw11) => 1u32 << (dw11 & 0xF),
        None => 256,
    };

    Some(Bfpt {
        size_bytes,
        page_size,
        addressing,
        erase_4k_opcode,
        erase_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All-0xFF data is what a non-SFDP chip returns. Magic check
    /// must reject it cleanly.
    #[test]
    fn ff_buffer_yields_no_sfdp() {
        let dump = vec![0xFF; 64];
        let s = parse(&dump);
        assert!(!s.header.valid);
        assert!(s.parameter_headers.is_empty());
        assert!(s.bfpt.is_none());
    }

    /// Hand-built header + 1 BFPT header + minimal BFPT body for
    /// a fictional 8 Mbit chip with 4 KB + 64 KB erase types and
    /// a 256-byte page.
    #[test]
    fn synthetic_bfpt_decodes() {
        let mut d = vec![0u8; 256];
        // Top header: "SFDP" + minor 6 + major 1 + NPH 0 + 0xFF.
        d[0..4].copy_from_slice(b"SFDP");
        d[4] = 0x06;
        d[5] = 0x01;
        d[6] = 0x00; // NPH = 0 → 1 parameter header total
        d[7] = 0xFF;

        // BFPT parameter header at offset 0x08:
        //   ID LSB 0x00, minor 6, major 1, length 16 DWORDs,
        //   pointer 0x000040, ID MSB 0xFF.
        d[8] = 0x00;
        d[9] = 0x06;
        d[10] = 0x01;
        d[11] = 16;
        d[12] = 0x40;
        d[13] = 0x00;
        d[14] = 0x00;
        d[15] = 0xFF;

        // BFPT body at 0x40:
        //   DWORD 1: 4 KB opcode=0x20 (byte 1), addressing=01 (either).
        //   bit layout: bits 8-15 = 0x20, bits 18-19 = 0b01 → bit 18 set.
        let dw1 = (0x20u32 << 8) | (0b01 << 17);
        d[0x40..0x44].copy_from_slice(&dw1.to_le_bytes());

        //   DWORD 2: density. 8 Mbit = 8,388,608 bits → density = 8,388,607.
        let dw2 = 8_388_607u32;
        d[0x44..0x48].copy_from_slice(&dw2.to_le_bytes());

        // DWORD 8 (offset 0x40 + 28 = 0x5C): two erase types.
        //   Type 1: 4 KB → N=12 (2^12 = 4096), opcode 0x20.
        //   Type 2: 64 KB → N=16, opcode 0xD8.
        let dw8 = 12u32 | (0x20u32 << 8) | (16u32 << 16) | (0xD8u32 << 24);
        d[0x5C..0x60].copy_from_slice(&dw8.to_le_bytes());

        // DWORD 11 (offset 0x40 + 40 = 0x68): page size N=8 → 256.
        let dw11 = 8u32;
        d[0x68..0x6C].copy_from_slice(&dw11.to_le_bytes());

        let s = parse(&d);
        assert!(s.header.valid);
        assert_eq!(s.header.major_rev, 1);
        assert_eq!(s.header.minor_rev, 6);
        assert_eq!(s.parameter_headers.len(), 1);
        let bfpt = s.bfpt.expect("BFPT should decode");
        assert_eq!(bfpt.size_bytes, 1024 * 1024); // 8 Mbit = 1 MB
        assert_eq!(bfpt.page_size, 256);
        assert_eq!(bfpt.addressing, Addressing::Either);
        assert_eq!(bfpt.erase_4k_opcode, 0x20);
        assert_eq!(bfpt.erase_types[0].size_bytes, 4096);
        assert_eq!(bfpt.erase_types[0].opcode, 0x20);
        assert_eq!(bfpt.erase_types[1].size_bytes, 65536);
        assert_eq!(bfpt.erase_types[1].opcode, 0xD8);
        assert_eq!(bfpt.erase_types[2].size_bytes, 0);
        assert_eq!(bfpt.erase_types[3].size_bytes, 0);
    }

    /// Bit-31 density encoding (used by very large chips). Encode
    /// 4 Gbit as `1 << 32` bits which is too big for u32-bit-0,
    /// so the encoding switches to "2^N" form.
    #[test]
    fn density_high_bit_form() {
        // 4 Gbit = 2^32 bits → density value = (1 << 31) | 32.
        let dw2 = 0x8000_0020u32;
        // Other DWORDs minimal but valid.
        let mut d = vec![0u8; 256];
        d[0..4].copy_from_slice(b"SFDP");
        d[6] = 0x00;
        d[7] = 0xFF;
        d[8] = 0x00;
        d[11] = 16;
        d[12] = 0x40;
        d[15] = 0xFF;
        d[0x40..0x44].fill(0);
        d[0x44..0x48].copy_from_slice(&dw2.to_le_bytes());
        let s = parse(&d);
        let bfpt = s.bfpt.expect("BFPT should decode");
        // 2^32 bits / 8 = 2^29 bytes = 512 MB. That's our expected
        // size for a 4 Gbit part.
        assert_eq!(bfpt.size_bytes, 1u64 << 29);
    }
}
