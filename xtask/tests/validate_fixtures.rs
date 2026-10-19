//! `cargo xtask validate-fixtures` end to end (US-RR-9-6): the subcommand
//! runs rr-testkit's validator (its `validate-fixtures` example) and passes its exit status through, so
//! CI fails on any fixture violation. The validator's rules are tested in
//! `crates/rr-testkit/tests/kat_validate.rs`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::validate_fixtures::{self, EXAMPLE, PACKAGE};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn scratch(name: &str) -> io::Result<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("validate-fixtures")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn xtask(dir: &Path, args: &[&str]) -> io::Result<(i32, String)> {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("validate-fixtures")
        .args(args)
        .current_dir(dir)
        .output()?;
    Ok((
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    ))
}

#[test]
fn command_runs_the_rr_testkit_validator() {
    let root = workspace_root();
    let cmd = validate_fixtures::command(
        &root,
        Some("origin/master"),
        None,
        &[PathBuf::from("fixtures/kat")],
    );
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let manifest = root.join("Cargo.toml").to_string_lossy().into_owned();
    assert_eq!(
        args,
        [
            "run",
            "--quiet",
            "--manifest-path",
            manifest.as_str(),
            "--package",
            PACKAGE,
            "--example",
            EXAMPLE,
            "--",
            "--base",
            "origin/master",
            "fixtures/kat"
        ]
    );
    let bare = validate_fixtures::command(&root, None, None, &[]);
    assert_eq!(bare.get_args().last().unwrap(), "--");
    // An empty `before` (a new branch) still reaches the validator as one
    // argument.
    let push = validate_fixtures::command(&root, None, Some(""), &[]);
    assert_eq!(push.get_args().last().unwrap(), "--push-before=");
}

const HEAD: &str = r#"schema = 1
title = "u8"

[definition.encoding]
storage = "u8"
endian = "big"

[input]
hex = "7F"

[[decode]]
raw = 127
"#;

#[test]
fn exits_zero_on_the_clean_fixtures_and_non_zero_on_any_violation() {
    let (code, out) = xtask(&workspace_root(), &[]).unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("validate-fixtures: ok ("), "{out}");

    let dir = scratch("violations").unwrap();
    fs::write(dir.join("no_source.toml"), HEAD).unwrap();
    let (code, out) = xtask(&dir, &["no_source.toml"]).unwrap();
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("no_source.toml: [[decode]] #1 (offset 0): expected value has no `source`"),
        "{out}"
    );

    fs::write(
        dir.join("trivial.toml"),
        format!("{HEAD}source = {{ kind = \"hand-calc\", working = \"127\" }}\n"),
    )
    .unwrap();
    let (code, out) = xtask(&dir, &["trivial.toml"]).unwrap();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[hand-calc-working]"), "{out}");

    fs::write(
        dir.join("good.toml"),
        format!(
            "{HEAD}source = {{ kind = \"hand-calc\", working = \"0x7F = 7*16 + 15 = 127\" }}\n"
        ),
    )
    .unwrap();
    let (code, out) = xtask(&dir, &["good.toml"]).unwrap();
    assert_eq!(code, 0, "{out}");
}
