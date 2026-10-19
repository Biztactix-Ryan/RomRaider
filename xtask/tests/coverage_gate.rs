//! Tests for `cargo xtask coverage-gate` (`xtask::coverage_gate`, task
//! US-RR-4-5): synthetic lcov reports in, verdicts out. A report at or above
//! the minimum with no regression passes; one below the minimum, or below the
//! base, fails; a crate with code but no coverage data fails; one with no
//! functions at all passes. The last tests run the real binary and check its
//! exit status, which is what fails the CI `coverage-gate` job; the final one
//! (task US-RR-4-1) runs it with the exact arguments of the `gate` job in
//! `.github/workflows/coverage.yml`.

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use workflow::{Workflow, workflow_yml};
use xtask::coverage_gate::{
    self, GATED_CRATES, LineCount, Points, Report, check, crate_of, parse_lcov, parse_points,
    src_has_functions,
};

/// One lcov record for `path` with `found` lines, the first `hit` of them hit
/// (DA lines plus matching LF/LH, as cargo-llvm-cov writes them).
fn record(path: &str, found: u32, hit: u32) -> String {
    let mut s = format!("SF:{path}\nFN:1,f\nFNDA:1,f\n");
    for line in 1..=found {
        let hits = u32::from(line <= hit) * 3;
        s.push_str(&format!("DA:{line},{hits}\n"));
    }
    s.push_str(&format!("LF:{found}\nLH:{hit}\nend_of_record\n"));
    s
}

/// A report with rr-core at `core` (found, hit) split over two files, plus an
/// rr-core integration test, an rr-testkit file and an xtask file that must
/// not count.
fn report(core: (u32, u32)) -> String {
    let (found, hit) = core;
    let first = found / 2;
    let first_hit = hit.min(first);
    [
        record("/w/RomRaider/crates/rr-core/src/clock.rs", first, first_hit),
        record(
            "/w/RomRaider/crates/rr-core/src/diag/codes.rs",
            found - first,
            hit - first_hit,
        ),
        record("/w/RomRaider/crates/rr-core/tests/clock.rs", 50, 0),
        record("/w/RomRaider/crates/rr-testkit/src/kat.rs", 50, 0),
        record("/w/RomRaider/xtask/src/main.rs", 50, 0),
    ]
    .concat()
}

fn crates(names: &[&str]) -> Vec<String> {
    names.iter().map(|&n| n.to_owned()).collect()
}

fn gate(head: &str, base: Option<&str>, names: &[&str], has_code: bool) -> Result<Report> {
    let head = parse_lcov(head)?;
    let base = base.map(parse_lcov).transpose()?;
    check(&head, base.as_ref(), &crates(names), 90, 0, |_| {
        Ok(has_code)
    })
}

fn assert_flags(report: &Report, needle: &str) {
    assert!(
        report.problems.iter().any(|p| p.contains(needle)),
        "expected a problem mentioning {needle:?}, got {:#?}",
        report.problems
    );
}

#[test]
fn ci_gates_the_three_safety_crates_at_ninety_percent() {
    assert_eq!(GATED_CRATES, ["rr-core", "rr-rom", "rr-comms"]);
    assert_eq!(coverage_gate::DEFAULT_MIN_PERCENT, 90);
}

#[test]
fn files_map_to_crates_by_src_dir_only() {
    let cases = [
        ("/w/R/crates/rr-core/src/lib.rs", Some("rr-core")),
        ("crates/rr-rom/src/journal/mod.rs", Some("rr-rom")),
        (r"C:\a\R\crates\rr-comms\src\gate.rs", Some("rr-comms")),
        // Last match wins: a workspace checked out under a `crates` dir.
        ("/crates/x/src/crates/rr-core/src/a.rs", Some("rr-core")),
        ("/w/R/crates/rr-core/tests/clock.rs", None),
        ("/w/R/crates/rr-core/benches/b.rs", None),
        ("/w/R/xtask/src/main.rs", None),
        (
            "/home/u/.cargo/registry/src/index.crates.io-1/serde-1/src/lib.rs",
            None,
        ),
    ];
    for (path, want) in cases {
        assert_eq!(crate_of(path).as_deref(), want, "{path}");
    }
}

