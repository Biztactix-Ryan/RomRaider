//! Workspace architecture test: the real `cargo metadata` graph obeys the
//! ARCHITECTURE.md dependency rule, and each rule fails when the real graph is
//! made to violate it.

use std::collections::BTreeSet;
use std::path::Path;

use cargo_metadata::MetadataCommand;

use xtask::deps::{DepGraph, EdgeKind, RULES, Violation, check};

#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn workspace_graph() -> DepGraph {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    DepGraph::load(&manifest).unwrap_or_else(|e| panic!("cargo metadata on the workspace: {e:#}"))
}

#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn id(g: &DepGraph, name: &str) -> String {
    g.workspace_id(name)
        .unwrap_or_else(|| panic!("workspace crate {name} missing"))
        .to_owned()
}

/// Proves US-RR-1-2, US-RR-1-3 and US-RR-6-3 on the real graph: the committed
/// workspace satisfies every rule (CI runs this via `cargo nextest`, and the
/// same check via `cargo xtask check-deps` in the clippy job).
#[test]
fn real_workspace_obeys_dependency_rule() {
    let v = check(&workspace_graph());
    let msgs: Vec<String> = v.iter().map(ToString::to_string).collect();
    assert!(
        v.is_empty(),
        "dependency rule violations:\n{}",
        msgs.join("\n")
    );
}

/// Proves US-RR-1-2 (workspace half): an `rr-core -> <workspace crate>` edge
/// planted into the real `cargo metadata` graph is reported.
#[test]
fn real_workspace_with_core_to_defs_edge_fails() {
    let mut g = workspace_graph();
    let (core, defs) = (id(&g, "rr-core"), id(&g, "rr-defs"));
    g.add_edge(&core, &defs, EdgeKind::Normal);
    let v = check(&g);
    assert!(v.contains(&Violation::CoreDependsOnWorkspace {
        core: "rr-core".into(),
        dep: "rr-defs".into(),
        kind: EdgeKind::Normal,
    }));
}

/// Proves US-RR-1-3 (table-driven, real `cargo metadata` graph): for every
/// real workspace crate other than rr-link/rr-cli/rr-app, a planted edge to each
/// of serialport, libloading, nusb and winreg is reported; the three sanctioned
/// crates may take it.
#[test]
fn real_workspace_any_unsanctioned_crate_to_any_hardware_crate_fails() {
    let names: Vec<String> = workspace_graph()
        .workspace_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert!(names.len() >= 10, "unexpectedly small workspace: {names:?}");
    for from in &names {
        for &hw in RULES.hardware_crates {
            let mut g = workspace_graph();
            let from_id = id(&g, from);
            let synthetic = format!("synthetic-{hw}");
            g.add_package(&synthetic, hw, false);
            g.add_edge(&from_id, &synthetic, EdgeKind::Normal);
            let v = check(&g);
            let reported = v.contains(&Violation::HardwareReachable {
                from: from.clone(),
                hardware: hw.into(),
                path: vec![from.clone(), hw.into()],
            });
            let sanctioned = RULES.hardware_allowed.contains(&from.as_str());
            assert_eq!(
                reported, !sanctioned,
                "{from} -> {hw}: sanctioned={sanctioned}, violations={v:?}"
            );
        }
    }
}

/// Proves US-RR-1-3 (single real-graph example).
#[test]
fn real_workspace_with_rom_to_serialport_edge_fails() {
    let mut g = workspace_graph();
    let rom = id(&g, "rr-rom");
    g.add_package("synthetic-serialport", "serialport", false);
    g.add_edge(&rom, "synthetic-serialport", EdgeKind::Normal);
    let v = check(&g);
    assert!(v.contains(&Violation::HardwareReachable {
        from: "rr-rom".into(),
        hardware: "serialport".into(),
        path: vec!["rr-rom".into(), "serialport".into()],
    }));
}

/// Proves US-RR-1-2 (hardware half): `rr-core` reaching any hardware crate,
/// planted into the real `cargo metadata` graph, is reported.
#[test]
fn real_workspace_with_core_to_hardware_crate_fails() {
    for &hw in RULES.hardware_crates {
        let mut g = workspace_graph();
        let core = id(&g, "rr-core");
        let synthetic = format!("synthetic-{hw}");
        g.add_package(&synthetic, hw, false);
        g.add_edge(&core, &synthetic, EdgeKind::Normal);
        assert!(
            check(&g).contains(&Violation::HardwareReachable {
                from: "rr-core".into(),
                hardware: hw.into(),
                path: vec!["rr-core".into(), hw.into()],
            }),
            "rr-core -> {hw} not reported"
        );
    }
}

