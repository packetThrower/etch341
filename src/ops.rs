//! High-level operations: detect / read / erase / write / verify / blank_check.
//!
//! Each op takes a `&mut dyn SpiTransport` so it can be unit-tested
//! against a mock. Only `detect` opens the real hardware itself.

use crate::ch341::Ch341;
use crate::chipdb::{Chip, ChipDb};
use crate::cli::GlobalOpts;
use crate::error::{Error, Result};
use crate::spi::{self, Addressing, SpiTransport};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// Long-op progress channel. The CLI implementation drives an
/// `indicatif::ProgressBar`; the GUI's implementation writes to an
/// `Arc<AtomicU64>` pair the render thread polls. All methods have
/// no-op defaults so an `impl ProgressSink for MyType {}` is enough.
pub trait ProgressSink: Send {
    /// Called once at the start of an operation with the total work
    /// expected (bytes for read/write/verify/blank, sectors for erase).
    fn start(&mut self, total: u64) {
        let _ = total;
    }
    /// Called periodically as work progresses.
    fn update(&mut self, current: u64) {
        let _ = current;
    }
    /// Called once when work completes (success or failure path).
    fn finish(&mut self) {}
}

/// Default sink that swallows all events. Useful for tests and for
/// callers that don't care about progress.
#[allow(dead_code)] // kept available for callers that don't care about progress
#[derive(Default)]
pub struct NullSink;
impl ProgressSink for NullSink {}

const CHIP_ERASE_TIMEOUT: Duration = Duration::from_secs(300);
const BLOCK_ERASE_TIMEOUT: Duration = Duration::from_secs(10);
const SECTOR_ERASE_TIMEOUT: Duration = Duration::from_secs(2);
const PAGE_PROGRAM_TIMEOUT: Duration = Duration::from_secs(1);

const READ_CHUNK: u32 = 4096;

/// Pick the address width for chip-side opcodes. Threshold is 16 MB
/// (16384 KB) — at or below uses standard 3-byte opcodes; anything
/// larger needs the 4-byte variants (0x13 / 0x12 / 0x21 / 0xDC).
fn addressing_for(chip: &Chip) -> Addressing {
    if chip.size_kb > 16384 {
        Addressing::FourByte
    } else {
        Addressing::ThreeByte
    }
}

/// Structured detect result. CLI formats it for stdout; GUI uses the
/// fields to drive its connection state and activity log.
#[derive(Debug, Clone)]
pub struct DetectResult {
    pub jedec_id: [u8; 3],
    pub diagnosis: Diagnosis,
}

#[derive(Debug, Clone)]
pub enum Diagnosis {
    MisoStuckLow,
    MisoFloatsHigh,
    Known(Chip),
    UnknownChip,
}

impl DetectResult {
    pub fn jedec_string(&self) -> String {
        format!(
            "{:02X}{:02X}{:02X}",
            self.jedec_id[0], self.jedec_id[1], self.jedec_id[2]
        )
    }
}

/// JEDEC read + DB lookup, no printing. Caller (CLI or GUI) decides how
/// to surface the result.
pub fn run_detect(spi: &mut dyn SpiTransport) -> Result<DetectResult> {
    let id = spi::jedec_read(spi)?;
    let diagnosis = match id {
        [0x00, 0x00, 0x00] => Diagnosis::MisoStuckLow,
        [0xFF, 0xFF, 0xFF] => Diagnosis::MisoFloatsHigh,
        _ => {
            let jedec = format!("{:02X}{:02X}{:02X}", id[0], id[1], id[2]);
            match ChipDb::load_embedded().find_by_jedec(&jedec) {
                Some(c) => Diagnosis::Known(c.clone()),
                None => Diagnosis::UnknownChip,
            }
        }
    };
    Ok(DetectResult {
        jedec_id: id,
        diagnosis,
    })
}