#[test]
fn parse_uses_lf_lh_and_falls_back_to_da() {
    let lcov = parse_lcov(&record("crates/rr-core/src/a.rs", 10, 7)).unwrap();
    assert_eq!(
        lcov.files["crates/rr-core/src/a.rs"],
        LineCount { found: 10, hit: 7 }
    );
    // LF/LH win over DA (llvm-cov's own line summary).
    let lcov = parse_lcov("SF:a.rs\nDA:1,1\nDA:2,0\nLF:3\nLH:1\nend_of_record\n").unwrap();
    assert_eq!(lcov.files["a.rs"], LineCount { found: 3, hit: 1 });
    // No LF/LH: distinct DA lines, a repeated line counted once and hit if
    // any entry hit it; a trailing record without end_of_record still counts.
    let lcov = parse_lcov("SF:a.rs\nDA:1,0\nDA:1,4\nDA:2,0\nDA:3,0,abc\n").unwrap();
    assert_eq!(lcov.files["a.rs"], LineCount { found: 3, hit: 1 });
}

#[test]
fn parse_rejects_empty_or_malformed_reports() {
    for (text, needle) in [
        ("", "empty"),
        ("TN:\n", "empty"),
        ("DA:1,1\n", "outside an `SF:` record"),
        ("SF:a.rs\nDA:x,1\nend_of_record\n", "malformed"),
        ("SF:a.rs\nDA:1\nend_of_record\n", "malformed"),
        ("SF:a.rs\nDA:1,-1\nend_of_record\n", "malformed"),
        ("SF:a.rs\nLF:2\nLH:3\nend_of_record\n", "above LF"),
        (
            "SF:a.rs\nLF:1\nLH:1\nend_of_record\nSF:a.rs\nLF:1\nLH:0\nend_of_record\n",
            "more than one",
        ),
    ] {
        let err = parse_lcov(text).unwrap_err();
        assert!(
            format!("{err:#}").contains(needle),
            "{text:?}: expected {needle:?}, got {err:#}"
        );
    }
}

#[test]
fn threshold_is_exact_integer_comparison() {
    assert!(LineCount { found: 10, hit: 9 }.meets(90));
    assert!(
        LineCount {
            found: 10_000,
            hit: 9_000
        }
        .meets(90)
    );
    assert!(
        !LineCount {
            found: 10_000,
            hit: 8_999
        }
        .meets(90)
    );
    assert!(LineCount { found: 1, hit: 0 }.meets(0));
    assert!(!LineCount { found: 0, hit: 0 }.meets(0));
    assert!(LineCount { found: 3, hit: 3 }.meets(100));
}

#[test]
fn passes_at_or_above_minimum_with_no_regression() -> Result<()> {
    let r = gate(
        &report((1000, 900)),
        Some(&report((1000, 900))),
        &["rr-core"],
        true,
    )?;
    assert!(r.problems.is_empty(), "{:#?}", r.problems);
    assert_eq!(
        r.rows[0].head,
        LineCount {
            found: 1000,
            hit: 900
        }
    );
    // Improvement over the base passes.
    let r = gate(
        &report((1000, 950)),
        Some(&report((1000, 900))),
        &["rr-core"],
        true,
    )?;
    assert!(r.problems.is_empty(), "{:#?}", r.problems);
    // No base: threshold only.
    let r = gate(&report((1000, 990)), None, &["rr-core"], true)?;
    assert!(r.problems.is_empty(), "{:#?}", r.problems);
    Ok(())
}

#[test]
fn fails_below_minimum() -> Result<()> {
    let r = gate(&report((1000, 899)), None, &["rr-core"], true)?;
    assert_flags(
        &r,
        "rr-core: line coverage 89.90% (899/1000) is below the 90% minimum",
    );
    // Even when the base was just as low (no regression, still below).
    let r = gate(
        &report((1000, 500)),
        Some(&report((1000, 500))),
        &["rr-core"],
        true,
    )?;
    assert_eq!(r.problems.len(), 1, "{:#?}", r.problems);
    assert_flags(&r, "below the 90% minimum");
    Ok(())
}

