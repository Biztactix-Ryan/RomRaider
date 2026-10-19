//! `romraider-cli --version` exits successfully and prints the binary name and version.

use std::process::Command;

/// Proves US-RR-1-4 (`--version` half): the `romraider-cli` binary exists under that
/// name (`CARGO_BIN_EXE_romraider-cli` only resolves if it does) and prints
/// `romraider-cli <version>`. The "no `rr` binary" half is
/// `xtask/tests/architecture.rs::workspace_binaries_are_exactly_romraider_rs_romraider_cli_and_xtask`.
#[test]
fn version_flag_prints_binary_name_and_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_romraider-cli"))
        .arg("--version")
        .output()
        .expect("failed to run romraider-cli");

    assert!(output.status.success(), "exit status: {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).expect("stdout is not UTF-8");
    assert_eq!(
        stdout.trim(),
        format!("romraider-cli {}", env!("CARGO_PKG_VERSION")),
        "unexpected --version output"
    );
}
