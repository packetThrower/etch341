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

    /// SPI clock speed in kHz. Supported: 20, 100, 400, 750.
    #[arg(short = 's', long, global = true, default_value_t = 750)]
    pub speed: u32,

    /// Print raw SPI transactions.
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Parse + validate input without touching the hardware.
    #[arg(short = 'n', long, global = true)]
    pub dry_run: bool,
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
    /// I²C EEPROM operations (24Cxx family).
    I2c {
        #[command(subcommand)]
        action: I2cAction,
    },
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
    /// Output file.
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

    match cmd {
        Command::Detect => Ok(ops::detect(&global)?),

        Command::Read(args) => {
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(global.speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let chip_bytes = chip.size_kb.saturating_mul(1024);
            let len = args.length.unwrap_or(chip_bytes.saturating_sub(args.start));
            let mut sink = IndicatifSink::new("read   ");
            ops::read(&mut ch, &chip, args.start, len, &args.output, &mut sink)?;
            Ok(())
        }

        Command::Write(args) => {
            let data = std::fs::read(&args.input)?;
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(global.speed)?;
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
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(global.speed)?;
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
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(global.speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mut sink = IndicatifSink::new("verify ");
            let mismatches = ops::verify(&mut ch, &chip, &data, args.start, &mut sink)?;
            if mismatches > 0 {
                return Err(format!("verify failed: {} byte(s) differ", mismatches).into());
            }
            Ok(())
        }

        Command::BlankCheck => {
            let mut ch = Ch341::open(global.verbose)?;
            ch.set_clock(global.speed)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mut sink = IndicatifSink::new("blank  ");
            ops::blank_check(&mut ch, &chip, &mut sink)?;
            Ok(())
        }

        Command::I2c { action } => dispatch_i2c(global, action),
    }
}

fn dispatch_i2c(global: GlobalOpts, action: I2cAction) -> Result<(), Box<dyn std::error::Error>> {
    use crate::ch341::Ch341;
    use crate::i2c_ops;

    // I²C ops need a chip selected up front (no JEDEC ID to query),
    // except for `scan` which is bus-only.
    let resolve_chip = |chip: &Option<String>| {
        chip.as_deref()
            .ok_or_else(|| "I²C ops require --chip <NAME> (no JEDEC autodetect)".to_string())
            .and_then(|name| i2c_ops::resolve_chip(name).map_err(|e| e.to_string()))
    };

    match action {
        I2cAction::Scan => {
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(global.speed)?;
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
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(global.speed)?;
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
            let chip = resolve_chip(&global.chip)?;
            let data = std::fs::read(&args.input)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(global.speed)?;
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
            let chip = resolve_chip(&global.chip)?;
            let data = std::fs::read(&args.input)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(global.speed)?;
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
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(global.speed)?;
            let mut sink = IndicatifSink::new("i2c-blk");
            i2c_ops::blank_check(&mut ch, &chip, args.straps as u8, &mut sink)?;
            println!("Blank check OK ({} bytes all 0xFF)", chip.size_bytes);
            Ok(())
        }

        I2cAction::Erase(args) => {
            let chip = resolve_chip(&global.chip)?;
            let mut ch = Ch341::open_i2c(global.verbose)?;
            ch.set_clock(global.speed)?;
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
