//! US-RR-10-8 / US-RR-10-1: the dependency-rule test fails if the reference
//! codec module (`rr-testkit::reference`) uses anything from rr-core encoding,
//! expr or conversion — in fact anything from rr-core at all (rules in
//! `xtask/src/reference_isolation.rs`, `cargo xtask check-reference-isolation`).
//!
//! * The real tree passes with the committed [`RULE`].
//! * Planted reference modules fail for every supported path form (direct,
//!   inline, grouped, aliased, glob, `::`-rooted, `extern crate`, macro body,
//!   another workspace crate) and for every indirect form (a re-export in
//!   another rr-testkit module reached via `crate::` / `super::` / a glob /
//!   transitively, `#[path]`, `include!`, `extern crate self`, an unknown
//!   crate-level macro, `#[cfg_attr(.., path = ..)]`) and for every
//!   Cargo-level form (rr-core under a renamed dependency key, directly, as a
//!   dev / target dependency or through `[workspace.dependencies]`; a local
//!   path or git crate that could re-export it). Registry dependencies stay
//!   usable.
//! * A clean module passes, including one that uses the real `kat` types.
//! * The rule cannot silently cover nothing: once the module exists `required`
//!   must be set, and then a missing (renamed) module fails.
//! * `ci.yml`'s `clippy` job runs the CLI as an unskippable gate (this test
//!   file itself runs in every `test` job via nextest).

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use workflow::{
    Workflow, assert_flags, ci_yml, gate_needs_problems, skippable_problems, trigger_problems,
};
use xtask::reference_isolation::{self, Finding, Kind, Problem, RULE, Rule};

const JOB: &str = "clippy";
const CMD: &str = "cargo xtask check-reference-isolation";
const SRC: &str = "crates/rr-testkit/src";

/// The committed rule with `required` set (as committed since US-RR-10-5).
const REQUIRED: Rule = Rule {
    required: true,
    ..RULE
};

/// The rule as it was before US-RR-10-5 added the module.
const NOT_REQUIRED: Rule = Rule {
    required: false,
    ..RULE
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Builds a throwaway workspace with rr-core, rr-defs and rr-testkit manifests
/// and the given files (paths relative to `crates/rr-testkit/src`).
fn planted(name: &str, files: &[(&str, &str)]) -> io::Result<PathBuf> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("check-reference-isolation")
        .join(name);
    let _ = fs::remove_dir_all(&root);
    for krate in ["rr-core", "rr-defs", "rr-testkit"] {
        let dir = root.join("crates").join(krate);
        fs::create_dir_all(&dir)?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{krate}\"\nversion = \"0.0.0\"\n"),
        )?;
    }
    let src = root.join(SRC);
    fs::create_dir_all(&src)?;
    if !files.iter().any(|(p, _)| *p == "lib.rs") {
        fs::write(src.join("lib.rs"), "pub mod reference;\n")?;
    }
    for (rel, text) in files {
        let p = src.join(rel);
        fs::create_dir_all(p.parent().unwrap_or(&src))?;
        fs::write(p, text)?;
    }
    Ok(root)
}

#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn violations(root: &Path, rule: &Rule) -> Vec<Finding> {
    reference_isolation::run(root, rule)
        .unwrap()
        .problems
        .into_iter()
        .map(|p| match p {
            Problem::Violation(f) => f,
            other => panic!("unexpected problem: {other}"),
        })
        .collect()
}

fn rr_core() -> Kind {
    Kind::WorkspaceCrate("rr_core".into())
}

#[test]
fn real_tree_passes_the_committed_rule() {
    let report = reference_isolation::run(&workspace_root(), &RULE).unwrap();
    let msgs: Vec<String> = report.problems.iter().map(ToString::to_string).collect();
    assert!(
        report.problems.is_empty(),
        "check-reference-isolation problems:\n{}",
        msgs.join("\n")
    );
    // The module exists (US-RR-10-5) and `required` is set, so the check is
    // not vacuous: it covered the reference module's files.
    assert!(report.module_present, "{:?}", report.files);
    assert!(
        report
            .files
            .iter()
            .any(|f| f.starts_with(&format!("{SRC}/reference"))),
        "{:?}",
        report.files
    );
}

