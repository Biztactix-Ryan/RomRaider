//! `ci.yml` wiring for story US-RR-34 (built by task US-RR-34-9, verified
//! by US-RR-34-4): the resolve and ROM-ID lookup budgets (MF-18, MF-40,
//! EC-10) are enforced on every push and pull request, as part of the one
//! required check.
//!
//! [`perf_budget_problems`] is a pure check over the text of
//! `.github/workflows/ci.yml`, of the bench it runs and of the files that
//! decide its profile; one test runs it on the committed files and the
//! `*_rejects_*` tests prove each assertion bites on a mutated copy. It
//! asserts that job `perf-budget`:
//!
//! * exists, runs on Linux, has a timeout, and is in `ci-ok.needs`; the
//!   workflow runs on `push` and `pull_request`;
//! * runs exactly `cargo bench -p rr-defs --bench resolve_lookup --locked
//!   -- --check`: the `bench` profile (release settings), and `--check`,
//!   without which the bench only prints;
//! * runs that profile optimised: no `CARGO_PROFILE_*`, `RUSTFLAGS` or
//!   `opt-level` in the job or the workflow's `env`, no `[profile.*]` in
//!   `.cargo/config.toml`, and no `opt-level` below 3 in the workspace's
//!   `[profile.release]` / `[profile.bench]` (a slow debug build would fail
//!   the gate for the wrong reason, a softened profile would hide nothing,
//!   but either changes what the budget means);
//! * sets no `RR_CORPUS_DIR` (the bench's corpus report is for a developer,
//!   never checked) and cannot be skipped or softened (`if:`,
//!   `continue-on-error`, `|| true`);
//!
//! and that the bench (`crates/rr-defs/benches/resolve_lookup.rs`, a
//! `harness = false` `[[bench]]` of rr-defs) still measures a 5000-table
//! definition and decides with `benches/support/budget.rs`, which declares
//! a 200 ms resolve budget, a 5 ms lookup budget and a tolerance of 1.5,
//! and which `crates/rr-defs/tests/perf_budget_gate.rs` includes to prove
//! the gate fails above budget × tolerance. What the bench measures and why
//! that tolerance is in its module docs; whether the job is green can only
//! be seen on GitHub.

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::path::Path;

use workflow::{
    Workflow, assert_flags, ci_yml, gate_needs_problems, skippable_problems, trigger_problems,
};

const JOB: &str = "perf-budget";
const CHECK: &str = "cargo bench -p rr-defs --bench resolve_lookup --locked -- --check";

/// The bench, relative to the workspace root.
const BENCH: &str = "crates/rr-defs/benches/resolve_lookup.rs";
/// The bench's budgets and gate decision, relative to the workspace root.
const BUDGET: &str = "crates/rr-defs/benches/support/budget.rs";
/// The test proving that decision fails a blown budget.
const GATE_TEST: &str = "crates/rr-defs/tests/perf_budget_gate.rs";
/// Its crate's manifest, relative to the workspace root.
const MANIFEST: &str = "crates/rr-defs/Cargo.toml";
/// The workspace manifest (the only one whose `[profile.*]` cargo reads).
const ROOT_MANIFEST: &str = "Cargo.toml";
/// Cargo's repository config, which can also set profiles.
const CARGO_CONFIG: &str = ".cargo/config.toml";

/// The size the bench must resolve (EC-10: "5k+ tables"), as declared in
/// its source.
const TABLES: &str = "const TABLES: usize = 5000;";

/// The budgets the gate must check (story US-RR-34; PROJECT.md MF-18,
/// MF-40, EC-10), as declared in `support/budget.rs`.
const BUDGETS: [&str; 3] = [
    "pub const RESOLVE_BUDGET: Duration = Duration::from_millis(200);",
    "pub const LOOKUP_BUDGET: Duration = Duration::from_millis(5);",
    "pub const TOLERANCE: f64 = 1.5;",
];

/// How the bench and the gate test include `support/budget.rs`.
const BENCH_INCLUDE: &str = "#[path = \"support/budget.rs\"]";
const TEST_INCLUDE: &str = "#[path = \"../benches/support/budget.rs\"]";
/// The bench's exit status is the gate's decision.
const BENCH_DECIDES: &str = "let verdict = budget::gate(&[";

/// The `[[bench]]` entry that makes `--bench resolve_lookup` the bench's
/// own `main`.
const BENCH_ENTRY: &str = "[[bench]]\nname = \"resolve_lookup\"\nharness = false\n";

/// `rel` under the workspace root, line endings normalised (an
/// `<unreadable ...>` marker if missing, which then fails the checks).
fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// The files the check reads, as text.
#[derive(Clone)]
struct Sources {
    ci: String,
    bench: String,
    budget: String,
    gate_test: String,
    manifest: String,
    root_manifest: String,
    cargo_config: String,
}

