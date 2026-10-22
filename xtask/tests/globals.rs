//! `check-globals` end to end: the real workspace is clean, and a planted
//! workspace tree with forbidden globals fails with file:line:item findings.
//!
//! Proves US-RR-3-3: a `static mut` or a mutable global singleton in any
//! workspace crate fails CI (`cargo xtask check-globals`, clippy job).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use xtask::globals::{self, Kind, Problem};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn real_workspace_has_no_unallowlisted_globals() {
    let root = workspace_root();
    let report = globals::run(&root, &root.join(globals::DEFAULT_ALLOWLIST)).unwrap();
    assert!(report.files > 0, "no sources scanned");
    let msgs: Vec<String> = report.problems.iter().map(ToString::to_string).collect();
    assert!(
        report.problems.is_empty(),
        "check-globals problems:\n{}",
        msgs.join("\n")
    );
}

/// Builds a throwaway workspace under the per-test tmpdir.
fn planted(name: &str, files: &[(&str, &str)], allowlist: &str) -> io::Result<PathBuf> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("check-globals")
        .join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
    for (rel, src) in files {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap_or(&root))?;
        fs::write(p, src)?;
    }
    fs::write(root.join("allow.toml"), allowlist)?;
    Ok(root)
}

/// US-RR-3-3: `static mut`, a `LazyLock<Mutex<_>>` singleton and a
/// `thread_local!` planted in workspace crates are each reported.
#[test]
fn planted_globals_fail_and_skipped_dirs_are_ignored() {
    let root = planted(
        "fail",
        &[
            (
                "crates/rr-a/src/lib.rs",
                "pub fn ok() {}\n\nstatic mut HITS: u32 = 0;\n",
            ),
            (
                "crates/rr-b/src/state.rs",
                "use std::sync::{LazyLock, Mutex};\nstatic REG: LazyLock<Mutex<Vec<u8>>> = LazyLock::new(Default::default);\n",
            ),
            (
                "xtask/src/main.rs",
                "thread_local! { static T: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }\nfn main() {}\n",
            ),
            // Never scanned: build output, legacy tree, fixtures.
            ("crates/rr-a/target/gen.rs", "static mut X: u8 = 0;\n"),
            (
                "crates/rr-a/tests/fixtures/bad.rs",
                "static mut X: u8 = 0;\n",
            ),
            ("legacy/src/Foo.rs", "static mut X: u8 = 0;\n"),
        ],
        "",
    )
    .unwrap();
    let report = globals::run(&root, &root.join("allow.toml")).unwrap();
    let got: Vec<(String, usize, String, Kind)> = report
        .problems
        .iter()
        .map(|p| match p {
            Problem::Violation(f) => (f.path.clone(), f.line, f.item.clone(), f.kind),
            other => panic!("unexpected {other}"),
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                "crates/rr-a/src/lib.rs".to_owned(),
                3,
                "HITS".to_owned(),
                Kind::StaticMut
            ),
            (
                "crates/rr-b/src/state.rs".to_owned(),
                2,
                "REG".to_owned(),
                Kind::MutableStatic
            ),
            (
                "xtask/src/main.rs".to_owned(),
                1,
                "T".to_owned(),
                Kind::ThreadLocal
            ),
        ]
    );
}

#[test]
fn planted_global_with_justified_allowlist_entry_passes() {
    let root = planted(
        "allowed",
        &[("crates/rr-a/src/lib.rs", "static mut HITS: u32 = 0;\n")],
        "[[allow]]\npath = \"crates/rr-a/src/lib.rs\"\nitem = \"HITS\"\njustification = \"planted sample\"\n",
    )
    .unwrap();
    let report = globals::run(&root, &root.join("allow.toml")).unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.findings.len(), 1);
}