/// Every direct path form naming rr-core (encoding, expr, conversion or
/// otherwise), and another workspace crate, fails at the right line.
#[test]
fn planted_direct_rr_core_uses_fail() {
    let cases: &[(&str, &str, usize, Kind)] = &[
        ("use", "use rr_core::encoding::Storage;\n", 1, rr_core()),
        (
            "inline",
            "pub fn f() -> u8 {\n    rr_core::encoding::decode_u8(&[0])\n}\n",
            2,
            rr_core(),
        ),
        ("grouped", "use rr_core::{encoding, expr};\n", 1, rr_core()),
        (
            "nested-group",
            "use {std::fmt, rr_core::conversion::Conv};\n",
            1,
            rr_core(),
        ),
        (
            "alias",
            "use rr_core as core2;\npub fn f() { core2::encoding::x(); }\n",
            1,
            rr_core(),
        ),
        ("glob", "use rr_core::*;\n", 1, rr_core()),
        ("rooted", "use ::rr_core::expr::Expr;\n", 1, rr_core()),
        ("raw-ident", "use r#rr_core::expr::Expr;\n", 1, rr_core()),
        ("extern", "extern crate rr_core as c;\n", 1, rr_core()),
        (
            "macro-body",
            "macro_rules! m {\n    () => { rr_core::expr::eval() };\n}\n",
            2,
            rr_core(),
        ),
        (
            "type-position",
            "pub struct S(pub rr_core::encoding::Storage);\n",
            1,
            rr_core(),
        ),
        (
            "other-workspace-crate",
            "use rr_defs::table::Table;\n",
            1,
            Kind::WorkspaceCrate("rr_defs".into()),
        ),
    ];
    for (name, src, line, kind) in cases {
        let root = planted(name, &[("reference.rs", src)]).unwrap();
        let got = violations(&root, &REQUIRED);
        assert_eq!(
            got,
            [Finding {
                path: format!("{SRC}/reference.rs"),
                line: *line,
                kind: kind.clone(),
                via: None,
            }],
            "case {name}"
        );
    }
}

/// `(name, files, file reported, line, via)`.
type ReexportCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str, usize, &'a str);

const SHIM: &str = "pub use rr_core::encoding as enc;\n";
const LIB_WITH_SHIM: &str = "pub mod reference;\npub mod shim;\n";