impl Sources {
    fn committed() -> Self {
        Self {
            ci: ci_yml(),
            bench: read(BENCH),
            budget: read(BUDGET),
            gate_test: read(GATE_TEST),
            manifest: read(MANIFEST),
            root_manifest: read(ROOT_MANIFEST),
            cargo_config: read(CARGO_CONFIG),
        }
    }
}

/// `true` if `src` has a line that is exactly `want` once trimmed.
fn declares(src: &str, want: &str) -> bool {
    src.lines().any(|l| l.trim() == want)
}

/// Problems if a `[profile.release]` or `[profile.bench]` of the workspace
/// manifest sets an `opt-level` other than 3.
fn profile_problems(root_manifest: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut section = "";
    for line in root_manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if !matches!(section, "[profile.release]" | "[profile.bench]") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "opt-level" && value.trim() != "3" {
            problems.push(format!(
                "{ROOT_MANIFEST} {section} sets `{line}`: the bench must run optimised"
            ));
        }
    }
    problems
}

/// Every way `src` fails the US-RR-34 perf-budget wiring; empty when it
/// holds.
fn perf_budget_problems(src: &Sources) -> Vec<String> {
    let wf = Workflow::parse(&src.ci);
    let mut problems = trigger_problems(&wf);
    problems.extend(gate_needs_problems(&wf, JOB));
    if !declares(&src.bench, TABLES) {
        problems.push(format!("{BENCH} does not declare `{TABLES}`"));
    }
    for want in BUDGETS {
        if !declares(&src.budget, want) {
            problems.push(format!("{BUDGET} does not declare `{want}`"));
        }
    }
    if !declares(&src.bench, BENCH_INCLUDE) || !src.bench.contains(BENCH_DECIDES) {
        problems.push(format!(
            "{BENCH} does not decide with {BUDGET} (`{BENCH_INCLUDE}`, `{BENCH_DECIDES}`)"
        ));
    }
    if !declares(&src.gate_test, TEST_INCLUDE) {
        problems.push(format!("{GATE_TEST} does not test {BUDGET}"));
    }
    problems.extend(profile_problems(&src.root_manifest));
    if src
        .cargo_config
        .lines()
        .any(|l| l.trim_start().starts_with("[profile") || l.trim_start().starts_with("profile."))
    {
        problems.push(format!("{CARGO_CONFIG} sets a cargo profile"));
    }
    for line in wf.section("env") {
        if ["CARGO_PROFILE_", "opt-level"]
            .iter()
            .any(|k| line.contains(k))
        {
            problems.push(format!(
                "workflow `env` changes the bench profile: `{}`",
                line.trim()
            ));
        }
    }
    if !src.manifest.contains(BENCH_ENTRY) {
        problems.push(format!(
            "{MANIFEST} has no `harness = false` [[bench]] `resolve_lookup`"
        ));
    }
    let Some(job) = wf.job(JOB) else {
        problems.push(format!("no `{JOB}` job"));
        return problems;
    };
    if !job
        .value_at("runs-on", 4)
        .is_some_and(|r| r.starts_with("ubuntu-"))
    {
        problems.push(format!("`{JOB}` does not run on an ubuntu- runner"));
    }
    if job.value_at("timeout-minutes", 4).is_none() {
        problems.push(format!("`{JOB}` has no timeout-minutes"));
    }
    let scripts = job.run_scripts();
    if !scripts.iter().any(|s| s == CHECK) {
        problems.push(format!(
            "`{JOB}` does not run exactly `{CHECK}` (scripts: {scripts:?})"
        ));
    }
    if job.content_lines().any(|l| l.contains("RR_CORPUS_DIR")) {
        problems.push(format!("`{JOB}` sets RR_CORPUS_DIR"));
    }
    for line in job.content_lines() {
        if ["CARGO_PROFILE_", "RUSTFLAGS", "opt-level"]
            .iter()
            .any(|k| line.contains(k))
        {
            problems.push(format!("`{JOB}` changes the bench profile: `{line}`"));
        }
    }
    problems.extend(skippable_problems(job, &[]));
    problems
}

#[test]
fn ci_enforces_the_resolve_and_lookup_budgets() {
    let problems = perf_budget_problems(&Sources::committed());
    assert!(
        problems.is_empty(),
        "US-RR-34-9 perf-budget wiring is broken:\n{}",
        problems.join("\n")
    );
}