#[test]
fn fails_on_regression_against_the_base_even_above_minimum() -> Result<()> {
    let r = gate(
        &report((1000, 950)),
        Some(&report((1000, 951))),
        &["rr-core"],
        true,
    )?;
    assert_eq!(r.problems.len(), 1, "{:#?}", r.problems);
    assert_flags(
        &r,
        "rr-core: line coverage regressed from 95.10% (951/1000) on the base to 95.00% (950/1000)",
    );
    // Ratios, not counts: more lines hit but a lower share still regresses.
    let r = gate(
        &report((2000, 1900)),
        Some(&report((1000, 960))),
        &["rr-core"],
        true,
    )?;
    assert_flags(&r, "regressed");
    // Equal ratio with different counts does not.
    let r = gate(
        &report((2000, 1900)),
        Some(&report((1000, 950))),
        &["rr-core"],
        true,
    )?;
    assert!(r.problems.is_empty(), "{:#?}", r.problems);
    Ok(())
}

#[test]
fn each_crate_is_gated_on_its_own_files() -> Result<()> {
    // rr-core is fine; rr-rom has code and 80%; rr-comms regressed.
    let head = [
        report((1000, 950)),
        record("crates/rr-rom/src/journal.rs", 100, 80),
        record("crates/rr-comms/src/gate.rs", 100, 95),
    ]
    .concat();
    let base = [
        report((1000, 950)),
        record("crates/rr-comms/src/gate.rs", 100, 99),
    ]
    .concat();
    let r = gate(&head, Some(&base), GATED_CRATES, true)?;
    assert_eq!(r.problems.len(), 2, "{:#?}", r.problems);
    assert_flags(&r, "rr-rom: line coverage 80.00% (80/100) is below");
    assert_flags(&r, "rr-comms: line coverage regressed from 99.00%");
    // rr-rom has no lines on the base: threshold applies, regression skipped.
    let rom = &r.rows[1];
    assert_eq!(rom.base, Some(LineCount::default()));
    assert!(
        rom.notes
            .iter()
            .any(|n| n.contains("regression check skipped"))
    );
    Ok(())
}

#[test]
fn crate_without_coverage_data_fails_if_it_has_code() -> Result<()> {
    let r = gate(&report((1000, 950)), None, &["rr-rom"], true)?;
    assert_flags(
        &r,
        "rr-rom: crates/rr-rom/src has functions but the coverage report has no instrumented lines",
    );
    let r = gate(&report((1000, 950)), None, &["rr-rom"], false)?;
    assert!(r.problems.is_empty(), "{:#?}", r.problems);
    assert!(r.rows[0].notes.iter().any(|n| n.contains("no functions")));
    Ok(())
}

#[test]
fn regression_tolerance_is_in_hundredths_of_a_point() -> Result<()> {
    let base = LineCount {
        found: 10_000,
        hit: 9_500,
    };
    // 95.00% -> 94.75%: a 0.25-point drop.
    let head = LineCount {
        found: 10_000,
        hit: 9_475,
    };
    assert!(head.regressed_from(base, 0));
    assert!(head.regressed_from(base, 24));
    assert!(!head.regressed_from(base, 25));
    assert!(!base.regressed_from(base, 0));
    // The measured noise: 3 of 2176 lines (0.14 points) passes at 0.25.
    let base = LineCount {
        found: 2176,
        hit: 2137,
    };
    let noisy = LineCount {
        found: 2176,
        hit: 2134,
    };
    assert!(noisy.regressed_from(base, 0));
    assert!(!noisy.regressed_from(base, 25));
    assert!(
        LineCount {
            found: 2176,
            hit: 2131
        }
        .regressed_from(base, 25)
    );

    let head = parse_lcov(&report((10_000, 9_475)))?;
    let base = parse_lcov(&report((10_000, 9_500)))?;
    let names = crates(&["rr-core"]);
    let r = check(&head, Some(&base), &names, 90, 25, |_| Ok(true))?;
    assert!(r.problems.is_empty(), "{:#?}", r.problems);
    let r = check(&head, Some(&base), &names, 90, 20, |_| Ok(true))?;
    assert_flags(
        &r,
        "regressed from 95.00% (9500/10000) on the base to 94.75% (9475/10000) (allowed drop: 0.20 points)",
    );
    // The tolerance never relaxes the minimum.
    let head = parse_lcov(&report((10_000, 8_999)))?;
    let base = parse_lcov(&report((10_000, 9_000)))?;
    let r = check(&head, Some(&base), &names, 90, 25, |_| Ok(true))?;
    assert_eq!(r.problems.len(), 1, "{:#?}", r.problems);
    assert_flags(&r, "below the 90% minimum");
    Ok(())
}