/// Indirect forms: a re-export of rr_core in another rr-testkit module that
/// the reference module reaches is reported in that module, naming the use.
#[test]
fn planted_reexport_reached_from_reference_fails() {
    let cases: &[ReexportCase<'_>] = &[
        (
            "crate-path",
            &[("reference.rs", "use crate::shim::enc;\n")],
            "shim.rs",
            1,
            "reference.rs:1",
        ),
        (
            "crate-group",
            &[("reference.rs", "use crate::{reference as _, shim};\n")],
            "shim.rs",
            1,
            "reference.rs:1",
        ),
        (
            "inline-crate-path",
            &[(
                "reference.rs",
                "\npub fn f() -> u8 { crate::shim::enc::decode_u8() }\n",
            )],
            "shim.rs",
            1,
            "reference.rs:2",
        ),
        (
            "super-escape",
            &[("reference.rs", "use super::shim::enc;\n")],
            "shim.rs",
            1,
            "reference.rs:1",
        ),
        (
            "super-escape-nested-file",
            &[
                ("reference.rs", "mod inner;\n"),
                ("reference/inner.rs", "use super::super::shim::enc;\n"),
            ],
            "shim.rs",
            1,
            "reference/inner.rs:1",
        ),
        (
            "super-escape-inline-mod",
            &[(
                "reference.rs",
                "mod a {\n    mod b {\n        use super::super::super::shim::enc;\n    }\n}\n",
            )],
            "shim.rs",
            1,
            "reference.rs:3",
        ),
        (
            "dollar-crate",
            &[(
                "reference.rs",
                "macro_rules! m { () => { $crate::shim::enc::x() }; }\n",
            )],
            "shim.rs",
            1,
            "reference.rs:1",
        ),
        (
            "transitive",
            &[
                ("lib.rs", "pub mod reference;\npub mod a;\npub mod shim;\n"),
                ("reference.rs", "use crate::a::Thing;\n"),
                ("a.rs", "pub use crate::shim::enc as Thing;\n"),
            ],
            "shim.rs",
            1,
            "a.rs:1",
        ),
        (
            "dir-module",
            &[
                ("reference/mod.rs", "use crate::shim::enc;\n"),
                ("shim/mod.rs", "pub mod deep;\n"),
                ("shim/deep.rs", SHIM),
            ],
            "shim/deep.rs",
            1,
            "reference/mod.rs:1",
        ),
    ];
    for (name, files, bad, line, via) in cases {
        let mut all: Vec<(&str, &str)> = vec![("lib.rs", LIB_WITH_SHIM), ("shim.rs", SHIM)];
        for f in *files {
            all.retain(|(p, _)| *p != f.0);
            all.push(*f);
        }
        if *bad != "shim.rs" {
            all.retain(|(p, _)| *p != "shim.rs");
        }
        let root = planted(name, &all).unwrap();
        let got = violations(&root, &REQUIRED);
        assert_eq!(
            got,
            [Finding {
                path: format!("{SRC}/{bad}"),
                line: *line,
                kind: rr_core(),
                via: Some(format!("{SRC}/{via}")),
            }],
            "case {name}"
        );
    }
}

/// A glob or alias of the crate root, or a path to an item defined in
/// `lib.rs`, pulls in the whole crate: rr_core anywhere in it is reported.
#[test]
fn planted_crate_root_reach_checks_the_whole_crate() {
    let lib = "pub mod reference;\npub mod other;\npub use rr_core::encoding;\n";
    for (name, src) in [
        ("super-glob", "use super::*;\n"),
        ("crate-glob", "use crate::*;\n"),
        ("crate-self", "use crate::{self as tk};\n"),
        ("crate-alias", "use crate as tk;\n"),
        ("lib-item", "use crate::encoding::Storage;\n"),
        (
            "unknown-root-macro",
            "pub fn f() { crate::enc_macro!(); }\n",
        ),
    ] {
        let root = planted(
            name,
            &[
                ("lib.rs", lib),
                ("reference.rs", src),
                ("other.rs", "use rr_core::expr;\n"),
            ],
        )
        .unwrap();
        let got: Vec<(String, usize)> = violations(&root, &REQUIRED)
            .into_iter()
            .map(|f| (f.path, f.line))
            .collect();
        assert_eq!(
            got,
            [(format!("{SRC}/lib.rs"), 3), (format!("{SRC}/other.rs"), 1)],
            "case {name}"
        );
    }
}

