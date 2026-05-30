//! Command-line surface. See README / CLAUDE.md for the full spec.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "CH341A flash programmer (CLI/GUI)")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Args, Clone)]
pub struct GlobalOpts {
    /// Override chip auto-detection by name (e.g. W25Q128JV).
    #[arg(short = 'c', long, global = true)]
    pub chip: Option<String>,

    /// Clock speed in kHz. Defaults are mode-aware: SPI ops use
    /// 750 kHz when unset; I²C ops use 100 kHz (Standard mode).
    /// Supported values: 20, 100, 400, 750. I²C ops reject 750
    /// because every 24Cxx in our database is spec'd at 400 kHz
    /// max — over-clocking a 24C02 has been observed to lock up
    /// the chip mid-write past recovery.
    #[arg(short = 's', long, global = true)]
    pub speed: Option<u32>,

    /// Log every SPI or I²C transaction to stderr.
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Validate args and print a [dry-run] summary, never opening
    /// the CH341. Requires --chip for ops that would otherwise
    /// JEDEC-autodetect. Offline commands (chips/strings/search)
    /// ignore this flag.
    #[arg(short = 'n', long, global = true)]
    pub dry_run: bool,
}

/// Default SPI clock when `--speed` is unset. Maximum the CH341A
/// natively supports through the I2C_STREAM SET command.
pub const SPI_DEFAULT_SPEED_KHZ: u32 = 750;
/// Default I²C clock when `--speed` is unset. Standard-mode I²C —
/// universally supported by every 24Cxx revision.
pub const I2C_DEFAULT_SPEED_KHZ: u32 = 100;
/// Hard ceiling for I²C operations. Every chip in our DB is
/// 400 kHz max per its datasheet; 750 kHz over-clocks them and
/// has been observed to lock up parts mid-write.
pub const I2C_MAX_SPEED_KHZ: u32 = 400;

impl GlobalOpts {
    /// Resolve `--speed` for SPI operations. Falls back to
    /// [`SPI_DEFAULT_SPEED_KHZ`] when the user didn't pass `-s`.
    pub fn spi_speed(&self) -> u32 {
        self.speed.unwrap_or(SPI_DEFAULT_SPEED_KHZ)
    }

