//! Headless RomRaider-RS command-line tool.

use clap::Parser;

/// Headless RomRaider-RS command-line tool.
#[derive(Debug, Parser)]
#[command(name = "romraider-cli", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