#[test]
fn ci_perf_budget_check_rejects_weakened_job() {
    let m = |edit: &dyn Fn(&str) -> String| {
        workflow::mutated(
            |ci| {
                perf_budget_problems(&Sources {
                    ci: ci.to_owned(),
                    ..Sources::committed()
                })
            },
            edit,
        )
    };
    let run = format!("        run: {CHECK}\n");

    assert_flags(
        &m(&|s| s.replace(", perf-budget]", "]")),
        "`ci-ok.needs` does not include `perf-budget`",
    );
    assert_flags(
        &m(&|s| s.replace("\n  perf-budget:\n", "\n  perf:\n")),
        "no `perf-budget` job",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                "        run: cargo bench -p rr-defs --bench resolve_lookup --locked\n",
            )
        }),
        "does not run exactly",
    );
    assert_flags(
        &m(&|s| s.replace(&run, &format!("        run: {CHECK} || true\n"))),
        "swallows failures",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!("        if: github.event_name == 'push'\n{run}"),
            )
        }),
        "has a conditional",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!("        env:\n          RR_CORPUS_DIR: /corpus\n{run}"),
            )
        }),
        "sets RR_CORPUS_DIR",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                "        run: cargo bench -p rr-defs --bench resolve_lookup --locked --profile dev -- --check\n",
            )
        }),
        "does not run exactly",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                "        run: cargo test -p rr-defs --bench resolve_lookup --locked -- --check\n",
            )
        }),
        "does not run exactly",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!("        env:\n          CARGO_PROFILE_BENCH_OPT_LEVEL: \"0\"\n{run}"),
            )
        }),
        "changes the bench profile",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                &run,
                &format!("        env:\n          RUSTFLAGS: -C opt-level=0\n{run}"),
            )
        }),
        "changes the bench profile",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "env:\n  CARGO_TERM_COLOR: always\n",
                "env:\n  CARGO_TERM_COLOR: always\n  CARGO_PROFILE_RELEASE_OPT_LEVEL: \"1\"\n",
            )
        }),
        "workflow `env` changes the bench profile",
    );
    assert_flags(
        &m(&|s| {
            s.replace("    timeout-minutes: 20\n    steps:\n      - uses: actions/checkout@v5\n      - name: Install toolchain\n        run: rustup toolchain install --no-self-update && rustup show\n      - uses: Swatinem/rust-cache@v2\n        with:\n          key: perf-budget\n", "    steps:\n      - uses: actions/checkout@v5\n      - name: Install toolchain\n        run: rustup toolchain install --no-self-update && rustup show\n      - uses: Swatinem/rust-cache@v2\n        with:\n          key: perf-budget\n")
        }),
        "has no timeout-minutes",
    );
    assert_flags(
        &m(&|s| {
            s.replace(
                "    name: perf budgets (resolve, ROM-ID lookup)\n    runs-on: ubuntu-24.04\n",
                "    name: perf budgets (resolve, ROM-ID lookup)\n    runs-on: windows-2022\n",
            )
        }),
        "does not run on an ubuntu- runner",
    );
}

#[test]
fn ci_perf_budget_check_rejects_weakened_bench() {
    let committed = Sources::committed();
    let flags = |edit: &dyn Fn(&mut Sources), needle: &str| {
        let mut src = committed.clone();
        edit(&mut src);
        assert_flags(&perf_budget_problems(&src), needle);
    };
    let swap = |text: &mut String, from: &str, to: &str| {
        let changed = text.replace(from, to);
        assert_ne!(&changed, text, "mutation {from:?} did not apply");
        *text = changed;
    };

    flags(
        &|s| {
            swap(
                &mut s.bench,
                "TABLES: usize = 5000;",
                "TABLES: usize = 500;",
            )
        },
        "does not declare",
    );
    for (from, to) in [
        (
            "Duration::from_millis(200);",
            "Duration::from_millis(2000);",
        ),
        ("Duration::from_millis(5);", "Duration::from_millis(50);"),
        ("TOLERANCE: f64 = 1.5;", "TOLERANCE: f64 = 10.0;"),
    ] {
        flags(&|s| swap(&mut s.budget, from, to), "does not declare");
    }
    flags(
        &|s| swap(&mut s.bench, BENCH_DECIDES, "let verdict = my_gate(&["),
        "does not decide with",
    );
    flags(
        &|s| {
            swap(
                &mut s.bench,
                BENCH_INCLUDE,
                "#[path = \"support/lenient.rs\"]",
            );
        },
        "does not decide with",
    );
    flags(
        &|s| swap(&mut s.gate_test, TEST_INCLUDE, "#[path = \"lenient.rs\"]"),
        "does not test",
    );
    flags(
        &|s| swap(&mut s.gate_test, TEST_INCLUDE, ""),
        "does not test",
    );
    flags(
        &|s| {
            swap(
                &mut s.manifest,
                "name = \"resolve_lookup\"\nharness = false",
                "name = \"resolve_lookup\"",
            );
        },
        "no `harness = false` [[bench]]",
    );
    flags(
        &|s| {
            s.root_manifest
                .push_str("\n[profile.bench]\nopt-level = 0\n")
        },
        "the bench must run optimised",
    );
    flags(
        &|s| {
            s.root_manifest
                .push_str("\n[profile.release]\nopt-level = \"s\"\n")
        },
        "the bench must run optimised",
    );
    flags(
        &|s| {
            s.cargo_config
                .push_str("\n[profile.bench]\nopt-level = 1\n")
        },
        "sets a cargo profile",
    );
    // A missing file fails closed.
    flags(
        &|s| s.gate_test = "<unreadable>".to_owned(),
        "does not test",
    );
    flags(
        &|s| s.budget = "<unreadable>".to_owned(),
        "does not declare",
    );
}