pub fn detect(global: &GlobalOpts) -> Result<()> {
    let mut ch = Ch341::open(global.verbose)?;
    let result = run_detect(&mut ch)?;
    println!("JEDEC ID : 0x{}", result.jedec_string());
    match &result.diagnosis {
        Diagnosis::MisoStuckLow => {
            println!("Chip     : MISO stuck low: target board likely fighting us.");
            println!("           Lift pin 8 (VCC) or remove the chip from the board.");
        }
        Diagnosis::MisoFloatsHigh => {
            println!("Chip     : MISO floats high: no chip detected.");
            println!("           Check clip orientation, VCC jumper (3.3V), and chip power.");
        }
        Diagnosis::Known(c) => print_chip_facts(c),
        Diagnosis::UnknownChip => {
            // JEDEC isn't in chips.toml; same SFDP fallback the
            // destructive / read ops use. Lets `detect` answer
            // "what chip is this?" for an uncatalogued part
            // instead of dead-ending at "unknown". The
            // "(SFDP)" suffix on the synthesised name keeps the
            // signal that this didn't come from the bundled DB,
            // and the chips.toml hint stays so the user knows
            // they can curate it.
            let jedec = result.jedec_string();
            match synthesize_from_sfdp(&mut ch, &jedec)? {
                Some(c) => {
                    println!(
                        "Chip     : {} (no chips.toml entry; parameters from SFDP)",
                        c.name
                    );
                    print_chip_facts(&c);
                    println!("           Consider adding an entry to chips/chips.toml.");
                }
                None => {
                    println!("Chip     : unknown (JEDEC 0x{jedec} not in chips.toml)");
                    println!("           Chip has no SFDP either; add an entry to");
                    println!("           chips/chips.toml or pass --chip <NAME>.");
                }
            }
        }
    }
    Ok(())
}

/// Shared "Size / Notes" printer used by both the DB-hit and
/// SFDP-fallback branches of `detect`. Factored out so the two
/// paths produce identical formatting and the SFDP-derived chip's
/// auto-generated notes get the same prefix as a curated entry's.
fn print_chip_facts(c: &Chip) {
    println!(
        "Size     : {} KB ({} pages of {} B, {} sectors of {} B)",
        c.size_kb,
        (c.size_kb as u64 * 1024) / c.page_size as u64,
        c.page_size,
        (c.size_kb as u64 * 1024) / c.sector_size as u64,
        c.sector_size,
    );
    if !c.notes.is_empty() {
        println!("Notes    : {}", c.notes);
    }
}

/// Read SR1/SR2/SR3 from the chip and print the raw bytes plus a
/// decoded summary of the W25Q-family bit names. Standalone op —
/// doesn't depend on chip-name lookup, just needs the chip to ACK
/// SPI and respond to the read-status opcodes.
pub fn status(global: &GlobalOpts) -> Result<()> {
    let mut ch = Ch341::open(global.verbose)?;
    // Run a JEDEC probe first so the user gets a clear "no chip"
    // message when MISO is floating instead of a decoded SR1 of
    // 0xFF reading as "WIP=1 WEL=1 BP=7 …" (every bit set looks
    // alarmingly like real protected state, but it's actually
    // just MISO pulled high through the CH341 with nothing
    // driving it). The detect message points at the most common
    // physical-setup mistakes.
    let detect = run_detect(&mut ch)?;
    println!("JEDEC ID : 0x{}", detect.jedec_string());
    match &detect.diagnosis {
        Diagnosis::MisoStuckLow => {
            println!("Chip     : MISO stuck low: target board likely fighting us.");
            println!("           Lift pin 8 (VCC) or remove the chip from the board.");
            return Ok(());
        }
        Diagnosis::MisoFloatsHigh => {
            println!("Chip     : MISO floats high: no chip detected.");
            println!("           Check clip orientation, VCC jumper (3.3V), and chip power.");
            return Ok(());
        }
        Diagnosis::Known(c) => println!("Chip     : {}", c.name),
        Diagnosis::UnknownChip => println!("Chip     : (JEDEC ID not in DB; reading SR anyway)"),
    }
    println!();
    let regs = spi::read_all_status(&mut ch)?;
    print_status(&regs);
    Ok(())
}

