//! RomRaider-RS desktop application.
//!
//! No UI yet: this stub only handles command-line arguments (`--version`,
//! `--help`).

use clap::Parser;

/// RomRaider-RS desktop application.
#[derive(Debug, Parser)]
#[command(name = "romraider-rs", version, about)]
struct Args {}

fn main() {
    let _args = Args::parse();
}
