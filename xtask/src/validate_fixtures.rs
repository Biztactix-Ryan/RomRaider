//! `cargo xtask validate-fixtures [--base <git-rev>] [PATH...]` (US-RR-9-6).
//!
//! The validator itself is `rr_testkit::kat::validate`, built on rr-testkit's
//! KAT fixture types. rr-testkit is dev-dependency only (`cargo xtask
//! check-deps`, rule 3), so xtask cannot link it; instead this subcommand runs
//! rr-testkit's `validate-fixtures` example with `cargo run` and passes its
//! exit status through: 0 when every fixture passes, 1 on any violation, 2 on
//! a usage or read error. The rules are documented in
//! `crates/rr-testkit/src/kat/validate.rs`. (An example, not a binary: the
//! workspace's only binaries are `romraider-rs`, `romraider-cli` and `xtask`.)

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The package holding the validator.
pub const PACKAGE: &str = "rr-testkit";

/// The validator's example target.
pub const EXAMPLE: &str = "validate-fixtures";

/// The `cargo run` invocation of the validator for the workspace at
/// `workspace_root`, with `--base <rev>` when `base` is given and the fixture
/// `paths` (none means the validator's defaults). The current directory is
/// inherited, so relative paths and the git base resolve where the caller
/// stands.
#[must_use]
pub fn command(workspace_root: &Path, base: Option<&str>, paths: &[PathBuf]) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut cmd = Command::new(cargo);
    cmd.arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(workspace_root.join("Cargo.toml"))
        .args(["--package", PACKAGE, "--example", EXAMPLE, "--"]);
    if let Some(rev) = base {
        cmd.arg("--base").arg(rev);
    }
    cmd.args(paths);
    cmd
}