/// Forms that pull in source or macros this check cannot see.
#[test]
fn planted_unseeable_source_fails() {
    let cases: &[(&str, &str, Kind)] = &[
        (
            "path-attr",
            "#[path = \"../shim.rs\"]\nmod x;\n",
            Kind::PathAttr,
        ),
        (
            "cfg-attr-path",
            "#[cfg_attr(all(), path = \"../shim.rs\")]\nmod x;\n",
            Kind::PathAttr,
        ),
        (
            "nested-cfg-attr-path",
            "#[cfg_attr(unix, cfg_attr(all(), path = \"../shim.rs\"))]\nmod x;\n",
            Kind::PathAttr,
        ),
        ("include", "include!(\"../shim.rs\");\n", Kind::Include),
        (
            "extern-self",
            "extern crate self as tk;\n",
            Kind::ExternCrate("self".into()),
        ),
        (
            "extern-other",
            "extern crate proptest as p;\n",
            Kind::ExternCrate("proptest".into()),
        ),
        (
            "crate-macro",
            "pub fn f() { crate_level_encode!(1); }\n",
            Kind::UnknownMacro("crate_level_encode".into()),
        ),
    ];
    for (name, src, kind) in cases {
        let root = planted(name, &[("reference.rs", src)]).unwrap();
        let got: Vec<Kind> = violations(&root, &REQUIRED)
            .into_iter()
            .map(|f| f.kind)
            .collect();
        assert_eq!(got, std::slice::from_ref(kind), "case {name}");
    }
}

/// Cargo-level evasions: the reference module names rr-core (or a local
/// crate that could re-export it) under a dependency key that is not a
/// workspace crate name. `(name, root manifest extra, rr-testkit deps, source,
/// expected kind)`.
type CargoCase<'a> = (&'a str, &'a str, &'a str, &'a str, Kind);

#[test]
fn planted_cargo_level_evasions_fail() {
    let cases: &[CargoCase<'_>] = &[
        (
            "dep-rename",
            "",
            "rrc = { package = \"rr-core\", path = \"../rr-core\" }\n",
            "use rrc::encoding::Storage;\n",
            Kind::LocalDependency("rrc".into(), "rr-core".into()),
        ),
        (
            "dep-rename-dotted-dev",
            "",
            "[dev-dependencies.rr-enc]\npackage = \"rr-core\"\npath = \"../rr-core\"\n",
            "#[cfg(test)]\nmod t { use rr_enc::conversion::Conv; }\n",
            Kind::LocalDependency("rr_enc".into(), "rr-core".into()),
        ),
        (
            "local-shim-crate",
            "",
            "shim = { path = \"../../shim\" }\n",
            "pub use shim::encoding as enc;\n",
            Kind::LocalDependency("shim".into(), "shim".into()),
        ),
        (
            "git-dep",
            "",
            "enc = { git = \"https://example.invalid/r.git\", package = \"codec\" }\n",
            "extern crate enc;\n",
            Kind::LocalDependency("enc".into(), "codec".into()),
        ),
        (
            "workspace-dep-rename",
            "[workspace.dependencies]\nenc = { package = \"rr-core\", path = \"crates/rr-core\" }\n",
            "enc.workspace = true\n",
            "pub fn f() { enc::encoding::x(); }\n",
            Kind::LocalDependency("enc".into(), "rr-core".into()),
        ),
        (
            "target-dep-rename",
            "",
            "[target.'cfg(unix)'.dependencies]\nc = { package = \"rr-core\", path = \"../rr-core\" }\n",
            "use c::*;\n",
            Kind::LocalDependency("c".into(), "rr-core".into()),
        ),
    ];
    for (name, root_extra, deps, src, kind) in cases {
        let root = planted(name, &[("reference.rs", src)]).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nmembers = [\"crates/*\"]\n{root_extra}"),
        )
        .unwrap();
        let deps = if deps.starts_with('[') {
            (*deps).to_owned()
        } else {
            format!("[dependencies]\n{deps}")
        };
        fs::write(
            root.join("crates/rr-testkit/Cargo.toml"),
            format!("[package]\nname = \"rr-testkit\"\nversion = \"0.0.0\"\n\n{deps}"),
        )
        .unwrap();
        let got: Vec<Kind> = violations(&root, &REQUIRED)
            .into_iter()
            .map(|f| f.kind)
            .collect();
        assert_eq!(got, std::slice::from_ref(kind), "case {name}");
    }
}

