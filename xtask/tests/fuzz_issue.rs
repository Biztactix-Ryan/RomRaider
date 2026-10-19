//! US-RR-5-6: `cargo xtask fuzz file-issues`, which turns a nightly run's
//! `fuzz-failure-<target>/` artifacts into GitHub issues labelled `fuzz`.
//!
//! * [`collect`] keeps only known targets, declared features and plainly
//!   named saved inputs, and does not file a failure without an input.
//! * One open issue per target and feature set: a new failure opens one
//!   (`gh issue create --label fuzz`), a repeat comments on the open issue
//!   with the same title (`gh issue comment <n>`).
//! * The body quotes the log in a fence the log cannot close, and the `gh`
//!   calls carry it by `--body-file`, never on a command line.
//! * `--dry-run` end to end on a planted-panic failure prints exactly the
//!   `gh` calls it would run, and runs none.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::fuzz::Limits;
use xtask::fuzz_issue::{
    ARTIFACT_PREFIX, Context, EXCERPT_FRAMES, FEATURES_FILE, Failure, Kind, LABEL, LOG_FILE,
    PREVIEW_BYTES, SavedInput, collect, existing_issue, fence_for, file_call, hex, is_plain_name,
    issue_body, issue_title, label_call, list_call, log_excerpt, plan, shell_quote, strip_ansi,
};

const PANIC_LOG: &str = "INFO: Seed: 1\n\
#8\tINITED cov: 22 ft: 27 corp: 5/27b exec/s: 0 rss: 34Mb\n\
\n\
thread '<unnamed>' (1) panicked at src/lib.rs:110:9:\n\
planted panic (feature planted-panic)\n\
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\
==1== ERROR: libFuzzer: deadly signal\n\
    #0 0x1  (toy+0x1)\n\
    #1 0x2  (toy+0x2)\n\
    #2 0x3  (toy+0x3)\n\
    #3 0x4  (toy+0x4)\n\
    #4 0x5  (toy+0x5)\n\
    #5 0x6  (toy+0x6)\n\
    #6 0x7  (toy+0x7)\n\
    #7 0x8  (toy+0x8)\n\
    #8 0x9  (toy+0x9)\n\
    #9 0xa  (toy+0xa)\n\
\n\
SUMMARY: libFuzzer: deadly signal\n\
Base64: UEFOSUM=\n";

fn targets() -> Vec<String> {
    vec!["ecu_id".to_owned(), "toy".to_owned()]
}