/// Pretty-printer split out so the GUI Status pane can format the
/// same decoded view by reusing the formatting helpers below
/// without going through stdout. (The print itself stays here so
/// the CLI dispatcher doesn't need to know about the layout.)
fn print_status(regs: &spi::StatusRegisters) {
    println!("SR1 : 0x{:02X}  (0b{:08b})", regs.sr1, regs.sr1);
    println!(
        "        WIP={} WEL={} BP={} TB={} SEC/BP3={} SRP0={}",
        bit(regs.wip()),
        bit(regs.wel()),
        regs.bp(),
        bit(regs.tb()),
        bit(regs.sec_or_bp3()),
        bit(regs.srp0()),
    );
    if regs.sr2_present() {
        println!("SR2 : 0x{:02X}  (0b{:08b})", regs.sr2, regs.sr2);
        println!(
            "        SRP1={} QE={} LB={} CMP={} SUS={}",
            bit(regs.srp1()),
            bit(regs.qe()),
            regs.lb(),
            bit(regs.cmp()),
            bit(regs.sus()),
        );
    } else {
        println!("SR2 : 0xFF    (chip didn't respond, likely doesn't implement SR2)");
    }
    if regs.sr3_present() {
        println!("SR3 : 0x{:02X}  (0b{:08b})", regs.sr3, regs.sr3);
        println!(
            "        ADP={} WPS={} DRV={} HOLD/RST={}",
            bit(regs.adp()),
            bit(regs.wps()),
            regs.drv(),
            bit(regs.hold_rst()),
        );
    } else {
        println!("SR3 : 0xFF    (chip didn't respond, likely doesn't implement SR3)");
    }
    // Two common gotchas worth surfacing without forcing the user
    // to know the bit semantics: a non-zero BP mask silently fails
    // writes, and writes to the SR itself need SRP0/SRP1 to permit
    // it. Anything else is left to the decoded view above.
    if regs.bp() != 0 || regs.sec_or_bp3() {
        println!();
        println!("note   : SR1 has block-protect bits set: writes and erases to the protected");
        println!("         range will silently fail. Clear BP[2:0] (and SEC/BP3 if set) via");
        println!("         WRSR before programming.");
    }
}

fn bit(b: bool) -> char {
    if b { '1' } else { '0' }
}

/// Read the chip's SFDP table, parse the JEDEC Basic Flash
/// Parameter Table out of it, and print both the raw header walk
/// and the decoded BFPT. Same JEDEC-first guard as `status` so
/// the "no chip" case surfaces a clean message instead of an
/// all-`0xFF` SFDP dump that decodes as garbage.
pub fn sfdp(global: &GlobalOpts) -> Result<()> {
    let mut ch = Ch341::open(global.verbose)?;
    let detect = run_detect(&mut ch)?;
    println!("JEDEC ID : 0x{}", detect.jedec_string());
    match &detect.diagnosis {
        Diagnosis::MisoStuckLow => {
            println!("Chip     : MISO stuck low: target board likely fighting us.");
            println!("           Lift pin 8 (VCC) or remove the chip from the board.");
            return Ok(());
        }
        Diagnosis::MisoFloatsHigh => {
            println!("Chip     : MISO floats high: no chip detected.");
            println!("           Check clip orientation, VCC jumper (3.3V), and chip power.");
            return Ok(());
        }
        Diagnosis::Known(c) => println!("Chip     : {}", c.name),
        Diagnosis::UnknownChip => {
            println!("Chip     : (unknown JEDEC; SFDP gives us the parameters anyway)")
        }
    }
    println!();
    // 256 bytes is enough to cover the header walk (8 + 8N for
    // typical N ≤ 5) plus the BFPT body for every published
    // JESD216 revision through F. Larger SFDP regions exist on
    // some chips (extra vendor tables, multi-KB security
    // descriptors) but the BFPT is always reachable inside the
    // first 256 bytes in practice.
    let data = spi::read_sfdp(&mut ch, 0, 256)?;
    print_sfdp(&data, &crate::sfdp::parse(&data));
    Ok(())
}

