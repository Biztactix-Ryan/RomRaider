//! `validate-fixtures [--base <git-rev>] [PATH...]`: validates KAT fixtures
//! with [`rr_testkit::kat::validate::cli`] (US-RR-9-6). Run it as
//! `cargo xtask validate-fixtures`; the rules and the exit status (0 pass,
//! 1 violations, 2 usage or read error) are documented in
//! `crates/rr-testkit/src/kat/validate.rs`.
//!
//! An example target rather than a binary: the workspace's only binaries are
//! `romraider-rs`, `romraider-cli` and `xtask` (US-RR-1-4), and rr-testkit is
//! dev-dependency only, so xtask cannot link the validator itself.

use std::process::ExitCode;

use rr_testkit::kat::validate;

fn main() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("validate-fixtures: error: cannot read the current directory: {error}");
            return ExitCode::from(2);
        }
    };
    ExitCode::from(validate::cli(
        std::env::args_os().skip(1),
        &cwd,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ))
}
