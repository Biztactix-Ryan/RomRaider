//! Lint documentation coverage on the real files (US-RR-3-4).

use std::path::Path;

use xtask::lint_docs;

/// Proves US-RR-3-4: every lint in the root `Cargo.toml` `[workspace.lints]`
/// (not `allow`), every `clippy.toml` `disallowed-methods` path and every
/// `clippy.toml` switch is documented in CONTRIBUTING.md `## Lint
/// configuration`, at its configured level, with a non-empty reason. CI runs
/// this in the `test` job (`cargo nextest run --workspace`).
#[test]
fn every_configured_ban_is_documented_with_its_reason() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let problems = lint_docs::run(&root).unwrap();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The real configuration is what the check reads: the bans US-RR-3 names
/// are all present, so the coverage test above cannot pass vacuously.
#[test]
fn real_configuration_contains_the_story_bans() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let read = |n: &str| std::fs::read_to_string(root.join(n)).unwrap();
    let bans: Vec<String> = lint_docs::bans(&read("Cargo.toml"), &read("clippy.toml"))
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    for want in [
        "lint `clippy::unwrap_used` = deny",
        "lint `clippy::expect_used` = deny",
        "lint `clippy::panic` = deny",
        "lint `clippy::disallowed_methods` = deny",
        "disallowed method `std::thread::sleep`",
        "disallowed method `std::time::Instant::now`",
        "disallowed method `std::time::SystemTime::now`",
        "clippy.toml switch `allow-unwrap-in-tests`",
        "clippy.toml switch `allow-expect-in-tests`",
    ] {
        assert!(bans.iter().any(|b| b == want), "missing {want} in {bans:?}");
    }
}