    /// Resolve `--speed` for I²C operations. Falls back to
    /// [`I2C_DEFAULT_SPEED_KHZ`] and rejects any explicit value
    /// above [`I2C_MAX_SPEED_KHZ`] with a message that tells the
    /// user what the spec'd ceiling is.
    pub fn i2c_speed(&self) -> Result<u32, String> {
        let speed = self.speed.unwrap_or(I2C_DEFAULT_SPEED_KHZ);
        if speed > I2C_MAX_SPEED_KHZ {
            return Err(format!(
                "I²C clock {speed} kHz exceeds the {I2C_MAX_SPEED_KHZ} kHz max for the 24Cxx \
                 family. Use -s 100 (default), -s 20, or -s 400."
            ));
        }
        Ok(speed)
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Read JEDEC ID and display chip info.
    Detect,
    /// Dump flash contents to file.
    Read(ReadArgs),
    /// Program flash from file.
    Write(WriteArgs),
    /// Erase chip or address range.
    Erase(EraseArgs),
    /// Compare file to chip without writing.
    Verify(VerifyArgs),
    /// Confirm chip is fully erased.
    BlankCheck,
    /// Dump SR1/SR2/SR3 with decoded bit names. Diagnoses "writes
    /// silently failing" (block-protect bits set) and "quad mode
    /// not enabled" (QE clear) without needing to memorise the
    /// register layout.
    Sr,
    /// Read + decode the chip's SFDP (Serial Flash Discoverable
    /// Parameters) table. Surfaces size / page / erase opcodes /
    /// addressing directly from the chip without a DB lookup, so
    /// you can read a brand-new chip that isn't in chips.toml yet.
    Sfdp,
    /// Read the chip's one-time-programmable security registers.
    /// W25Q / GD25Q convention (opcode 0x48): three 256-byte
    /// registers commonly holding serial numbers, MAC addresses,
    /// or vendor keys. Read-only for now.
    Otp {
        #[command(subcommand)]
        action: OtpAction,
    },
    /// I²C EEPROM operations (24Cxx family).
    I2c {
        #[command(subcommand)]
        action: I2cAction,
    },
    /// List supported chips (SPI + I²C) from the embedded chip DB.
    Chips(ChipsArgs),
    /// Extract printable ASCII strings from a binary file.
    Strings(StringsArgs),
    /// Find every offset where a byte pattern (hex or ASCII) occurs in a file.
    Search(SearchArgs),
}

#[derive(Args)]
pub struct ChipsArgs {
    /// Filter to only this bus family. Default: list both.
    #[arg(long, value_parser = ["spi", "i2c"])]
    pub bus: Option<String>,
    /// Case-insensitive substring filter on chip name (or JEDEC ID for SPI).
    #[arg(long)]
    pub find: Option<String>,
}

#[derive(Args)]
pub struct StringsArgs {
    /// Input binary file.
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Minimum run length to report. Common defaults: 4 (lots of noise) or 8 (just labels).
    #[arg(long, default_value_t = 4)]
    pub min_len: usize,
}

#[derive(Args)]
pub struct SearchArgs {
    /// Pattern to find. All-hex-digits-and-even-length → hex bytes
    /// (e.g. "55AA" or "55 AA"); anything else → ASCII (case-insensitive
    /// for letters, exact for everything else).
    pub pattern: String,
    /// Input binary file.
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Bytes of surrounding context to print with each hit. 0 disables.
    #[arg(long, default_value_t = 16)]
    pub context: usize,
}

#[derive(Subcommand)]
pub enum OtpAction {
    /// Dump the security registers (read-only).
    Read,
    /// Erase one security register back to 0xFF. Repeatable unless
    /// the register's lock bit is set (etch341 never sets it).
    /// Requires `--yes`.
    Erase(OtpEraseArgs),
    /// Program one security register from a file. Programs only
    /// clear bits, so erase the register first for a clean write;
    /// the write is read-back verified. Requires `--yes`.
    Write(OtpWriteArgs),
}

#[derive(Args)]
pub struct OtpEraseArgs {
    /// Which security register to erase (1, 2, or 3).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
    pub register: u8,
    /// Confirm the destructive op. Without this the command refuses
    /// to run.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args)]
pub struct OtpWriteArgs {
    /// Input binary file. Must fit within the register from `--start`
    /// (registers are 256 bytes).
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Which security register to program (1, 2, or 3).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
    pub register: u8,
    /// Byte offset within the register to start at (default 0).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
    /// Confirm the destructive op. Without this the command refuses
    /// to run.
    #[arg(long)]
    pub yes: bool,
}

#[derive(Subcommand)]
pub enum I2cAction {
    /// Probe each 7-bit address and list which ones ACK.
    Scan,
    /// Dump the EEPROM contents to a file. Requires `--chip <NAME>`.
    Read(I2cReadArgs),
    /// Write a binary file to the EEPROM. Requires `--chip <NAME>`.
    Write(I2cWriteArgs),
    /// Compare a file against the EEPROM. Requires `--chip <NAME>`.
    Verify(I2cVerifyArgs),
    /// Confirm every byte reads back as 0xFF.
    BlankCheck(I2cBlankArgs),
    /// Write 0xFF to every byte (EEPROMs have no true erase op).
    Erase(I2cBlankArgs),
}

#[derive(Args)]
pub struct I2cReadArgs {
    /// Output file.
    #[arg(short = 'o', long, default_value = "eeprom_dump.bin")]
    pub output: PathBuf,
    /// Start address (decimal or 0x-prefixed hex).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
    /// Number of bytes to read. Defaults to the whole chip.
    #[arg(long, value_parser = parse_addr)]
    pub length: Option<u32>,
    /// A0/A1/A2 pin straps (3-bit value, default 0).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub straps: u32,
}

#[derive(Args)]
pub struct I2cWriteArgs {
    /// Input binary file.
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Skip readback verify after write.
    #[arg(long)]
    pub no_verify: bool,
    /// Start address.
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
    /// A0/A1/A2 pin straps (3-bit value, default 0).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub straps: u32,
}

#[derive(Args)]
pub struct I2cVerifyArgs {
    /// Reference file to compare against the EEPROM.
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Start address.
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
    /// A0/A1/A2 pin straps (3-bit value, default 0).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub straps: u32,
}

#[derive(Args)]
pub struct I2cBlankArgs {
    /// A0/A1/A2 pin straps (3-bit value, default 0).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub straps: u32,
}

#[derive(Args)]
pub struct ReadArgs {
    /// Output file. Pass `-` to write the dump to stdout (useful for
    /// `etch341 read -o - | sha256sum` and similar pipe idioms);
    /// the "Read OK / SHA-256" summary lines are suppressed in that
    /// mode so they don't interleave with the binary data.
    #[arg(short = 'o', long, default_value = "flash_dump.bin")]
    pub output: PathBuf,
    /// Start address (decimal or 0x-prefixed hex).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
    /// Number of bytes to read. Defaults to the whole chip.
    #[arg(long, value_parser = parse_addr)]
    pub length: Option<u32>,
}

#[derive(Args)]
pub struct WriteArgs {
    /// Input binary file.
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Skip readback verify after write.
    #[arg(long)]
    pub no_verify: bool,
    /// Skip erase before write (dangerous; existing 1 bits cannot become 0).
    #[arg(long)]
    pub no_erase: bool,
    /// Start address (decimal or 0x-prefixed hex).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
}

#[derive(Args)]
pub struct EraseArgs {
    /// Erase a range instead of the full chip, formatted START:LEN
    /// (either side decimal or 0x-prefixed hex).
    #[arg(long)]
    pub range: Option<String>,
}

#[derive(Args)]
pub struct VerifyArgs {
    /// File to compare against the chip.
    #[arg(short = 'i', long)]
    pub input: PathBuf,
    /// Start address (decimal or 0x-prefixed hex).
    #[arg(long, value_parser = parse_addr, default_value = "0")]
    pub start: u32,
}

fn parse_addr(s: &str) -> Result<u32, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u32>().map_err(|e| e.to_string())
    }
}