#[test]
fn points_parse_with_at_most_two_decimals() -> Result<()> {
    for (text, want) in [
        ("0", 0),
        ("0.25", 25),
        ("0.2", 20),
        ("1", 100),
        ("100", 10_000),
    ] {
        assert_eq!(parse_points(text)?, want, "{text}");
    }
    for bad in [
        "",
        ".5",
        "1.",
        "0.255",
        "-1",
        "abc",
        "1e2",
        "100.01",
        "99999999999",
    ] {
        assert!(parse_points(bad).is_err(), "{bad:?}");
    }
    assert_eq!(Points(25).to_string(), "0.25 points");
    assert_eq!(Points(100).to_string(), "1.00 points");
    Ok(())
}

fn scratch(name: &str) -> io::Result<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("coverage-gate")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn src_has_functions_sees_fn_tokens_only() -> io::Result<()> {
    let dir = scratch("src-scan")?;
    let src = dir.join("src");
    fs::create_dir_all(src.join("nested"))?;
    fs::write(
        src.join("lib.rs"),
        "//! Docs mentioning fn main() {}\n/* fn x() {} */\nconst S: &str = \"fn y() {}\";\npub struct A;\n",
    )?;
    assert!(!src_has_functions(&src).unwrap());
    fs::write(
        src.join("nested/m.rs"),
        "impl A { pub fn new() -> Self { A } }\n",
    )?;
    assert!(src_has_functions(&src).unwrap());
    fs::write(src.join("nested/m.rs"), "pub struct B;\n")?;
    assert!(!src_has_functions(&src).unwrap());
    // Unlexable source counts as code (fail closed).
    fs::write(src.join("nested/m.rs"), "pub struct B { \"unterminated\n")?;
    assert!(src_has_functions(&src).unwrap());
    assert!(src_has_functions(&dir.join("missing")).is_err());
    Ok(())
}

#[test]
fn gated_crates_exist_and_rr_core_has_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // rr-core has code; the gate must demand coverage data for it.
    assert!(src_has_functions(&root.join("crates/rr-core/src")).unwrap());
    for name in GATED_CRATES {
        assert!(
            root.join("crates").join(name).join("src").is_dir(),
            "{name}"
        );
    }
}

/// A workspace root under `dir` holding `crates/<name>/src/lib.rs` for each
/// gated crate, with a function in it for the crates in `with_code` and none
/// in the others. The binary tests use it instead of the real workspace, so
/// their verdicts do not change the day rr-rom or rr-comms gains code.
fn fake_root(dir: &Path, with_code: &[&str]) -> io::Result<PathBuf> {
    let root = dir.join("root");
    for name in GATED_CRATES {
        let src = root.join("crates").join(name).join("src");
        fs::create_dir_all(&src)?;
        let text = if with_code.contains(name) {
            "pub fn f() {}\n"
        } else {
            "//! No code yet.\n"
        };
        fs::write(src.join("lib.rs"), text)?;
    }
    Ok(root)
}