fn print_sfdp(raw: &[u8], parsed: &crate::sfdp::Sfdp) {
    use crate::sfdp::{Addressing, ParameterHeader};
    if !parsed.header.valid {
        println!("SFDP     : (chip didn't return the 'SFDP' magic; this chip");
        println!("           probably predates JESD216, or is the rare modern");
        println!("           part that omits SFDP. First 16 bytes:)");
        println!("  {}", hex::encode(&raw[..16.min(raw.len())]));
        return;
    }
    println!(
        "SFDP     : rev {}.{}, {} parameter header(s)",
        parsed.header.major_rev,
        parsed.header.minor_rev,
        parsed.parameter_headers.len(),
    );
    for (i, ph) in parsed.parameter_headers.iter().enumerate() {
        let tag = if ph.id == ParameterHeader::BFPT_ID {
            "JEDEC BFPT"
        } else {
            "vendor"
        };
        println!(
            "  [{i}] id=0x{:04X} ({tag}) rev {}.{} len={} dwords @ 0x{:06X}",
            ph.id, ph.major_rev, ph.minor_rev, ph.length_dwords, ph.ptr,
        );
    }
    let Some(bfpt) = &parsed.bfpt else {
        println!();
        println!("BFPT     : (not present or body outside the 256-byte read window)");
        return;
    };
    println!();
    println!("BFPT     :");
    println!(
        "  size      : {} bytes ({} KB, {} Mbit)",
        bfpt.size_bytes,
        bfpt.size_bytes / 1024,
        bfpt.size_bytes * 8 / 1_000_000,
    );
    println!("  page size : {} bytes", bfpt.page_size);
    let addr = match bfpt.addressing {
        Addressing::ThreeByteOnly => "3-byte only",
        Addressing::Either => "3- or 4-byte (default 3)",
        Addressing::FourByteOnly => "4-byte only",
        Addressing::Reserved => "reserved encoding",
    };
    println!("  address   : {addr}");
    if bfpt.erase_4k_opcode != 0xFF {
        println!("  4K erase  : opcode 0x{:02X}", bfpt.erase_4k_opcode);
    } else {
        println!("  4K erase  : not supported");
    }
    println!("  erase types:");
    let mut any = false;
    for (i, e) in bfpt.erase_types.iter().enumerate() {
        if e.size_bytes == 0 {
            continue;
        }
        any = true;
        println!(
            "    [{i}] 0x{:02X}  {} bytes ({})",
            e.opcode,
            e.size_bytes,
            human_size(e.size_bytes as u64),
        );
    }
    if !any {
        println!("    (none advertised)");
    }
}

fn human_size(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{} MB", n >> 20)
    } else if n >= 1 << 10 {
        format!("{} KB", n >> 10)
    } else {
        format!("{n} B")
    }
}

/// Pick a chip via `--chip <NAME>` if given, else by reading JEDEC
/// ID and falling back to SFDP when JEDEC isn't in the bundled DB.
/// The SFDP path lets etch341 read/write a brand-new chip that we
/// haven't catalogued yet, as long as the chip supports JESD216
/// (most parts made since ~2011). Hand-named overrides (`--chip`)
/// always win — that's the escape hatch for chips with damaged or
/// uncatalogued JEDEC IDs.
pub fn resolve_chip(spi: &mut dyn SpiTransport, global: &GlobalOpts) -> Result<Chip> {
    let db = ChipDb::load_embedded();
    if let Some(name) = &global.chip {
        return db
            .find_by_name(name)
            .cloned()
            .ok_or_else(|| Error::ChipNotRecognized(name.clone()));
    }
    let id = spi::jedec_read(spi)?;
    let jedec = format!("{:02X}{:02X}{:02X}", id[0], id[1], id[2]);
    if let Some(c) = db.find_by_jedec(&jedec) {
        return Ok(c.clone());
    }
    if let Some(chip) = synthesize_from_sfdp(spi, &jedec)? {
        return Ok(chip);
    }
    Err(Error::ChipNotRecognized(jedec))
}