fn features() -> Vec<String> {
    ["planted-panic", "planted-hang", "planted-oom"]
        .map(str::to_owned)
        .to_vec()
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fuzz-issue")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// Writes `fuzz-failure-<target>/` with `files` under `dir`.
fn failure_dir(dir: &Path, target: &str, files: &[(&str, &[u8])]) -> io::Result<()> {
    let d = dir.join(format!("{ARTIFACT_PREFIX}{target}"));
    fs::create_dir_all(&d)?;
    for (name, bytes) in files {
        fs::write(d.join(name), bytes)?;
    }
    Ok(())
}

fn toy_failure(features: &[&str]) -> Failure {
    Failure {
        target: "toy".to_owned(),
        features: features.iter().map(|&f| f.to_owned()).collect(),
        inputs: vec![SavedInput {
            name: "crash-575131b9".to_owned(),
            kind: Kind::Crash,
            len: 6,
            head: b"PANIC\\".to_vec(),
        }],
        log: PANIC_LOG.to_owned(),
    }
}

fn limits() -> Limits {
    Limits {
        timeout_secs: 10,
        rss_limit_mb: 2048,
        malloc_limit_mb: 512,
    }
}

#[test]
fn collect_reads_a_planted_panic_failure() {
    let dir = scratch("collect-ok");
    failure_dir(
        &dir,
        "toy",
        &[
            ("crash-575131b9", b"PANIC\\"),
            (LOG_FILE, PANIC_LOG.as_bytes()),
            (FEATURES_FILE, b"planted-panic\n"),
        ],
    )
    .unwrap();
    let c = collect(&dir, &targets(), &features()).unwrap();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    assert_eq!(c.failures.len(), 1);
    let f = &c.failures[0];
    assert_eq!(f.target, "toy");
    assert_eq!(f.features, ["planted-panic"]);
    assert_eq!(f.inputs.len(), 1);
    assert_eq!(f.inputs[0].kind, Kind::Crash);
    assert_eq!(f.inputs[0].len, 6);
    assert!(f.log.contains("planted panic"));
}

#[test]
fn collect_skips_what_it_cannot_trust() {
    let dir = scratch("collect-skip");
    // Not a known target.
    failure_dir(&dir, "evil$(id)", &[("crash-1", b"x")]).unwrap();
    // Not a failure directory at all.
    fs::create_dir_all(dir.join("something-else")).unwrap();
    // Undeclared feature (ignored, the failure is still filed).
    failure_dir(
        &dir,
        "ecu_id",
        &[
            ("crash-2", b"\x00"),
            ("crash-bad name`", b"\x01"),
            (FEATURES_FILE, b"planted-panic,not-a-feature"),
            ("notes.txt", b"ignored: not a libFuzzer file name"),
        ],
    )
    .unwrap();
    // Failed without saving an input (a build error): not filed.
    failure_dir(
        &dir,
        "toy",
        &[(LOG_FILE, b"error[E0425]: cannot find value")],
    )
    .unwrap();

    let c = collect(&dir, &targets(), &features()).unwrap();
    let w = c.warnings.join("\n");
    assert!(w.contains("is not a fuzz target"), "{w}");
    assert!(w.contains("not a fuzz-failure-<target> directory"), "{w}");
    assert!(
        w.contains("`not-a-feature` is not a fuzz-crate feature"),
        "{w}"
    );
    assert!(w.contains("unexpected name"), "{w}");
    assert!(w.contains("without saving an input"), "{w}");
    assert_eq!(c.failures.len(), 1, "{:?}", c.failures);
    let f = &c.failures[0];
    assert_eq!(f.target, "ecu_id");
    assert_eq!(f.features, ["planted-panic"]);
    let names: Vec<&str> = f.inputs.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["crash-2"]);
}

#[test]
fn kinds_follow_libfuzzer_file_names() {
    for (name, kind) in [
        ("crash-ab", Some(Kind::Crash)),
        ("timeout-ab", Some(Kind::Timeout)),
        ("oom-ab", Some(Kind::OutOfMemory)),
        ("leak-ab", Some(Kind::Leak)),
        ("slow-unit-ab", Some(Kind::SlowUnit)),
        ("fuzz.log", None),
        ("ab", None),
    ] {
        assert_eq!(Kind::from_file_name(name), kind, "{name}");
    }
    assert!(is_plain_name("crash-0a1b.c_d"));
    for bad in ["", ".hidden", "a b", "a/b", "a`b", "$(x)", "é"] {
        assert!(!is_plain_name(bad), "{bad:?}");
    }
}

#[test]
fn one_title_per_target_and_feature_set() {
    let none = issue_title("toy", &[]);
    let planted = issue_title("toy", &["planted-panic".to_owned()]);
    assert_eq!(none, "fuzz: target `toy` found a failing input");
    assert_ne!(none, planted, "a self-test must not land on a real issue");
    assert!(planted.contains("planted-panic"));
    assert_ne!(none, issue_title("ecu_id", &[]));
}