/// Registry dependencies of the holding crate stay usable: a crates.io crate
/// cannot depend on the path-only rr-core, so naming one is not an evasion.
#[test]
fn registry_dependency_is_allowed() {
    let root = planted(
        "registry-dep",
        &[(
            "reference.rs",
            "pub fn f() -> bool { toml::Value::Boolean(true).is_bool() }\n",
        )],
    )
    .unwrap();
    fs::write(
        root.join("crates/rr-testkit/Cargo.toml"),
        "[package]\nname = \"rr-testkit\"\nversion = \"0.0.0\"\n\n[dependencies]\nrr-core = { path = \"../rr-core\" }\ntoml = \"0.8\"\nserde = { version = \"1\", features = [\"derive\"] }\n",
    )
    .unwrap();
    assert_eq!(violations(&root, &REQUIRED), []);
}

/// A self-contained module passes: comments and strings naming rr_core,
/// visibility paths, local macros, std / path-qualified macros, `!(..)` after
/// keywords, `crate::reference` / `self::` / in-module `super::` paths.
#[test]
fn clean_reference_module_passes() {
    let root = planted(
        "clean",
        &[
            (
                "reference.rs",
                r#"//! Naive codec. Not rr_core::encoding (see `rr_core::expr`).
// use rr_core::encoding;
pub mod inner;
pub(crate) fn be_u16(b: &[u8]) -> u16 {
    let s = "rr_core::encoding::decode";
    let _ = s;
    if !(b.len() >= 2) { return 0; }
    u16::from(b[0]) << 8 | u16::from(b[1])
}
macro_rules! shl { ($v:expr, $n:expr) => { ($v as u32) << $n }; }
pub(super) fn word(b: &[u8]) -> u32 {
    assert!(b.len() >= 4, "{}", format!("short: {}", b.len()));
    std::debug_assert!(true);
    shl!(b[0], 24) | self::inner::low(b) | crate::reference::inner::zero() | crate::exported!()
}
#[macro_export]
macro_rules! exported { () => { 0u32 }; }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() { assert_eq!(be_u16(&[1, 2]), 0x0102); }
}
"#,
            ),
            (
                "reference/inner.rs",
                "use super::be_u16;\npub(in crate::reference) fn low(b: &[u8]) -> u32 { u32::from(be_u16(&b[2..])) }\npub fn zero() -> u32 { 0 }\n",
            ),
        ],
    )
    .unwrap();
    let report = reference_isolation::run(&root, &REQUIRED).unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(report.module_present);
    assert_eq!(report.files.len(), 2, "{:?}", report.files);
    assert!(report.reached.is_empty(), "{:?}", report.reached);
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let p = e?.path();
        let dest = to.join(p.file_name().unwrap_or_default());
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            fs::copy(&p, &dest)?;
        }
    }
    Ok(())
}

/// What US-RR-10-5 may legitimately do: take the real rr-testkit sources, add
/// a reference module that uses the schema-1 `kat` types, and it passes (the
/// closure over `kat` and what `kat` uses is checked and is rr-core free).
#[test]
fn reference_using_real_kat_types_passes() {
    let root = planted("real-kat", &[]).unwrap();
    let src = root.join(SRC);
    fs::remove_dir_all(&src).unwrap();
    copy_dir(&workspace_root().join(SRC), &src).unwrap();
    let _ = fs::remove_file(src.join("reference.rs"));
    let _ = fs::remove_dir_all(src.join("reference"));
    fs::write(
        src.join("reference.rs"),
        "use crate::kat::{Endian, Storage};\npub fn f(_: Storage, _: Endian) {}\n",
    )
    .unwrap();
    let report = reference_isolation::run(&root, &REQUIRED).unwrap();
    let msgs: Vec<String> = report.problems.iter().map(ToString::to_string).collect();
    assert!(report.problems.is_empty(), "{}", msgs.join("\n"));
    assert!(
        report.reached.iter().any(|m| m == "kat"),
        "{:?}",
        report.reached
    );
}