/// Try to build a `Chip` from the chip's SFDP table when the JEDEC
/// ID isn't in the bundled DB. Returns `Ok(None)` for chips that
/// don't carry SFDP at all (pre-JESD216 silicon — all-`0xFF`
/// response, or a magic-mismatch) so the caller can surface a
/// "ChipNotRecognized" error instead of pretending we identified
/// it. Errors only propagate when the SPI transfer itself fails.
///
/// The synthesised `Chip` carries a name like "EF4018 (SFDP)" so a
/// quick glance at the header / log makes the source obvious.
/// `erase_time_ms` defaults to 60ms (the typical 4K-sector erase
/// time for the W25Q / MX25L families); future versions could
/// derive this from BFPT DWORD 10's erase timing fields.
pub fn synthesize_from_sfdp(spi: &mut dyn SpiTransport, jedec: &str) -> Result<Option<Chip>> {
    let data = spi::read_sfdp(spi, 0, 256)?;
    let parsed = crate::sfdp::parse(&data);
    if !parsed.header.valid {
        return Ok(None);
    }
    let Some(b) = parsed.bfpt else {
        return Ok(None);
    };
    // Pick the smallest advertised erase type as the sector size
    // — that's the one our sector-by-sector write path will use.
    // Falls back to the universal 4 KB when the chip doesn't
    // advertise erase types at all (rare).
    let sector_size = b
        .erase_types
        .iter()
        .filter(|e| e.size_bytes > 0)
        .map(|e| e.size_bytes)
        .min()
        .unwrap_or(4096);
    Ok(Some(Chip {
        name: format!("{jedec} (SFDP)"),
        jedec_id: jedec.to_string(),
        size_kb: (b.size_bytes / 1024) as u32,
        page_size: b.page_size,
        sector_size,
        erase_time_ms: 60,
        notes: "Auto-derived from SFDP; add an entry to chips.toml for a friendlier name + accurate erase timing.".to_string(),
    }))
}

pub fn read(
    spi: &mut dyn SpiTransport,
    chip: &Chip,
    start: u32,
    len: u32,
    output: &Path,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    let chip_size = chip.size_kb.saturating_mul(1024);
    if start.saturating_add(len) > chip_size {
        return Err(Error::AddressOutOfRange {
            addr: start,
            len,
            chip_size,
        });
    }
    // `-o -` (the standard UNIX-pipe idiom) writes the chip dump
    // to stdout instead of a file. Suppresses the success summary
    // lines so they don't interleave with binary data going to
    // `sha256sum` / `diff -` / etc. on the consumer side. Errors
    // still surface — they're returned from this function and
    // printed by the dispatcher to stderr.
    let stdout_mode = output == std::path::Path::new("-");
    let mut out: Box<dyn Write> = if stdout_mode {
        Box::new(std::io::stdout())
    } else {
        Box::new(File::create(output)?)
    };
    let mut hasher = Sha256::new();
    let addressing = addressing_for(chip);
    progress.start(len as u64);

    let mut addr = start;
    let end = start + len;
    while addr < end {
        let n = std::cmp::min(READ_CHUNK, end - addr);
        let data = spi::read_data(spi, addressing, addr, n as usize)?;
        out.write_all(&data)?;
        hasher.update(&data);
        addr += n;
        progress.update((addr - start) as u64);
    }
    progress.finish();
    out.flush()?;
    if !stdout_mode {
        // To stderr (`eprintln!`) so the dump file can be redirected
        // (`etch341 read -o foo.bin > log.txt`) without losing the
        // SHA confirmation. On a normal interactive run the
        // terminal sees both anyway.
        eprintln!("Read OK  : {} bytes → {}", len, output.display());
        eprintln!("SHA-256  : {}", hex::encode(hasher.finalize()));
    }
    Ok(())
}