#[test]
fn existing_issue_matches_the_exact_title_only() {
    let t = issue_title("toy", &[]);
    let json = format!(
        r#"[{{"number":12,"title":"{t} (old)"}},{{"number":9,"title":"{t}"}},{{"number":7,"title":"{t}"}},{{"number":3,"title":"other"}}]"#,
    );
    assert_eq!(existing_issue(&json, &t).unwrap(), Some(7));
    assert_eq!(existing_issue("[]", &t).unwrap(), None);
    assert_eq!(
        existing_issue(
            r#"[{"number":1,"title":"fuzz: target `ecu_id` found a failing input"}]"#,
            &t
        )
        .unwrap(),
        None
    );
    assert!(existing_issue("not json", &t).is_err());
    assert!(existing_issue(r#"{"number":1}"#, &t).is_err());
    assert!(existing_issue(r#"[{"title":"x"}]"#, &t).is_err());
}

#[test]
fn plan_opens_a_new_issue_or_comments_on_the_open_one() {
    let failures = [toy_failure(&[])];
    let ctx = Context {
        run_url: Some("https://github.com/o/r/actions/runs/42".to_owned()),
        limits: Some(limits()),
    };
    let body_dir = Path::new("bodies");
    let title = issue_title("toy", &[]);

    let new = plan(&failures, &ctx, "[]", body_dir).unwrap();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].existing, None);
    assert_eq!(new[0].body_file, body_dir.join("toy.md"));
    assert_eq!(
        new[0].call.args,
        [
            "issue",
            "create",
            "--title",
            title.as_str(),
            "--label",
            LABEL,
            "--body-file",
            "bodies/toy.md"
        ]
    );
    assert!(new[0].body.contains("### Reproduce and fix"));

    let open = format!(r#"[{{"number":5,"title":"{title}"}}]"#);
    let again = plan(&failures, &ctx, &open, body_dir).unwrap();
    assert_eq!(again[0].existing, Some(5));
    assert_eq!(
        again[0].call.args,
        ["issue", "comment", "5", "--body-file", "bodies/toy.md"]
    );
    assert!(again[0].body.contains("failing again"));
    assert!(!again[0].body.contains("### Reproduce and fix"));
}

#[test]
fn gh_calls_list_label_and_never_carry_the_body() {
    assert_eq!(
        list_call().args,
        [
            "issue",
            "list",
            "--label",
            "fuzz",
            "--state",
            "open",
            "--limit",
            "200",
            "--json",
            "number,title"
        ]
    );
    let label = label_call();
    assert_eq!(&label.args[..3], ["label", "create", "fuzz"]);
    assert!(label.args.iter().any(|a| a == "--force"));
    let call = file_call("t", None, Path::new("b.md"));
    assert!(!call.args.iter().any(|a| a == "--body" || a == "-b"));
    assert_eq!(
        file_call("fuzz: target `toy` x", None, Path::new("b.md")).to_string(),
        "gh issue create --title 'fuzz: target `toy` x' --label fuzz --body-file b.md"
    );
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
    assert_eq!(shell_quote(""), "''");
    assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
}

#[test]
fn body_reports_the_failure_and_how_to_reproduce_it() {
    let ctx = Context {
        run_url: Some("https://github.com/o/r/actions/runs/42".to_owned()),
        limits: Some(limits()),
    };
    let b = issue_body(&toy_failure(&["planted-panic"]), &ctx, false);
    for want in [
        "fuzz target `toy`",
        "crash (panic",
        "https://github.com/o/r/actions/runs/42",
        "artifact `fuzz-failure-toy`",
        "-timeout=10 -rss_limit_mb=2048 -malloc_limit_mb=512",
        "deliberately planted bugs",
        "| `crash-575131b9` | crash | 6 B | `50 41 4e 49 43 5c` |",
        "planted panic (feature planted-panic)",
        "SUMMARY: libFuzzer: deadly signal",
        "cargo xtask fuzz run toy --features planted-panic --input crash-575131b9",
        "cargo +nightly fuzz tmin --fuzz-dir fuzz toy crash-575131b9",
        "fuzz/regressions/toy/",
        "job `fuzz-regressions`",
        "`cargo xtask fuzz replay --features planted-panic` must fail on it",
    ] {
        assert!(b.contains(want), "body lacks {want:?}:\n{b}");
    }
    let real = issue_body(&toy_failure(&[]), &ctx, false);
    assert!(!real.contains("deliberately planted"), "{real}");
    assert!(real.contains("cargo xtask fuzz run toy --input crash-575131b9"));
    assert!(
        real.contains("`cargo xtask fuzz replay` must fail on it"),
        "{real}"
    );
}

#[test]
fn log_cannot_escape_its_fence_or_flood_the_body() {
    assert_eq!(fence_for("plain"), "```");
    assert_eq!(fence_for("a ``` b"), "````");
    assert_eq!(fence_for("`````"), "``````");
    let mut f = toy_failure(&[]);
    f.log = "panicked at x:\n```\n@everyone <script>\n````\n".to_owned();
    let b = issue_body(&f, &Context::default(), false);
    assert!(b.contains("`````text\n"), "{b}");

    // Frames are capped and the rest elided.
    let e = log_excerpt(PANIC_LOG);
    assert!(e.starts_with("thread '<unnamed>' (1) panicked at"), "{e}");
    let frames = e
        .lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .count();
    assert_eq!(frames, EXCERPT_FRAMES, "{e}");
    assert!(e.contains("more frames in the run log"));
    assert!(e.contains("SUMMARY: libFuzzer: deadly signal"));

    // ANSI colour and control bytes are dropped.
    assert_eq!(
        strip_ansi("\u{1b}[1m\u{1b}[31merror\u{1b}[0m\u{7}!"),
        "error!"
    );
    // A huge log is cut.
    let huge = format!("panicked at x\n{}", "y".repeat(100_000));
    assert!(log_excerpt(&huge).len() < 25_000);
    // No panic marker: the tail.
    let tail = log_excerpt(&(0..500).map(|i| format!("line {i}\n")).collect::<String>());
    assert!(tail.ends_with("line 499"), "{tail}");

    assert_eq!(hex(&[0, 0xab, 0x10]), "00 ab 10");
    let mut long = toy_failure(&[]);
    long.inputs[0].len = 1000;
    long.inputs[0].head = vec![0x41; PREVIEW_BYTES];
    assert!(issue_body(&long, &Context::default(), false).contains("41 ...` |"));
}

/// End to end, offline: the planted-panic failure as fuzz.yml uploads it,
/// through the real binary with `--dry-run`.
#[test]
fn dry_run_prints_the_gh_calls_and_runs_none() {
    let dir = scratch("dry-run");
    let failures = dir.join("fuzz-failures");
    failure_dir(
        &failures,
        "toy",
        &[
            ("crash-575131b9", b"PANIC\\"),
            (LOG_FILE, PANIC_LOG.as_bytes()),
            (FEATURES_FILE, b"planted-panic\n"),
        ],
    )
    .unwrap();
    let bodies = dir.join("bodies");
    let run = |extra: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["fuzz", "file-issues", "--dir"])
            .arg(&failures)
            .args([
                "--run-url",
                "https://github.com/o/r/actions/runs/42",
                "--dry-run",
            ])
            .arg("--body-dir")
            .arg(&bodies)
            .args(extra)
            // A `gh` that would be found on PATH must not be run.
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let title = issue_title("toy", &["planted-panic".to_owned()]);
    let body_file = bodies.join("toy.md");
    let out = run(&[]);
    let calls: Vec<&str> = out.lines().filter(|l| l.starts_with("+ gh ")).collect();
    assert_eq!(
        calls,
        [
            "+ gh issue list --label fuzz --state open --limit 200 --json number,title".to_owned(),
            format!("+ {}", label_call()),
            format!(
                "+ gh issue create --title {} --label fuzz --body-file {}",
                shell_quote(&title),
                shell_quote(&body_file.to_string_lossy())
            ),
        ]
    );
    let body = fs::read_to_string(&body_file).unwrap();
    assert!(body.contains("planted panic (feature planted-panic)"));
    assert!(out.contains("would file 1 target(s)"));

    // An open issue with the same title: comment on it instead.
    let open = dir.join("open.json");
    fs::write(&open, format!(r#"[{{"number":17,"title":"{title}"}}]"#)).unwrap();
    let out = run(&["--open-issues", open.to_str().unwrap()]);
    assert!(
        out.lines().any(|l| l
            == format!(
                "+ gh issue comment 17 --body-file {}",
                shell_quote(&body_file.to_string_lossy())
            )),
        "{out}"
    );
    assert!(!out.contains("gh issue create"), "{out}");
}

#[test]
fn nothing_to_file_is_not_an_error() {
    let dir = scratch("nothing");
    failure_dir(&dir, "toy", &[(LOG_FILE, b"error: could not compile")]).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["fuzz", "file-issues", "--dry-run", "--dir"])
        .arg(&dir)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(o.status.success());
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("nothing to file"), "{out}");
    assert!(!out.contains("+ gh"), "{out}");
}