/// CLI `ProgressSink` impl driving an `indicatif::ProgressBar`. Built
/// fresh per op so the label tag (read / write / erase …) matches
/// what's happening. Lives in cli.rs because the GUI uses its own
/// implementation against a shared atomic counter — ops itself stays
/// presentation-agnostic.
struct IndicatifSink {
    pb: indicatif::ProgressBar,
}

impl IndicatifSink {
    fn new(label: &'static str) -> Self {
        let pb = indicatif::ProgressBar::new(0);
        pb.set_style(
            indicatif::ProgressStyle::with_template(&format!(
                "{{spinner}} {label}{{bar:40}} {{bytes}}/{{total_bytes}} ({{eta}})"
            ))
            .expect("static template")
            .progress_chars("=> "),
        );
        Self { pb }
    }
}

impl crate::ops::ProgressSink for IndicatifSink {
    fn start(&mut self, total: u64) {
        self.pb.set_length(total);
        self.pb.set_position(0);
    }
    fn update(&mut self, current: u64) {
        self.pb.set_position(current);
    }
    fn finish(&mut self) {
        self.pb.finish_and_clear();
    }
}

pub fn dispatch(global: GlobalOpts, cmd: Command) -> Result<(), Box<dyn std::error::Error>> {
    use crate::ch341::Ch341;
    use crate::ops;

    let speed = global.spi_speed();
    match cmd {
        Command::Detect => {
            if global.dry_run {
                println!(
                    "[dry-run] would open CH341A and read JEDEC ID at {} kHz",
                    speed
                );
                return Ok(());
            }
            Ok(ops::detect(&global)?)
        }

        Command::Sr => {
            if global.dry_run {
                println!(
                    "[dry-run] would open CH341A at {} kHz and read SR1/SR2/SR3",
                    speed
                );
                return Ok(());
            }
            Ok(ops::status(&global)?)
        }

        Command::Sfdp => {
            if global.dry_run {
                println!(
                    "[dry-run] would open CH341A at {} kHz and read SFDP table",
                    speed
                );
                return Ok(());
            }
            Ok(ops::sfdp(&global)?)
        }

        Command::Otp { action } => match action {
            OtpAction::Read => {
                if global.dry_run {
                    println!(
                        "[dry-run] would open CH341A at {} kHz and read the security registers",
                        speed
                    );
                    return Ok(());
                }
                Ok(ops::otp(&global)?)
            }
            OtpAction::Erase(args) => {
                if global.dry_run {
                    println!(
                        "[dry-run] would erase security register {} back to 0xFF",
                        args.register
                    );
                    return Ok(());
                }
                if !args.yes {
                    return Err(format!(
                        "refusing to erase security register {} without --yes. This clears the \
                         register to 0xFF; re-run with --yes to confirm.",
                        args.register
                    )
                    .into());
                }
                let mut ch = Ch341::open(global.verbose)?;
                ch.set_clock(speed)?;
                ops::ensure_chip_present(&mut ch)?;
                ops::otp_erase(&mut ch, args.register)?;
                println!("OTP erase OK: register {} back to 0xFF", args.register);
                Ok(())
            }
            OtpAction::Write(args) => {
                // Read the file in both paths so dry-run still flags a
                // missing/unreadable file.
                let data = std::fs::read(&args.input)?;
                if global.dry_run {
                    println!(
                        "[dry-run] would program {} byte(s) from {} into security register {} \
                         at offset 0x{:02X}",
                        data.len(),
                        args.input.display(),
                        args.register,
                        args.start
                    );
                    return Ok(());
                }
                if !args.yes {
                    return Err(format!(
                        "refusing to program security register {} without --yes. Programming \
                         only clears bits (1->0); erase the register first for a clean write. \
                         Re-run with --yes to confirm.",
                        args.register
                    )
                    .into());
                }
                let mut ch = Ch341::open(global.verbose)?;
                ch.set_clock(speed)?;
                ops::ensure_chip_present(&mut ch)?;
                ops::otp_write(&mut ch, args.register, args.start as usize, &data)?;
                println!(
                    "OTP write OK: {} byte(s) into register {} at offset 0x{:02X} (read-back verified)",
                    data.len(),
                    args.register,
                    args.start
                );
                Ok(())
            }
        },

        Command::Read(args) => {
            if global.dry_run {
                let chip = resolve_chip_offline(&global)?;
                let chip_bytes = chip.size_kb.saturating_mul(1024);
                let len = args.length.unwrap_or(chip_bytes.saturating_sub(args.start));
                validate_spi_range(&chip, args.start, len)?;
                println!(
                    "[dry-run] would read {} bytes (0x{:08X}..0x{:08X}) from {} → {}",
                    len,
                    args.start,
                    args.start + len,
                    chip.name,
                    args.output.display()
                );
                return Ok(());
            }
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let chip_bytes = chip.size_kb.saturating_mul(1024);
            let len = args.length.unwrap_or(chip_bytes.saturating_sub(args.start));
            let mut sink = IndicatifSink::new("read   ");
            ops::read(&mut ch, &chip, args.start, len, &args.output, &mut sink)?;
            Ok(())
        }

        Command::Write(args) => {
            // Read the input file in either path so dry-run still
            // surfaces a missing/unreadable file as an error.
            let data = std::fs::read(&args.input)?;
            if global.dry_run {
                let chip = resolve_chip_offline(&global)?;
                validate_spi_range(&chip, args.start, data.len() as u32)?;
                println!(
                    "[dry-run] would {}write {} bytes from {} to {} at 0x{:08X}{}",
                    if args.no_erase { "" } else { "erase + " },
                    data.len(),
                    args.input.display(),
                    chip.name,
                    args.start,
                    if args.no_verify { "" } else { " + verify" },
                );
                return Ok(());
            }
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mut sink = IndicatifSink::new("write  ");
            ops::write(
                &mut ch,
                &chip,
                &data,
                args.start,
                !args.no_erase,
                !args.no_verify,
                &mut sink,
            )?;
            Ok(())
        }

        Command::Erase(args) => {
            if global.dry_run {
                let chip = resolve_chip_offline(&global)?;
                match args.range.as_deref() {
                    None => println!(
                        "[dry-run] would erase entire {} chip ({} KB)",
                        chip.name, chip.size_kb
                    ),
                    Some(s) => {
                        let (start, len) = parse_range(s)?;
                        validate_spi_range(&chip, start, len)?;
                        println!(
                            "[dry-run] would erase {} bytes (0x{:08X}..0x{:08X}) on {}",
                            len,
                            start,
                            start + len,
                            chip.name
                        );
                    }
                }
                return Ok(());
            }
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mut sink = IndicatifSink::new("erase  ");
            match args.range.as_deref() {
                None => ops::erase_chip(&mut ch, &chip, &mut sink)?,
                Some(s) => {
                    let (start, len) = parse_range(s)?;
                    ops::erase_range(&mut ch, &chip, start, len, &mut sink)?;
                }
            }
            Ok(())
        }

        Command::Verify(args) => {
            let data = std::fs::read(&args.input)?;
            if global.dry_run {
                let chip = resolve_chip_offline(&global)?;
                validate_spi_range(&chip, args.start, data.len() as u32)?;
                println!(
                    "[dry-run] would verify {} bytes from {} against {} at 0x{:08X}",
                    data.len(),
                    args.input.display(),
                    chip.name,
                    args.start
                );
                return Ok(());
            }
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mut sink = IndicatifSink::new("verify ");
            let mismatches = ops::verify(&mut ch, &chip, &data, args.start, &mut sink)?;
            if mismatches > 0 {
                return Err(format!("verify failed: {} byte(s) differ", mismatches).into());
            }
            Ok(())
        }

        Command::BlankCheck => {
            if global.dry_run {
                let chip = resolve_chip_offline(&global)?;
                println!(
                    "[dry-run] would blank-check {} ({} KB)",
                    chip.name, chip.size_kb
                );
                return Ok(());
            }
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mut sink = IndicatifSink::new("blank  ");
            ops::blank_check(&mut ch, &chip, &mut sink)?;
            Ok(())
        }

        Command::I2c { action } => dispatch_i2c(global, action),

        Command::Chips(args) => {
            print_chips(&args);
            Ok(())
        }

        Command::Strings(args) => {
            let bytes = std::fs::read(&args.input)?;
            for (offset, s) in crate::inspect::extract_strings(&bytes, args.min_len) {
                println!("{:08X}  {}", offset, s);
            }
            Ok(())
        }

        Command::Search(args) => {
            let bytes = std::fs::read(&args.input)?;
            let needle = crate::inspect::parse_hex_needle(&args.pattern);
            if needle.is_empty() {
                return Err("empty search pattern".into());
            }
            let hits = crate::inspect::find_pattern(&bytes, &needle);
            if hits.is_empty() {
                eprintln!(
                    "No matches for {:?} in {}.",
                    args.pattern,
                    args.input.display()
                );
                return Ok(());
            }
            println!(
                "{} match{} for {:?} ({} byte{}):",
                hits.len(),
                if hits.len() == 1 { "" } else { "es" },
                args.pattern,
                needle.len(),
                if needle.len() == 1 { "" } else { "s" }
            );
            for offset in hits {
                print_search_hit(&bytes, offset, needle.len(), args.context);
            }
            Ok(())
        }
    }
}