/// Proves US-RR-1-2 (OS/IO half): `rr-core` reaching any named OS/IO crate
/// ([`RULES`]`.core_forbidden_io`), planted into the real graph, is reported.
#[test]
fn real_workspace_with_core_to_io_crate_fails() {
    assert!(!RULES.core_forbidden_io.is_empty());
    for &io in RULES.core_forbidden_io {
        let mut g = workspace_graph();
        let core = id(&g, "rr-core");
        let synthetic = format!("synthetic-{io}");
        g.add_package(&synthetic, io, false);
        g.add_edge(&core, &synthetic, EdgeKind::Normal);
        assert!(
            check(&g).contains(&Violation::CoreReachesIo {
                core: "rr-core".into(),
                io: io.into(),
                path: vec!["rr-core".into(), io.into()],
            }),
            "rr-core -> {io} not reported"
        );
    }
}

/// Proves US-RR-6-3: a normal (non-dev) edge to `rr-testkit`, planted into the
/// real `cargo metadata` graph, is reported (while the real graph, where e.g.
/// rr-rom takes rr-testkit as a dev-dependency, passes).
#[test]
fn real_workspace_with_defs_normal_dep_on_testkit_fails() {
    let mut g = workspace_graph();
    let (defs, testkit) = (id(&g, "rr-defs"), id(&g, "rr-testkit"));
    g.add_edge(&defs, &testkit, EdgeKind::Normal);
    let v = check(&g);
    assert!(v.contains(&Violation::DevOnlyAsNonDev {
        from: "rr-defs".into(),
        dep: "rr-testkit".into(),
        kind: EdgeKind::Normal,
    }));
}

/// Proves US-RR-1-4 (naming half; the `--version` half is
/// `crates/rr-cli/tests/version.rs` and `crates/rr-app/tests/version.rs`): the
/// workspace's only binary targets are `romraider-rs` (rr-app),
/// `romraider-cli` (rr-cli) and `xtask`, so no binary is called `rr`.
#[test]
fn workspace_binaries_are_exactly_romraider_rs_romraider_cli_and_xtask() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let meta = MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()
        .expect("cargo metadata --no-deps on the workspace");
    let bins: BTreeSet<(String, String)> = meta
        .workspace_packages()
        .into_iter()
        .flat_map(|p| {
            p.targets
                .iter()
                .filter(|t| t.is_bin())
                .map(|t| (p.name.to_string(), t.name.clone()))
        })
        .collect();
    assert!(
        bins.iter().all(|(_, bin)| bin != "rr"),
        "a binary is named `rr`: {bins:?}"
    );
    let expected: BTreeSet<(String, String)> = [
        ("rr-app", "romraider-rs"),
        ("rr-cli", "romraider-cli"),
        ("xtask", "xtask"),
    ]
    .into_iter()
    .map(|(p, b)| (p.to_owned(), b.to_owned()))
    .collect();
    assert_eq!(bins, expected, "workspace binary targets (package, bin)");
}

/// Proves US-RR-6-3 (table-driven, real `cargo metadata` graph): every real
/// workspace crate taking rr-testkit as a normal or build dependency is
/// reported; the real graph itself (dev-dependency edges only) passes, see
/// `real_workspace_obeys_dependency_rule`.
#[test]
fn real_workspace_any_non_dev_edge_to_testkit_fails() {
    assert!(RULES.dev_only_crates.contains(&"rr-testkit"));
    let names: Vec<String> = workspace_graph()
        .workspace_names()
        .into_iter()
        .filter(|n| *n != "rr-testkit")
        .map(str::to_owned)
        .collect();
    for from in &names {
        for kind in [EdgeKind::Normal, EdgeKind::Build] {
            let mut g = workspace_graph();
            let (from_id, testkit) = (id(&g, from), id(&g, "rr-testkit"));
            g.add_edge(&from_id, &testkit, kind);
            let v = check(&g);
            assert!(
                v.contains(&Violation::DevOnlyAsNonDev {
                    from: from.clone(),
                    dep: "rr-testkit".into(),
                    kind,
                }),
                "{from} -> rr-testkit ({kind}) not reported: {v:?}"
            );
        }
    }
}