pub fn erase_chip(
    spi: &mut dyn SpiTransport,
    chip: &Chip,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    println!("Erasing entire chip ({} KB)…", chip.size_kb);
    // No granular progress for whole-chip erase — just start/finish
    // bookends so observers (GUI header, indicatif spinner) know
    // an operation is in flight.
    progress.start(chip.size_kb.saturating_mul(1024) as u64);
    spi::chip_erase(spi)?;
    spi::wait_until_ready(spi, CHIP_ERASE_TIMEOUT)?;
    progress.finish();
    Ok(())
}

pub fn erase_range(
    spi: &mut dyn SpiTransport,
    chip: &Chip,
    start: u32,
    len: u32,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    let chip_size = chip.size_kb.saturating_mul(1024);
    if start.saturating_add(len) > chip_size {
        return Err(Error::AddressOutOfRange {
            addr: start,
            len,
            chip_size,
        });
    }
    if !start.is_multiple_of(chip.sector_size) {
        return Err(Error::UnalignedErase {
            addr: start,
            sector_size: chip.sector_size,
        });
    }

    // Round end up to sector boundary so we always cover the requested range.
    let aligned_len = len.div_ceil(chip.sector_size) * chip.sector_size;
    let end = start + aligned_len;
    let addressing = addressing_for(chip);
    progress.start(aligned_len as u64);

    let mut addr = start;
    while addr < end {
        // Prefer 64K block when both endpoints land on a 64K boundary.
        if addr.is_multiple_of(0x10000) && (end - addr) >= 0x10000 {
            spi::block_erase_64k(spi, addressing, addr)?;
            spi::wait_until_ready(spi, BLOCK_ERASE_TIMEOUT)?;
            addr += 0x10000;
        } else {
            spi::sector_erase_4k(spi, addressing, addr)?;
            spi::wait_until_ready(spi, SECTOR_ERASE_TIMEOUT)?;
            addr += chip.sector_size;
        }
        progress.update((addr - start) as u64);
    }
    progress.finish();
    println!("Erase OK : {} bytes (rounded from {})", aligned_len, len);
    Ok(())
}

pub fn write(
    spi: &mut dyn SpiTransport,
    chip: &Chip,
    data: &[u8],
    start: u32,
    erase: bool,
    verify_after: bool,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    let chip_size = chip.size_kb.saturating_mul(1024);
    if start.saturating_add(data.len() as u32) > chip_size {
        return Err(Error::AddressOutOfRange {
            addr: start,
            len: data.len() as u32,
            chip_size,
        });
    }

    if erase {
        erase_range(spi, chip, start, data.len() as u32, progress)?;
    }

    progress.start(data.len() as u64);
    let addressing = addressing_for(chip);
    let page = chip.page_size;
    let mut written: usize = 0;
    while written < data.len() {
        let addr = start + written as u32;
        let page_offset = addr % page;
        let chunk_len = std::cmp::min((page - page_offset) as usize, data.len() - written);
        spi::page_program(
            spi,
            addressing,
            addr,
            &data[written..written + chunk_len],
            page,
        )?;
        spi::wait_until_ready(spi, PAGE_PROGRAM_TIMEOUT)?;
        written += chunk_len;
        progress.update(written as u64);
    }
    progress.finish();
    println!("Write OK : {} bytes at 0x{:08X}", data.len(), start);

    if verify_after {
        let bad = verify(spi, chip, data, start, progress)?;
        if bad > 0 {
            return Err(Error::VerifyFailed {
                addr: 0,
                expected: 0,
                actual: 0,
            });
        }
    }
    Ok(())
}