/// The rule cannot silently cover nothing.
#[test]
fn rule_is_not_vacuous() {
    // Absent and not required (before US-RR-10-5): passes, reports nothing checked.
    let absent = planted("absent", &[("lib.rs", "pub mod kat;\n")]).unwrap();
    let r = reference_isolation::run(&absent, &NOT_REQUIRED).unwrap();
    assert!(r.problems.is_empty() && !r.module_present && r.files.is_empty());

    // Absent under the committed rule: fails (RULE.required is true since
    // US-RR-10-5 added the module).
    let r = reference_isolation::run(&absent, &RULE).unwrap();
    assert!(
        matches!(r.problems.as_slice(), [Problem::ModuleMissing { .. }]),
        "{:?}",
        r.problems
    );

    // Present but `required` still false: fails, telling 10-5 to flip it.
    let present = planted("present", &[("reference.rs", "pub fn f() {}\n")]).unwrap();
    let r = reference_isolation::run(&present, &NOT_REQUIRED).unwrap();
    assert!(
        matches!(r.problems.as_slice(), [Problem::NotRequired { .. }]),
        "{:?}",
        r.problems
    );

    // Required and renamed away: fails.
    let renamed = planted(
        "renamed",
        &[
            ("lib.rs", "pub mod refcodec;\n"),
            ("refcodec.rs", "use rr_core::encoding;\n"),
        ],
    )
    .unwrap();
    let r = reference_isolation::run(&renamed, &REQUIRED).unwrap();
    assert!(
        matches!(r.problems.as_slice(), [Problem::ModuleMissing { .. }]),
        "{:?}",
        r.problems
    );

    // The crate itself moved: error, not a pass.
    let gone = planted("gone", &[]).unwrap();
    fs::remove_dir_all(gone.join(SRC)).unwrap();
    assert!(reference_isolation::run(&gone, &RULE).is_err());

    // A file that does not lex fails closed.
    let broken = planted("broken", &[("reference.rs", "pub fn f() {\n")]).unwrap();
    assert!(reference_isolation::run(&broken, &REQUIRED).is_err());
}

/// As CI runs it: the binary exits non-zero on a planted violation, naming
/// file:line, and zero on the committed-rule clean state.
#[test]
fn cli_fails_on_planted_violation() {
    let run = |root: &Path| {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["check-reference-isolation", "--root"])
            .arg(root)
            .output()
            .unwrap()
    };
    let bad = planted("cli-bad", &[("reference.rs", "use rr_core::encoding;\n")]).unwrap();
    let out = run(&bad);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "passed: {stderr}");
    assert!(
        stderr.contains(&format!(
            "{SRC}/reference.rs:1: names workspace crate `rr_core`"
        )),
        "{stderr}"
    );

    let out = run(&workspace_root());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn wiring_problems(ci: &str) -> Vec<String> {
    let wf = Workflow::parse(ci);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, JOB));
    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    if job.value_at("if", 4).is_some() {
        problems.push(format!("`{JOB}` has a job-level `if:`"));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == CMD) {
        problems.push(format!(
            "`{JOB}` does not run `{CMD}` (scripts: {scripts:?})"
        ));
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_runs_the_isolation_check_as_a_gate() {
    let problems = wiring_problems(&ci_yml());
    assert!(
        problems.is_empty(),
        ".github/workflows/ci.yml does not gate on {CMD}:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_isolation_check_wiring_rejects_weakened_ci() {
    let m = |edit: &dyn Fn(&str) -> String| workflow::mutated(wiring_problems, edit);
    let step = format!("        run: {CMD}\n");
    assert_flags(
        &m(&|s| s.replace(&step, "        run: echo skipped\n")),
        "does not run `cargo xtask check-reference-isolation`",
    );
    assert_flags(
        &m(&|s| s.replace(&step, &format!("        run: {CMD} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &format!("      - name: {CMD}\n"),
                &format!("      - name: {CMD}\n        if: false\n"),
            )
        }),
        "if:",
    );
}