/// MF-04 (US-RR-19-3): the encoding module may hold no `static` at all, so
/// a write-once global byte order (`OnceLock`) planted there fails the
/// check, as `cargo xtask check-globals` runs it, while the same item
/// elsewhere is only a read-only static and passes.
#[test]
fn write_once_global_in_the_encoding_module_fails() {
    const ONCE: &str = "static ENDIAN: std::sync::OnceLock<u8> = std::sync::OnceLock::new();\n";
    let root = planted(
        "stateless",
        &[
            ("crates/rr-core/src/encoding/codec.rs", ONCE),
            ("crates/rr-core/src/encoding.rs", "const OK: u8 = 0;\n"),
            ("crates/rr-core/src/clock.rs", ONCE),
        ],
        "",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-globals")
        .arg("--root")
        .arg(&root)
        .arg("--allowlist")
        .arg(root.join("allow.toml"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "check-globals passed: {stderr}");
    assert!(
        stderr.contains(
            "crates/rr-core/src/encoding/codec.rs:1: static in a stateless module `ENDIAN`"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("clock.rs"), "{stderr}");
    assert!(stderr.contains("1 problem(s)"), "{stderr}");
}

/// US-RR-34-5, the review leg of "resolved Definitions are immutable": a
/// lock, cell, atomic or once-cell field planted in a frozen resolved type
/// (`resolve.rs`, the ROM-ID index) or in a type one embeds (rr-defs'
/// `CellFormat`, rr-core's `Provenance`) fails `cargo xtask check-globals`,
/// each named by file, line and type. `Mutex`, `RwLock`, the atomics and
/// `OnceLock` are `Sync` and `RefUnwindSafe`, so resolve.rs's compile-time
/// assertion alone would let them through. The same field in a module that
/// may hold state (the clock) passes.
#[test]
fn interior_mutability_planted_in_a_resolved_definition_fails() {
    const DEFINITION: &str = "\
use std::cell::Cell;
use std::sync::Mutex;
pub struct Definition {
    tables: Vec<u8>,
    cache: Mutex<Vec<u8>>,
    hits: Cell<u32>,
}
";
    const INDEX: &str = "\
pub struct RomIdIndex {
    last: std::sync::atomic::AtomicUsize,
    built: std::sync::OnceLock<u8>,
}
";
    const FORMAT: &str = "pub struct CellFormat {\n    memo: std::sync::RwLock<u8>,\n}\n";
    const PROVENANCE: &str = "pub struct Provenance {\n    seen: core::cell::RefCell<u8>,\n}\n";
    let root = planted(
        "frozen-definition",
        &[
            ("crates/rr-defs/src/resolve.rs", DEFINITION),
            ("crates/rr-defs/src/resolve/index.rs", INDEX),
            ("crates/rr-defs/src/romraider/scaling.rs", FORMAT),
            ("crates/rr-core/src/diag.rs", PROVENANCE),
            ("crates/rr-core/src/clock.rs", DEFINITION),
        ],
        "",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-globals")
        .arg("--root")
        .arg(&root)
        .arg("--allowlist")
        .arg(root.join("allow.toml"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "check-globals passed: {stderr}");
    for want in [
        "crates/rr-defs/src/resolve.rs:1: lock or cell in a stateless module `Cell`",
        "crates/rr-defs/src/resolve.rs:2: lock or cell in a stateless module `Mutex`",
        "crates/rr-defs/src/resolve.rs:5: lock or cell in a stateless module `Mutex`",
        "crates/rr-defs/src/resolve.rs:6: lock or cell in a stateless module `Cell`",
        "crates/rr-defs/src/resolve/index.rs:2: lock or cell in a stateless module `atomic`",
        "crates/rr-defs/src/resolve/index.rs:2: lock or cell in a stateless module `AtomicUsize`",
        "crates/rr-defs/src/resolve/index.rs:3: lock or cell in a stateless module `OnceLock`",
        "crates/rr-defs/src/romraider/scaling.rs:2: lock or cell in a stateless module `RwLock`",
        "crates/rr-core/src/diag.rs:2: lock or cell in a stateless module `RefCell`",
    ] {
        assert!(stderr.contains(want), "missing {want:?} in:\n{stderr}");
    }
    assert!(!stderr.contains("clock.rs"), "{stderr}");
    assert!(stderr.contains("9 problem(s)"), "{stderr}");
}

/// US-RR-3-3, as CI runs it: the `cargo xtask check-globals` binary exits
/// non-zero on a planted `static mut` and names it, and exits zero once the
/// global is gone.
#[test]
fn check_globals_cli_fails_ci_on_planted_static_mut() {
    let bad = planted(
        "cli-bad",
        &[("crates/rr-a/src/lib.rs", "static mut HITS: u32 = 0;\n")],
        "",
    )
    .unwrap();
    let run = |root: &Path| {
        std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-globals")
            .arg("--root")
            .arg(root)
            .arg("--allowlist")
            .arg(root.join("allow.toml"))
            .output()
            .unwrap()
    };
    let out = run(&bad);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "check-globals passed: {stderr}");
    assert!(
        stderr.contains("crates/rr-a/src/lib.rs:1: static mut `HITS`"),
        "{stderr}"
    );

    let good = planted(
        "cli-good",
        &[("crates/rr-a/src/lib.rs", "const HITS: u32 = 0;\n")],
        "",
    )
    .unwrap();
    let out = run(&good);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
