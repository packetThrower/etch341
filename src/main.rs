//! etch341 — cross-platform CLI/GUI flash programmer for CH341A.
//!
//! No args launches the GUI; any subcommand runs in CLI mode.

mod ch341;
mod chipdb;
mod cli;
mod error;
mod i2c;
mod ops;
mod spi;

#[cfg(feature = "gui")]
mod gui;

use clap::Parser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Cli::parse();
    match args.command {
        Some(cmd) => cli::dispatch(args.global, cmd),
        None => run_default(),
    }
}

#[cfg(feature = "gui")]
fn run_default() -> Result<(), Box<dyn std::error::Error>> {
    gui::run()
}

#[cfg(not(feature = "gui"))]
fn run_default() -> Result<(), Box<dyn std::error::Error>> {
    use clap::CommandFactory;
    cli::Cli::command().print_help()?;
    println!();
    Ok(())
}