/// Runs `xtask coverage-gate --root <root> <args>` in `dir`: exit code and
/// output.
fn xtask_in(root: &Path, dir: &Path, args: &[&str]) -> io::Result<(i32, String)> {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("coverage-gate")
        .arg("--root")
        .arg(root)
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

/// The CI invocation's arguments, with `head` and `base` reports.
fn ci_args<'a>(head: &'a str, base: &'a str) -> Vec<&'a str> {
    vec![
        "--lcov",
        head,
        "--base-lcov",
        base,
        "--min",
        "90",
        "--crates",
        "rr-core,rr-rom,rr-comms",
    ]
}

#[test]
fn binary_exit_status_follows_the_verdict() -> io::Result<()> {
    let dir = scratch("binary")?;
    // rr-core has code, rr-rom and rr-comms none (the reports below only
    // cover rr-core).
    let root = fake_root(&dir, &["rr-core"])?;
    let xtask = |dir: &Path, args: &[&str]| xtask_in(&root, dir, args);
    let write = |name: &str, text: &str| fs::write(dir.join(name), text);
    write("ok.info", &report((1000, 950)))?;
    write("better.info", &report((1000, 960)))?;
    write("low.info", &report((1000, 800)))?;
    write("empty.info", "")?;

    let (code, out) = xtask(&dir, &ci_args("ok.info", "ok.info"))?;
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("coverage-gate: rr-core: line coverage 95.00% (950/1000)"),
        "{out}"
    );
    assert!(out.contains("coverage-gate: ok"), "{out}");

    let (code, out) = xtask(&dir, &ci_args("low.info", "low.info"))?;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("below the 90% minimum"), "{out}");

    let (code, out) = xtask(&dir, &ci_args("ok.info", "better.info"))?;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("regressed from 96.00%"), "{out}");

    // An empty head report is an error, never a pass.
    let (code, out) = xtask(&dir, &ci_args("empty.info", "ok.info"))?;
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("empty"), "{out}");

    // A missing base report is an error, not "no regression".
    let (code, out) = xtask(&dir, &ci_args("ok.info", "missing.info"))?;
    assert_ne!(code, 0, "{out}");

    // --max-drop admits a drop within it, and only that.
    let mut args = ci_args("ok.info", "better.info");
    args.extend(["--max-drop", "1"]);
    let (code, out) = xtask(&dir, &args)?;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("beyond 1.00 points"), "{out}");
    let mut args = ci_args("ok.info", "better.info");
    args.extend(["--max-drop", "0.99"]);
    let (code, out) = xtask(&dir, &args)?;
    assert_eq!(code, 1, "{out}");
    let mut args = ci_args("ok.info", "ok.info");
    args.extend(["--max-drop", "0.255"]);
    let (code, out) = xtask(&dir, &args)?;
    assert_ne!(code, 0, "{out}");

    // Defaults are the CI crates and minimum.
    let (code, out) = xtask(&dir, &["--lcov", "low.info"])?;
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("rr-comms"), "{out}");
    Ok(())
}

/// The arguments the CI `gate` job passes to `cargo xtask coverage-gate`,
/// read from `.github/workflows/coverage.yml`, with the `--lcov` and
/// `--base-lcov` paths replaced by `head` and `base`. An error if the job,
/// the command or either report flag is missing, so a reworded workflow
/// cannot make the test below pass without running the gate.
fn ci_gate_args_from_workflow(head: &str, base: &str) -> Result<Vec<String>> {
    let src = workflow_yml("coverage.yml");
    let wf = Workflow::parse(&src);
    let job = wf
        .job("gate")
        .ok_or_else(|| anyhow!("coverage.yml has no `gate` job"))?;
    let scripts = job.run_scripts();
    let cmd = scripts
        .iter()
        .find_map(|s| s.strip_prefix("cargo xtask coverage-gate "))
        .ok_or_else(|| anyhow!("`gate` does not run cargo xtask coverage-gate: {scripts:?}"))?;
    let mut args: Vec<String> = cmd.split(' ').map(str::to_owned).collect();
    for (flag, path) in [("--lcov", head), ("--base-lcov", base)] {
        let value = args
            .iter()
            .position(|a| a == flag)
            .and_then(|i| args.get_mut(i + 1))
            .ok_or_else(|| anyhow!("CI gate command has no `{flag} <path>`: {cmd}"))?;
        path.clone_into(value);
    }
    Ok(args)
}

/// An lcov report with one file per gated crate: `counts[i]` is
/// `GATED_CRATES[i]`'s (found, hit), `None` for no record at all.
fn gated_report(counts: [Option<(u32, u32)>; 3]) -> String {
    GATED_CRATES
        .iter()
        .zip(counts)
        .filter_map(|(name, c)| {
            c.map(|(found, hit)| record(&format!("/w/R/crates/{name}/src/lib.rs"), found, hit))
        })
        .chain([record("/w/R/crates/rr-testkit/src/lib.rs", 100, 0)])
        .collect()
}