/// Compare `expected` against the chip starting at `start`. Returns the
/// number of mismatched bytes (0 = clean). Prints the first mismatch's
/// address to stderr to aid debugging.
pub fn verify(
    spi: &mut dyn SpiTransport,
    chip: &Chip,
    expected: &[u8],
    start: u32,
    progress: &mut dyn ProgressSink,
) -> Result<usize> {
    let chip_size = chip.size_kb.saturating_mul(1024);
    if start.saturating_add(expected.len() as u32) > chip_size {
        return Err(Error::AddressOutOfRange {
            addr: start,
            len: expected.len() as u32,
            chip_size,
        });
    }
    let addressing = addressing_for(chip);
    progress.start(expected.len() as u64);
    let mut mismatches = 0usize;
    let mut first_bad: Option<(u32, u8, u8)> = None;
    let mut off: usize = 0;
    let end = expected.len();
    while off < end {
        let n = std::cmp::min(READ_CHUNK as usize, end - off);
        let actual = spi::read_data(spi, addressing, start + off as u32, n)?;
        for (i, (&a, &e)) in actual.iter().zip(&expected[off..off + n]).enumerate() {
            if a != e {
                mismatches += 1;
                first_bad.get_or_insert((start + (off + i) as u32, e, a));
            }
        }
        off += n;
        progress.update(off as u64);
    }
    progress.finish();

    if mismatches == 0 {
        println!("Verify OK: {} bytes match", expected.len());
    } else {
        let (addr, e, a) = first_bad.unwrap();
        eprintln!(
            "Verify FAIL: {} byte(s) differ; first at 0x{:08X} (expected 0x{:02X}, got 0x{:02X})",
            mismatches, addr, e, a
        );
    }
    Ok(mismatches)
}

