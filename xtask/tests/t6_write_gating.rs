//! T6 safety-invariant suite, write gating (SECURITY.md "Enforcement", D-013,
//! story US-RR-7). This file is the T6 skeleton: later stories add their
//! invariants here.
//!
//! Invariant: every write-performing function in `rr-rom` and `rr-comms`
//! carries `#[rr_write]` and takes a capability token (`RomWrite`,
//! `EcuMemoryWrite`, `EcuReset`); no public function there looks like an
//! unmarked write. Checked by `xtask::writes` (`cargo xtask check-writes`)
//! against the real tree, and proven to bite against planted trees.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use xtask::writes::{self, Problem};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn real_rr_rom_and_rr_comms_writes_are_token_gated() {
    let report = writes::run(&workspace_root()).unwrap();
    assert!(report.files > 0, "no sources scanned");
    let msgs: Vec<String> = report.problems.iter().map(ToString::to_string).collect();
    assert!(
        report.problems.is_empty(),
        "check-writes problems:\n{}",
        msgs.join("\n")
    );
    assert!(
        report.marked.iter().all(|m| m.token.is_some()),
        "{:?}",
        report.marked
    );
}

#[test]
fn scanned_crates_depend_on_the_marker_crate() {
    // The marker must be usable where the check looks for it.
    let root = workspace_root();
    for c in writes::SCAN_CRATES {
        let manifest = fs::read_to_string(root.join(c).join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("rr-write-marker"),
            "{c}/Cargo.toml does not depend on rr-write-marker"
        );
    }
}

/// Builds a throwaway workspace under the per-test tmpdir.
fn planted(name: &str, files: &[(&str, &str)]) -> io::Result<PathBuf> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("check-writes")
        .join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
    for c in writes::SCAN_CRATES {
        fs::create_dir_all(root.join(c).join("src"))?;
    }
    for (rel, src) in files {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap_or(&root))?;
        fs::write(p, src)?;
    }
    Ok(root)
}

#[test]
fn planted_ungated_writes_fail_with_file_line_name() {
    let root = planted(
        "fail",
        &[
            (
                "crates/rr-rom/src/save.rs",
                "use rr_write_marker::rr_write;\n\n#[rr_write]\npub fn save(image: &RomImage, path: &Path) {}\n",
            ),
            (
                "crates/rr-comms/src/engine/mod.rs",
                "pub struct Engine;\nimpl Engine {\n    pub fn write_memory(&mut self, link: &mut dyn Link, addr: u32) {}\n}\n",
            ),
            // Outside src/ and in a skipped dir: not scanned.
            ("crates/rr-rom/tests/it.rs", "pub fn save() {}\n"),
            ("crates/rr-rom/src/fixtures/x.rs", "pub fn save() {}\n"),
        ],
    )
    .unwrap();
    let report = writes::run(&root).unwrap();
    let msgs: Vec<String> = report.problems.iter().map(ToString::to_string).collect();
    assert_eq!(report.problems.len(), 2, "{msgs:#?}");
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("crates/rr-rom/src/save.rs:4: save:")),
        "{msgs:#?}"
    );
    assert!(
        msgs.iter().any(|m| m.starts_with(
            "crates/rr-comms/src/engine/mod.rs:3: Engine::write_memory: unmarked suspected write"
        )),
        "{msgs:#?}"
    );
    assert!(matches!(
        report.problems.iter().find(|p| p.name() == "save"),
        Some(Problem::MissingToken { .. })
    ));
}

#[test]
fn planted_gated_writes_pass_and_are_enumerated() {
    let root = planted(
        "pass",
        &[
            (
                "crates/rr-rom/src/lib.rs",
                "use rr_core::capability::RomWrite;\nuse rr_write_marker::rr_write;\n\n#[rr_write]\npub fn save(image: &RomImage, path: &Path, token: RomWrite) {}\n",
            ),
            (
                "crates/rr-comms/src/lib.rs",
                "pub struct Engine;\nimpl Engine {\n    #[rr_write]\n    pub fn write_memory(&mut self, link: &mut dyn Link, tok: &EcuMemoryWrite) {}\n    #[rr_write]\n    pub fn reset(&mut self, tok: EcuReset) {}\n    #[rr_not_write(\"reads only\")]\n    pub fn read_memory(&mut self, link: &mut dyn Link) {}\n    fn write_raw(&mut self) {}\n}\n",
            ),
        ],
    )
    .unwrap();
    let report = writes::run(&root).unwrap();
    assert_eq!(report.problems, []);
    let marked: Vec<(&str, Option<&str>)> = report
        .marked
        .iter()
        .map(|m| (m.name.as_str(), m.token.as_deref()))
        .collect();
    assert_eq!(
        marked,
        [
            ("Engine::write_memory", Some("EcuMemoryWrite")),
            ("Engine::reset", Some("EcuReset")),
            ("save", Some("RomWrite")),
        ]
    );
}

#[test]
fn missing_scanned_crate_fails_closed() {
    let root = planted("missing", &[]).unwrap();
    fs::remove_dir_all(root.join("crates/rr-comms")).unwrap();
    let err = writes::run(&root).unwrap_err();
    assert!(err.to_string().contains("rr-comms"), "{err}");
}