/// Print a chip-DB listing, optionally filtered by bus / substring.
fn print_chips(args: &ChipsArgs) {
    use crate::chipdb::{ChipDb, I2cChipDb};

    let needle = args.find.as_deref().map(|s| s.to_ascii_lowercase());
    let want_spi = args.bus.as_deref().is_none_or(|b| b == "spi");
    let want_i2c = args.bus.as_deref().is_none_or(|b| b == "i2c");

    if want_spi {
        let db = ChipDb::load_embedded();
        let rows: Vec<_> = db
            .iter()
            .filter(|c| match needle.as_ref() {
                None => true,
                Some(n) => {
                    c.name.to_ascii_lowercase().contains(n)
                        || c.jedec_id.to_ascii_lowercase().contains(n)
                }
            })
            .collect();
        println!("SPI flash ({} chip{}):", rows.len(), plural(rows.len()));
        if !rows.is_empty() {
            println!(
                "  {:<14} {:<8} {:<10} {:<6} {:<7} {:<7}  NOTES",
                "NAME", "JEDEC", "SIZE", "VOLT", "PAGE", "SECTOR"
            );
            for c in rows {
                println!(
                    "  {:<14} {:<8} {:<10} {:<6} {:<7} {:<7}  {}",
                    c.name,
                    c.jedec_id,
                    fmt_bytes(c.size_kb as u64 * 1024),
                    c.voltage(),
                    fmt_bytes(c.page_size as u64),
                    fmt_bytes(c.sector_size as u64),
                    c.notes,
                );
            }
        }
    }

    if want_spi && want_i2c {
        println!();
    }

    if want_i2c {
        let db = I2cChipDb::load_embedded();
        let rows: Vec<_> = db
            .iter()
            .filter(|c| match needle.as_ref() {
                None => true,
                Some(n) => c.name.to_ascii_lowercase().contains(n),
            })
            .collect();
        println!("I²C EEPROMs ({} chip{}):", rows.len(), plural(rows.len()));
        if !rows.is_empty() {
            println!(
                "  {:<10} {:<10} {:<9} {:<6} {:<6}",
                "NAME", "SIZE", "VOLT", "PAGE", "ADDR"
            );
            for c in rows {
                println!(
                    "  {:<10} {:<10} {:<9} {:<6} {:<6}",
                    c.name,
                    fmt_bytes(c.size_bytes as u64),
                    c.voltage(),
                    fmt_bytes(c.page_size as u64),
                    format!("{} B", c.addr_width),
                );
            }
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Resolve a SPI chip from the embedded DB by name without touching
/// hardware. Used by --dry-run paths where there's no CH341 open and
/// JEDEC autodetect isn't an option. Errors if --chip wasn't given
/// or names a chip that's not in chips.toml.
fn resolve_chip_offline(global: &GlobalOpts) -> Result<crate::chipdb::Chip, String> {
    let name = global.chip.as_deref().ok_or_else(|| {
        "--dry-run requires --chip <NAME> (no hardware to autodetect)".to_string()
    })?;
    crate::chipdb::ChipDb::load_embedded()
        .find_by_name(name)
        .cloned()
        .ok_or_else(|| format!("--chip {name}: not in chip DB (try `etch341 chips --find {name}`)"))
}

/// Same idea for I²C: name lookup only, no hardware. I²C ops always
/// require --chip anyway because there's no JEDEC equivalent.
fn resolve_i2c_chip_offline(global: &GlobalOpts) -> Result<crate::chipdb::I2cChip, String> {
    let name = global
        .chip
        .as_deref()
        .ok_or_else(|| "I²C ops require --chip <NAME>".to_string())?;
    crate::chipdb::I2cChipDb::load_embedded()
        .find_by_name(name)
        .cloned()
        .ok_or_else(|| format!("--chip {name}: not in i2c_chips.toml"))
}

/// Validate `start + len` fits in a SPI chip without overflowing.
fn validate_spi_range(chip: &crate::chipdb::Chip, start: u32, len: u32) -> Result<(), String> {
    let chip_bytes = chip.size_kb.saturating_mul(1024);
    if start.saturating_add(len) > chip_bytes {
        return Err(format!(
            "address range out of bounds: 0x{:08X}..0x{:08X} on a {} KB chip ({})",
            start,
            start.saturating_add(len),
            chip.size_kb,
            chip.name
        ));
    }
    Ok(())
}

/// Same for an I²C EEPROM.
fn validate_i2c_range(chip: &crate::chipdb::I2cChip, start: u32, len: u32) -> Result<(), String> {
    if start.saturating_add(len) > chip.size_bytes {
        return Err(format!(
            "address range out of bounds: 0x{:08X}..0x{:08X} on a {} B chip ({})",
            start,
            start.saturating_add(len),
            chip.size_bytes,
            chip.name
        ));
    }
    Ok(())
}

/// Compact human-readable byte count: B / KB / MB / GB.
fn fmt_bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{} GB", n >> 30)
    } else if n >= 1 << 20 {
        format!("{} MB", n >> 20)
    } else if n >= 1 << 10 {
        format!("{} KB", n >> 10)
    } else {
        format!("{} B", n)
    }
}

/// Print one search hit in `xxd`-style: offset, hex bytes, ASCII gutter.
/// `context` bytes of surrounding data are included on either side so the
/// hit reads in context. Bytes inside the match are uppercased; the rest
/// stay lowercase so a quick visual scan separates needle from haystack.
fn print_search_hit(bytes: &[u8], offset: usize, len: usize, context: usize) {
    let start = offset.saturating_sub(context);
    let end = (offset + len + context).min(bytes.len());
    let slice = &bytes[start..end];

    let mut hex_part = String::with_capacity(slice.len() * 3);
    let mut asc_part = String::with_capacity(slice.len());
    for (i, &b) in slice.iter().enumerate() {
        let abs = start + i;
        let in_match = abs >= offset && abs < offset + len;
        if in_match {
            hex_part.push_str(&format!("{:02X} ", b));
        } else {
            hex_part.push_str(&format!("{:02x} ", b));
        }
        asc_part.push(if (0x20..=0x7E).contains(&b) {
            b as char
        } else {
            '.'
        });
    }
    println!(
        "  0x{:08X}  {}  |{}|",
        offset,
        hex_part.trim_end(),
        asc_part
    );
}

fn dispatch_i2c(global: GlobalOpts, action: I2cAction) -> Result<(), Box<dyn std::error::Error>> {
    use crate::ch341::Ch341;
    use crate::i2c_ops;

    // Validate up front so a 750 kHz `-s` never reaches the chip on
    // an I²C op (it locked up an M24C02-R during 2026-05 bring-up).
    let speed = global.i2c_speed()?;

    // I²C ops need a chip selected up front (no JEDEC ID to query),
    // except for `scan` which is bus-only.
    let resolve_chip = |chip: &Option<String>| {
        chip.as_deref()
            .ok_or_else(|| "I²C ops require --chip <NAME> (no JEDEC autodetect)".to_string())
            .and_then(|name| i2c_ops::resolve_chip(name).map_err(|e| e.to_string()))
    };

    match action {
        I2cAction::Scan => {
            if global.dry_run {
                println!(
                    "[dry-run] would open CH341A in I²C mode and probe 0x08..0x77 at {} kHz",
                    speed
                );
                return Ok(());
            }
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(speed)?;
            let hits = i2c_ops::scan(&mut ch)?;
            if hits.is_empty() {
                println!("No I²C devices responded on 0x08..0x77.");
            } else {
                println!("I²C devices ACKing:");
                for a in hits {
                    println!("  0x{:02X}", a);
                }
            }
            Ok(())
        }

        I2cAction::Read(args) => {
            if global.dry_run {
                let chip = resolve_i2c_chip_offline(&global)?;
                let len = args
                    .length
                    .unwrap_or(chip.size_bytes.saturating_sub(args.start));
                validate_i2c_range(&chip, args.start, len)?;
                println!(
                    "[dry-run] would i2c-read {} bytes (0x{:08X}..0x{:08X}) from {} → {}",
                    len,
                    args.start,
                    args.start + len,
                    chip.name,
                    args.output.display()
                );
                return Ok(());
            }
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(speed)?;
            let len = args
                .length
                .unwrap_or(chip.size_bytes.saturating_sub(args.start));
            let mut sink = IndicatifSink::new("i2c-rd ");
            i2c_ops::read(
                &mut ch,
                &chip,
                args.start,
                len,
                args.straps as u8,
                &args.output,
                &mut sink,
            )?;
            Ok(())
        }

        I2cAction::Write(args) => {
            let data = std::fs::read(&args.input)?;
            if global.dry_run {
                let chip = resolve_i2c_chip_offline(&global)?;
                validate_i2c_range(&chip, args.start, data.len() as u32)?;
                println!(
                    "[dry-run] would i2c-write {} bytes from {} to {} at 0x{:08X}{}",
                    data.len(),
                    args.input.display(),
                    chip.name,
                    args.start,
                    if args.no_verify { "" } else { " + verify" },
                );
                return Ok(());
            }
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(speed)?;
            let mut sink = IndicatifSink::new("i2c-wr ");
            i2c_ops::write(
                &mut ch,
                &chip,
                args.start,
                &data,
                args.straps as u8,
                &mut sink,
            )?;
            if !args.no_verify {
                let mut vs = IndicatifSink::new("i2c-vfy");
                let mismatches = i2c_ops::verify(
                    &mut ch,
                    &chip,
                    &data,
                    args.start,
                    args.straps as u8,
                    &mut vs,
                )?;
                if mismatches > 0 {
                    return Err(format!("verify failed: {} byte(s) differ", mismatches).into());
                }
                println!("Verify OK");
            }
            Ok(())
        }

        I2cAction::Verify(args) => {
            let data = std::fs::read(&args.input)?;
            if global.dry_run {
                let chip = resolve_i2c_chip_offline(&global)?;
                validate_i2c_range(&chip, args.start, data.len() as u32)?;
                println!(
                    "[dry-run] would i2c-verify {} bytes from {} against {} at 0x{:08X}",
                    data.len(),
                    args.input.display(),
                    chip.name,
                    args.start
                );
                return Ok(());
            }
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(speed)?;
            let mut sink = IndicatifSink::new("i2c-vfy");
            let mismatches = i2c_ops::verify(
                &mut ch,
                &chip,
                &data,
                args.start,
                args.straps as u8,
                &mut sink,
            )?;
            if mismatches > 0 {
                return Err(format!("verify failed: {} byte(s) differ", mismatches).into());
            }
            println!("Verify OK");
            Ok(())
        }

        I2cAction::BlankCheck(args) => {
            if global.dry_run {
                let chip = resolve_i2c_chip_offline(&global)?;
                println!(
                    "[dry-run] would i2c-blank-check {} ({} bytes)",
                    chip.name, chip.size_bytes
                );
                return Ok(());
            }
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(speed)?;
            let mut sink = IndicatifSink::new("i2c-blk");
            i2c_ops::blank_check(&mut ch, &chip, args.straps as u8, &mut sink)?;
            println!("Blank check OK ({} bytes all 0xFF)", chip.size_bytes);
            Ok(())
        }

        I2cAction::Erase(args) => {
            if global.dry_run {
                let chip = resolve_i2c_chip_offline(&global)?;
                println!(
                    "[dry-run] would write 0xFF over all {} bytes of {}",
                    chip.size_bytes, chip.name
                );
                return Ok(());
            }
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(speed)?;
            let mut sink = IndicatifSink::new("i2c-er ");
            i2c_ops::erase(&mut ch, &chip, args.straps as u8, &mut sink)?;
            println!("Erase OK ({} bytes set to 0xFF)", chip.size_bytes);
            Ok(())
        }
    }
}

fn parse_range(s: &str) -> std::result::Result<(u32, u32), String> {
    let (start, len) = s
        .split_once(':')
        .ok_or_else(|| format!("--range must be START:LEN, got {:?}", s))?;
    Ok((parse_addr(start)?, parse_addr(len)?))
}