pub fn blank_check(
    spi: &mut dyn SpiTransport,
    chip: &Chip,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    let len = chip.size_kb.saturating_mul(1024);
    let addressing = addressing_for(chip);
    progress.start(len as u64);
    let mut addr = 0u32;
    while addr < len {
        let n = std::cmp::min(READ_CHUNK, len - addr);
        let data = spi::read_data(spi, addressing, addr, n as usize)?;
        for (i, &b) in data.iter().enumerate() {
            if b != 0xFF {
                progress.finish();
                return Err(Error::NotBlank {
                    addr: addr + i as u32,
                    value: b,
                });
            }
        }
        addr += n;
        progress.update(addr as u64);
    }
    progress.finish();
    println!("Blank OK : all {} bytes are 0xFF", len);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spi::test_support::MockSpi;

    fn fake_chip(size_kb: u32) -> Chip {
        Chip {
            name: "FAKE".into(),
            jedec_id: "AABBCC".into(),
            size_kb,
            page_size: 256,
            sector_size: 4096,
            erase_time_ms: 50,
            notes: String::new(),
        }
    }

    fn read_step(addr: u32, n: usize, body: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        let mut tx = vec![0x03, (addr >> 16) as u8, (addr >> 8) as u8, addr as u8];
        tx.resize(4 + n, 0);
        let mut rx = vec![0u8; 4];
        rx.extend(body);
        (tx, rx)
    }

    fn wip_clear_step() -> (Vec<u8>, Vec<u8>) {
        (vec![0x05, 0x00], vec![0x00, 0x00])
    }

    #[test]
    fn read_writes_bytes_to_file_and_hashes() {
        let body: Vec<u8> = (0..16u8).collect();
        let mut mock = MockSpi::new([read_step(0, 16, body.clone())]);
        let chip = fake_chip(1);
        let tmp = std::env::temp_dir().join("etch341_read_test.bin");
        read(&mut mock, &chip, 0, 16, &tmp, &mut NullSink).unwrap();
        let got = std::fs::read(&tmp).unwrap();
        assert_eq!(got, body);
        mock.assert_drained();
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn read_rejects_out_of_range() {
        let chip = fake_chip(1);
        let mut mock = MockSpi::new([]);
        let r = read(
            &mut mock,
            &chip,
            0,
            2048,
            Path::new("/tmp/x.bin"),
            &mut NullSink,
        );
        assert!(matches!(r, Err(Error::AddressOutOfRange { .. })));
    }

    #[test]
    fn erase_chip_polls_until_wip_clear() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0xC7], vec![0x00]),
            wip_clear_step(),
        ]);
        erase_chip(&mut mock, &fake_chip(1), &mut NullSink).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn erase_range_uses_block_when_aligned() {
        // 64K starting at 0x10000 → one block erase, not 16 sector erases.
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0xD8, 0x01, 0x00, 0x00], vec![0; 4]),
            wip_clear_step(),
        ]);
        erase_range(&mut mock, &fake_chip(128), 0x10000, 0x10000, &mut NullSink).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn erase_range_falls_back_to_sectors_when_short() {
        // 8K at 0x0 → 2 sector erases (not a full block).
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0]),
            (vec![0x20, 0x00, 0x00, 0x00], vec![0; 4]),
            wip_clear_step(),
            (vec![0x06], vec![0]),
            (vec![0x20, 0x00, 0x10, 0x00], vec![0; 4]),
            wip_clear_step(),
        ]);
        erase_range(&mut mock, &fake_chip(128), 0, 0x2000, &mut NullSink).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn erase_range_rejects_unaligned_start() {
        let mut mock = MockSpi::new([]);
        let r = erase_range(&mut mock, &fake_chip(128), 0x100, 0x1000, &mut NullSink);
        assert!(matches!(r, Err(Error::UnalignedErase { .. })));
    }

    #[test]
    fn write_does_page_aligned_program() {
        // Two pages worth starting mid-page (offset 0x80 → first chunk 128 bytes,
        // second chunk 256 bytes, third chunk 128 bytes).
        let data: Vec<u8> = (0..512u32).map(|i| i as u8).collect();
        let mut steps: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        // First chunk: 128 bytes at 0x80
        let mut c1 = vec![0x02, 0x00, 0x00, 0x80];
        c1.extend_from_slice(&data[0..128]);
        steps.push((vec![0x06], vec![0]));
        steps.push((c1, vec![0; 132]));
        steps.push(wip_clear_step());
        // Second chunk: 256 bytes at 0x100
        let mut c2 = vec![0x02, 0x00, 0x01, 0x00];
        c2.extend_from_slice(&data[128..384]);
        steps.push((vec![0x06], vec![0]));
        steps.push((c2, vec![0; 260]));
        steps.push(wip_clear_step());
        // Third chunk: 128 bytes at 0x200
        let mut c3 = vec![0x02, 0x00, 0x02, 0x00];
        c3.extend_from_slice(&data[384..512]);
        steps.push((vec![0x06], vec![0]));
        steps.push((c3, vec![0; 132]));
        steps.push(wip_clear_step());

        let mut mock = MockSpi::new(steps);
        write(
            &mut mock,
            &fake_chip(128),
            &data,
            0x80,
            false,
            false,
            &mut NullSink,
        )
        .unwrap();
        mock.assert_drained();
    }

    #[test]
    fn verify_counts_mismatches() {
        let expected = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let actual = vec![0xAA, 0x00, 0xCC, 0x00];
        let mut mock = MockSpi::new([read_step(0, 4, actual)]);
        let count = verify(&mut mock, &fake_chip(1), &expected, 0, &mut NullSink).unwrap();
        assert_eq!(count, 2);
        mock.assert_drained();
    }

    #[test]
    fn blank_check_passes_on_all_ff() {
        let mut mock = MockSpi::new([read_step(0, 1024, vec![0xFF; 1024])]);
        blank_check(&mut mock, &fake_chip(1), &mut NullSink).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn blank_check_reports_first_non_ff() {
        let mut body = vec![0xFF; 1024];
        body[10] = 0x42;
        let mut mock = MockSpi::new([read_step(0, 1024, body)]);
        let r = blank_check(&mut mock, &fake_chip(1), &mut NullSink);
        assert!(matches!(
            r,
            Err(Error::NotBlank {
                addr: 10,
                value: 0x42
            })
        ));
    }
}
