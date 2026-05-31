//! etch341 — cross-platform CLI/GUI flash programmer for CH341A.
//!
//! No args launches the GUI; any subcommand runs in CLI mode.

// On Windows, mark release `gui`-feature builds as the "windows"
// subsystem so double-clicking etch341.exe from Explorer (or
// launching it via a shortcut) doesn't pop a black console window
// alongside the GUI. Without this attribute Rust's default is the
// "console" subsystem, which allocates a fresh console for every
// GUI launch on Windows — what the user reported as "a terminal
// shows up when the exe is ran".
//
// Conditions:
//   - `target_os = "windows"`     — Linux/macOS have no subsystem
//                                   distinction and never popped
//                                   a terminal in the first place.
//   - `not(debug_assertions)`     — debug builds keep the default
//                                   console so `cargo run` on
//                                   Windows still surfaces
//                                   stdout/stderr in the launching
//                                   shell.
//   - `feature = "gui"`           — CLI-only builds
//                                   (`--no-default-features`) stay
//                                   on the console subsystem; the
//                                   binary's whole job is shell
//                                   output, no GUI to suppress
//                                   chrome for.
//
// Trade-off: a `windows_subsystem = "windows"` binary launched
// from `cmd` / PowerShell is fire-and-forgotten by the shell, so
// subcommand output prints after the prompt has redrawn. The
// `attach_parent_console` call below grafts onto the parent
// console so stdout/stderr at least *reach* the shell. Landing
// them cleanly above the next prompt would need a separate
// console-subsystem sibling (`etch341-cli.exe`) the way
// PortFinder ships; we can add that later if Windows-CLI usage
// justifies a second binary.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

mod ch341;
mod chipdb;
mod cli;
mod diff;
mod error;
mod i2c;
mod i2c_ops;
mod inspect;
mod ops;
mod prefs;
mod programmer;
mod sfdp;
mod spi;

#[cfg(feature = "gui")]
mod gui;

use clap::Parser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Cli::parse();
    match args.command {
        Some(cmd) => {
            // CLI mode: graft onto the parent shell's console so
            // stdout/stderr reach the user. No-op on Linux/macOS
            // (no subsystem distinction) and on Windows debug
            // builds (already console-subsystem).
            attach_parent_console();
            cli::dispatch(args.global, cmd)
        }
        None => run_default(),
    }
}

/// On Windows release `gui` builds we're a `windows_subsystem`
/// binary — no console is attached when launched from Explorer,
/// and the CLI's `println!` output would land in a detached
/// stdout. Attaching the parent process's console (when one
/// exists) makes that output visible if the user happens to run
/// us from `cmd.exe` / PowerShell. No-op on Linux / macOS.
#[allow(clippy::missing_safety_doc)]
fn attach_parent_console() {
    #[cfg(target_os = "windows")]
    {
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
        unsafe extern "system" {
            fn AttachConsole(dwProcessId: u32) -> i32;
        }
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
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