/// Story US-RR-4, criterion 1 (task US-RR-4-1): "A PR that drops coverage of
/// rr-core/rr-rom/rr-comms below 90% or below the base branch fails". Runs
/// the real binary with the CI job's own arguments (`--min 90 --max-drop
/// 0.25 --crates rr-core,rr-rom,rr-comms`, D-027) on a workspace where all
/// three crates have code, and checks, for each crate in turn while the
/// other two are healthy:
///
/// * 89.99% fails and exactly 90.00% passes (the minimum, no rounding);
/// * a drop of 0.26 points below the base fails, even at 94.74%, and 0.25
///   points passes (the D-027 noise tolerance, and nothing more);
/// * a crate missing from the head report fails (no data is not a pass);
/// * a crate missing from the base report (new on this PR) is still held to
///   90%, with only the regression check skipped;
///
/// and that a missing base report file fails rather than skipping the
/// regression check.
#[test]
fn ci_gate_fails_a_pr_below_ninety_or_below_the_base_for_each_gated_crate() -> Result<()> {
    let dir = scratch("us-rr-4-1")?;
    let root = fake_root(&dir, GATED_CRATES)?;
    const OK: Option<(u32, u32)> = Some((10_000, 9_500));
    let run = |case: &str, head: [Option<(u32, u32)>; 3], base: [Option<(u32, u32)>; 3]| {
        let (h, b) = (format!("{case}-head.info"), format!("{case}-base.info"));
        fs::write(dir.join(&h), gated_report(head))?;
        fs::write(dir.join(&b), gated_report(base))?;
        let args = ci_gate_args_from_workflow(&h, &b)?;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        Ok::<_, anyhow::Error>(xtask_in(&root, &dir, &args)?)
    };
    let expect = |(code, out): (i32, String), want: i32, needle: &str, case: &str| {
        assert_eq!(code, want, "{case}: exit status\n{out}");
        assert!(
            out.contains(needle),
            "{case}: expected {needle:?} in\n{out}"
        );
    };

    // Control: all three healthy and unchanged.
    expect(
        run("control", [OK; 3], [OK; 3])?,
        0,
        "coverage-gate: ok (3 crate(s), minimum 90%, no regression against the base beyond 0.25 points)",
        "control",
    );

    for (i, name) in GATED_CRATES.iter().enumerate() {
        let with = |c: Option<(u32, u32)>| {
            let mut all = [OK; 3];
            all[i] = c;
            all
        };
        let case = |what: &str| format!("{name}-{what}");

        // Below the minimum fails, even with no regression (base just as low).
        let low = with(Some((10_000, 8_999)));
        expect(
            run(&case("below-90"), low, low)?,
            1,
            &format!(
                "coverage-gate: {name}: line coverage 89.99% (8999/10000) is below the 90% minimum"
            ),
            &case("below-90"),
        );
        // Exactly 90.00% passes.
        let exact = with(Some((10_000, 9_000)));
        expect(
            run(&case("exactly-90"), exact, exact)?,
            0,
            "coverage-gate: ok",
            &case("exactly-90"),
        );

        // Below the base by more than 0.25 points fails, above the minimum.
        let base = with(Some((10_000, 9_500)));
        expect(
            run(&case("below-base"), with(Some((10_000, 9_474))), base)?,
            1,
            &format!(
                "coverage-gate: {name}: line coverage regressed from 95.00% (9500/10000) on the base \
                 to 94.74% (9474/10000) (allowed drop: 0.25 points)"
            ),
            &case("below-base"),
        );
        // A drop of exactly the tolerance passes.
        expect(
            run(&case("at-tolerance"), with(Some((10_000, 9_475))), base)?,
            0,
            "coverage-gate: ok",
            &case("at-tolerance"),
        );

        // Missing from the head report, though the crate has code: fails.
        expect(
            run(&case("missing-head"), with(None), base)?,
            1,
            &format!(
                "coverage-gate: {name}: crates/{name}/src has functions but the coverage report has no instrumented lines"
            ),
            &case("missing-head"),
        );

        // Missing from the base report: still held to the minimum.
        expect(
            run(&case("new-ok"), exact, with(None))?,
            0,
            "no instrumented lines on the base; regression check skipped",
            &case("new-ok"),
        );
        expect(
            run(&case("new-low"), low, with(None))?,
            1,
            "is below the 90% minimum",
            &case("new-low"),
        );
    }

    // A missing base report is an error, never "no regression".
    fs::write(dir.join("head.info"), gated_report([OK; 3]))?;
    let args = ci_gate_args_from_workflow("head.info", "no-such-base.info")?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, out) = xtask_in(&root, &dir, &args)?;
    assert_ne!(code, 0, "missing base report passed:\n{out}");
    assert!(out.contains("no-such-base.info"), "{out}");
    Ok(())
}
