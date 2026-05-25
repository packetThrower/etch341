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

    /// SPI clock speed in kHz. Supported: 400, 750, 1500, 3000, 6000, 12000, 24000.
    #[arg(short = 's', long, global = true, default_value_t = 1500)]
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

pub fn dispatch(
    global: GlobalOpts,
    cmd: Command,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::ch341::Ch341;
    use crate::ops;

    match cmd {
        Command::Detect => Ok(ops::detect(&global)?),

        Command::Read(args) => {
            let mut ch = Ch341::open(global.verbose)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let chip_bytes = chip.size_kb.saturating_mul(1024);
            let len = args.length.unwrap_or(chip_bytes.saturating_sub(args.start));
            ops::read(&mut ch, &chip, args.start, len, &args.output)?;
            Ok(())
        }

        Command::Write(args) => {
            let data = std::fs::read(&args.input)?;
            let mut ch = Ch341::open(global.verbose)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            ops::write(
                &mut ch,
                &chip,
                &data,
                args.start,
                !args.no_erase,
                !args.no_verify,
            )?;
            Ok(())
        }

        Command::Erase(args) => {
            let mut ch = Ch341::open(global.verbose)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            match args.range.as_deref() {
                None => ops::erase_chip(&mut ch, &chip)?,
                Some(s) => {
                    let (start, len) = parse_range(s)?;
                    ops::erase_range(&mut ch, &chip, start, len)?;
                }
            }
            Ok(())
        }

        Command::Verify(args) => {
            let data = std::fs::read(&args.input)?;
            let mut ch = Ch341::open(global.verbose)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            let mismatches = ops::verify(&mut ch, &chip, &data, args.start)?;
            if mismatches > 0 {
                return Err(format!("verify failed: {} byte(s) differ", mismatches).into());
            }
            Ok(())
        }

        Command::BlankCheck => {
            let mut ch = Ch341::open(global.verbose)?;
            let chip = ops::resolve_chip(&mut ch, &global)?;
            ops::blank_check(&mut ch, &chip)?;
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
