//! US-RR-8-2: CODEOWNERS requires a second approval for changes under the
//! gated safety modules, and branch protection enforces it.
//!
//! * The real-tree tests run [`codeowners::check_tree`] in CI (the nextest
//!   jobs): every gated path from [`codeowners::required_paths`] has a code
//!   owner, every rule has valid owners, CODEOWNERS owns itself, the
//!   workflows and this check, and the CONTRIBUTING.md branch-protection
//!   payload requires code-owner review, an approval, approval of the last
//!   push and the `ci-ok` check.
//! * The `*_rejects_*` / `*_catches_*` tests prove each assertion fails on a
//!   mutated copy of the real file (or a planted file in a scratch tree).
//! * The live state on GitHub is compared by `cargo xtask
//!   check-branch-protection`, which needs the network and so never runs
//!   here; its comparison logic is tested below with fixture JSON shaped like
//!   GitHub's responses and a fake [`Api`].
//!
//! Files are read with line endings normalised to `\n` (Windows CI checks out
//! CRLF), so the `\n`-based mutations also apply there.

#[path = "common/workflow.rs"]
mod workflow;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use xtask::codeowners::{
    self, Api, CODEOWNERS, CONTRIBUTING, Protection, Required, check_live, compare,
    contributing_payload, gated_list_problems, matches, ownership_problems, policy_problems,
    required_paths,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

// Helpers marked `cfg(test)` may unwrap (clippy.toml allow-unwrap-in-tests).
#[cfg(test)]
fn read(rel: &str) -> String {
    codeowners::read_text(&root().join(rel)).unwrap()
}

#[cfg(test)]
fn required() -> Vec<Required> {
    required_paths(&root()).unwrap()
}

/// `text` with exactly one occurrence of `from` replaced by `to`.
fn mutate(text: &str, from: &str, to: &str) -> String {
    assert_eq!(
        text.matches(from).count(),
        1,
        "mutation anchor {from:?} must occur once"
    );
    text.replacen(from, to, 1)
}

fn assert_caught(problems: &[String], want: &str) {
    assert!(
        problems.iter().any(|p| p.contains(want)),
        "expected a problem containing {want:?}, got:\n{}",
        problems.join("\n")
    );
}

// --- The real tree ---------------------------------------------------------

/// Proves the criterion on the committed files: every gated path is owned,
/// the rules are valid, there is no shadowing CODEOWNERS, CONTRIBUTING.md's
/// gated list matches the checked modules, and the documented protection
/// payload makes a code-owner approval (by someone other than the author)
/// required.
#[test]
fn codeowners_owns_every_gated_path_and_payload_enforces_review() {
    let report = codeowners::check_tree(&root()).unwrap();
    assert!(report.problems.is_empty(), "{}", report.problems.join("\n"));
    assert!(report.rules > 0);
}

/// The gated list is not vacuous: it contains the files the story names,
/// found in the tree rather than only listed.
#[test]
fn required_paths_include_the_gated_files_in_the_tree() {
    let paths: Vec<String> = required().into_iter().map(|r| r.path).collect();
    for want in [
        // derived: defines RomWrite / EcuMemoryWrite / EcuReset
        "crates/rr-core/src/capability.rs",
        // derived: T6 suites and fixtures
        "crates/rr-core/tests/t6_token_compile_fail.rs",
        "crates/rr-core/tests/ui/always/construct_rom_write.rs",
        "crates/rr-core/tests/ui/sealed/mint_without_authority.stderr",
        "xtask/tests/t6_write_gating.rs",
        "xtask/tests/t6_ci_config.rs",
        // derived: manifests that could enable confirmation-authority
        "crates/rr-core/Cargo.toml",
        "crates/rr-app/Cargo.toml",
        "crates/rr-cli/Cargo.toml",
        // derived: workflows and the marker crate
        ".github/workflows/ci.yml",
        "crates/rr-write-marker/src/lib.rs",
        // modules owned before they exist
        "crates/rr-core/src/encoding.rs",
        "crates/rr-core/src/expr/mod.rs",
        "crates/rr-rom/src/journal.rs",
        "crates/rr-rom/src/checksum/mod.rs",
        "crates/rr-rom/src/save.rs",
        "crates/rr-rom/src/table/write.rs",
        "crates/rr-comms/src/write.rs",
        "crates/rr-comms/src/gate.rs",
        // enforcement machinery and CODEOWNERS itself
        "Cargo.toml",
        "clippy.toml",
        "deny.toml",
        "xtask/src/writes.rs",
        CODEOWNERS,
        "xtask/src/codeowners.rs",
        "xtask/tests/codeowners.rs",
    ] {
        assert!(
            paths.iter().any(|p| p == want),
            "{want} is not a required path"
        );
    }
}

/// The CONTRIBUTING.md command protects both branches of the fork and
/// requires the CI aggregate job that `ci.yml` defines.
#[test]
fn contributing_payload_targets_the_protected_branches() {
    let p = contributing_payload(&read(CONTRIBUTING)).unwrap();
    assert_eq!(p.repo, "Biztactix-Ryan/RomRaider");
    assert_eq!(p.branches, ["master", "rust-rewrite"]);
    assert_eq!(codeowners::REQUIRED_CHECK, workflow::GATE_JOB);
    assert!(p.protection.contexts.contains(workflow::GATE_JOB));
    assert_eq!(p.protection.require_code_owner_reviews, Some(true));
    assert!(p.protection.required_approving_review_count >= Some(1));
    assert_eq!(p.protection.require_last_push_approval, Some(true));
}

/// `rust-rewrite` also requires the always-running gate jobs of
/// coverage.yml, mutants.yml and fixtures.yml (task US-RR-4-8), under the
/// `name:` GitHub reports each as; `master`, which has no Rust gates,
/// requires only `ci-ok`.
#[test]
fn rust_rewrite_requires_the_gate_aggregate_checks() {
    let p = contributing_payload(&read(CONTRIBUTING)).unwrap();
    let names: BTreeSet<String> = [
        ("coverage.yml", "gate"),
        ("mutants.yml", "mutants-ok"),
        ("fixtures.yml", "fixtures-ok"),
    ]
    .iter()
    .map(|(file, job)| {
        let src = workflow::workflow_yml(file);
        let wf = workflow::Workflow::parse(&src);
        wf.job(job)
            .and_then(|j| j.value_at("name", 4))
            .unwrap_or_else(|| panic!("{file}: no job `{job}` with a `name:`"))
    })
    .collect();
    let want: BTreeSet<String> = ["coverage-gate", "mutants-ok", "fixtures-ok"]
        .map(str::to_owned)
        .into();
    assert_eq!(names, want);
    assert_eq!(p.extra_contexts.get("rust-rewrite"), Some(&want));
    assert_eq!(p.extra_contexts.get("master"), None);
    let mut rr = want.clone();
    rr.insert(workflow::GATE_JOB.to_owned());
    assert_eq!(p.expected("rust-rewrite").contexts, rr);
    assert_eq!(
        p.expected("master").contexts,
        BTreeSet::from([workflow::GATE_JOB.to_owned()])
    );
}

// --- Mutations: CODEOWNERS ---------------------------------------------------

/// Every rule is load-bearing: deleting any one leaves some gated path
/// without an owner. (So the required list covers everything CODEOWNERS
/// claims, and a rule can't be dropped unnoticed.)
#[test]
fn removing_any_rule_is_caught() {
    let text = read(CODEOWNERS);
    let required = required();
    let rules = codeowners::parse(&text);
    assert!(
        rules.len() >= 30,
        "expected the full rule set, got {}",
        rules.len()
    );
    for rule in &rules {
        let mutated: String = text
            .lines()
            .enumerate()
            .filter(|(i, _)| i + 1 != rule.line)
            .map(|(_, l)| format!("{l}\n"))
            .collect();
        let problems = ownership_problems(&mutated, &required);
        assert!(
            problems.iter().any(|p| p.contains("has no code owner")),
            "removing line {} `{}` was not caught",
            rule.line,
            rule.pattern
        );
    }
}

#[test]
fn ownerless_later_rule_is_caught() {
    let text = read(CODEOWNERS) + "/crates/rr-core/src/capability.rs\n";
    let problems = ownership_problems(&text, &required());
    assert_caught(
        &problems,
        "crates/rr-core/src/capability.rs (capability tokens",
    );
    assert_caught(&problems, "which has no owners");
    assert_caught(&problems, "no owners; this un-owns");
}

#[test]
fn invalid_owners_and_syntax_are_caught() {
    let text = read(CODEOWNERS);
    let anchor = "/deny.toml                                  @Biztactix-Ryan";
    for (bad, want) in [
        (
            "/deny.toml Biztactix-Ryan",
            "invalid owner `Biztactix-Ryan`",
        ),
        ("/deny.toml @-bad", "invalid owner `@-bad`"),
        ("/deny.toml @org/", "invalid owner `@org/`"),
        ("/deny.toml", "no owners"),
    ] {
        let problems = ownership_problems(&mutate(&text, anchor, bad), &required());
        assert_caught(&problems, want);
    }
    let problems = ownership_problems(&(text.clone() + "!/docs/ @Biztactix-Ryan\n"), &required());
    assert_caught(&problems, "negation");
    let problems = ownership_problems(&(text + "/src/[ab].rs @Biztactix-Ryan\n"), &required());
    assert_caught(&problems, "character ranges");
}

#[test]
fn codeowners_and_workflows_must_own_themselves() {
    let text = read(CODEOWNERS);
    let problems = ownership_problems(
        &mutate(&text, "/.github/CODEOWNERS ", "/.github/CODEOWNERS.bak "),
        &required(),
    );
    assert_caught(
        &problems,
        ".github/CODEOWNERS (CODEOWNERS itself) has no code owner",
    );
    let problems = ownership_problems(
        &mutate(&text, "/.github/workflows/ ", "/.github/workflow/ "),
        &required(),
    );
    assert_caught(
        &problems,
        ".github/workflows/ci.yml (CI workflow) has no code owner",
    );
}

#[test]
fn valid_owner_forms() {
    for ok in [
        "@Biztactix-Ryan",
        "@a",
        "@org/team_name-1",
        "dev@example.com",
    ] {
        assert!(codeowners::valid_owner(ok), "{ok}");
    }
    for bad in [
        "Biztactix-Ryan",
        "@",
        "@-x",
        "@x-",
        "@a--b",
        "@org/",
        "a@b",
        "@a b",
    ] {
        assert!(!codeowners::valid_owner(bad), "{bad}");
    }
}

// --- Mutations: the derived gated paths --------------------------------------

/// A scratch tree with the real CODEOWNERS and planted gated files: the
/// derivation finds them, and the check reports the ones CODEOWNERS misses.
#[test]
fn planted_gated_files_are_required_and_unowned_ones_caught() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codeowners-planted");
    let _ = fs::remove_dir_all(&dir);
    let plant = |rel: &str, body: &str| {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    };
    plant("crates/rr-new/Cargo.toml", "[package]\n");
    plant(
        "crates/rr-new/src/token.rs",
        "pub struct EcuReset {\n    _p: (),\n}\n",
    );
    plant(
        "crates/rr-new/src/not_token.rs",
        "pub struct EcuResetRequest;\n",
    );
    plant("crates/rr-new/tests/t6_new_suite/main.rs", "\n");
    plant("crates/rr-new/tests/ui/case.rs", "\n");
    plant("xtask/tests/t6_extra.rs", "\n");
    plant(".github/workflows/release.yml", "on: push\n");
    plant("crates/rr-write-marker/src/extra.rs", "\n");

    let required = required_paths(&dir).unwrap();
    let paths: Vec<&str> = required.iter().map(|r| r.path.as_str()).collect();
    for want in [
        "crates/rr-new/Cargo.toml",
        "crates/rr-new/src/token.rs",
        "crates/rr-new/tests/t6_new_suite/main.rs",
        "crates/rr-new/tests/ui/case.rs",
        "xtask/tests/t6_extra.rs",
        ".github/workflows/release.yml",
        "crates/rr-write-marker/src/extra.rs",
    ] {
        assert!(paths.contains(&want), "{want} not derived; got {paths:?}");
    }
    assert!(!paths.contains(&"crates/rr-new/src/not_token.rs"));

    let problems = ownership_problems(&read(CODEOWNERS), &required);
    // Token type in a file CODEOWNERS doesn't know about.
    assert_caught(
        &problems,
        "crates/rr-new/src/token.rs (defines a capability token type) has no code owner",
    );
    // `/crates/*/tests/t6_*` matches files only; a t6_ directory binary is missed.
    assert_caught(&problems, "crates/rr-new/tests/t6_new_suite/main.rs");
    // `/crates/rr-core/tests/ui/` covers rr-core only.
    assert_caught(&problems, "crates/rr-new/tests/ui/case.rs");
    // These are covered by existing rules.
    for owned in [
        "crates/rr-new/Cargo.toml",
        "xtask/tests/t6_extra.rs",
        ".github/workflows/release.yml",
        "crates/rr-write-marker/src/extra.rs",
    ] {
        assert!(
            !problems.iter().any(|p| p.starts_with(&format!("{owned} "))),
            "{owned} should be owned: {problems:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn shadowing_codeowners_file_is_caught() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codeowners-shadow");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".github")).unwrap();
    fs::write(dir.join(CODEOWNERS), read(CODEOWNERS)).unwrap();
    fs::write(dir.join(CONTRIBUTING), read(CONTRIBUTING)).unwrap();
    let clean = codeowners::check_tree(&dir).unwrap();
    assert!(
        !clean.problems.iter().any(|p| p.contains("is ignored")),
        "{:?}",
        clean.problems
    );
    fs::write(dir.join("CODEOWNERS"), "* @someone\n").unwrap();
    let report = codeowners::check_tree(&dir).unwrap();
    assert_caught(
        &report.problems,
        "CODEOWNERS exists; GitHub uses only .github/CODEOWNERS",
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn gated_list_drift_is_caught() {
    let text = read(CONTRIBUTING);
    assert_eq!(gated_list_problems(&text), Vec::<String>::new());
    let problems = gated_list_problems(&mutate(
        &text,
        "`rr-core::encoding`, `rr-core::expr`",
        "`rr-core::encoding`, `rr-core::expr`, `rr-core::units`",
    ));
    assert_caught(
        &problems,
        "`rr-core::units` is not in codeowners::GATED_MODULES",
    );
    let problems = gated_list_problems(&mutate(&text, ", `rr-core::expr`", ""));
    assert_caught(&problems, "doesn't name rr-core::expr");
    let problems = gated_list_problems(&mutate(&text, "save, journal, checksum", "save, checksum"));
    assert_caught(&problems, "doesn't name rr-rom::journal");
    // The tree check includes it.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codeowners-gated-list");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".github")).unwrap();
    fs::write(dir.join(CODEOWNERS), read(CODEOWNERS)).unwrap();
    fs::write(
        dir.join(CONTRIBUTING),
        mutate(&text, ", `rr-core::expr`", ""),
    )
    .unwrap();
    let report = codeowners::check_tree(&dir).unwrap();
    assert_caught(&report.problems, "doesn't name rr-core::expr");
    let _ = fs::remove_dir_all(&dir);
}

// --- Mutations: the CONTRIBUTING.md payload ----------------------------------

#[test]
fn payload_policy_rejects_each_weakening() {
    let text = read(CONTRIBUTING);
    let policy = |t: &str| policy_problems(&contributing_payload(t).unwrap().protection);
    assert_eq!(policy(&text), Vec::<String>::new());
    for (from, to, want) in [
        (
            "\"require_code_owner_reviews\": true",
            "\"require_code_owner_reviews\": false",
            "require_code_owner_reviews must be true",
        ),
        (
            "\"required_approving_review_count\": 1",
            "\"required_approving_review_count\": 0",
            "required_approving_review_count must be at least 1",
        ),
        (
            "\"require_last_push_approval\": true",
            "\"require_last_push_approval\": false",
            "require_last_push_approval must be true",
        ),
        (
            "\"dismiss_stale_reviews\": true",
            "\"dismiss_stale_reviews\": false",
            "dismiss_stale_reviews must be true",
        ),
        (
            "\"contexts\": [\"ci-ok\"]",
            "\"contexts\": [\"test\"]",
            "must include `ci-ok`",
        ),
        (
            "\"allow_force_pushes\": false",
            "\"allow_force_pushes\": true",
            "allow_force_pushes must be false",
        ),
        (
            "\"allow_deletions\": false",
            "\"allow_deletions\": true",
            "allow_deletions must be false",
        ),
    ] {
        assert_caught(&policy(&mutate(&text, from, to)), want);
    }
    // Reviews removed entirely.
    let start = text.find("  \"required_pull_request_reviews\"").unwrap();
    let end = start + text[start..].find("},\n").unwrap() + 3;
    let no_reviews = format!("{}{}", &text[..start], &text[end..]);
    assert_caught(
        &policy(&no_reviews),
        "require_code_owner_reviews must be true",
    );
    // The tree check reports a weakened payload too.
    let weakened = mutate(
        &text,
        "\"require_code_owner_reviews\": true",
        "\"require_code_owner_reviews\": false",
    );
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codeowners-payload");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".github")).unwrap();
    fs::write(dir.join(CODEOWNERS), read(CODEOWNERS)).unwrap();
    fs::write(dir.join(CONTRIBUTING), weakened).unwrap();
    let report = codeowners::check_tree(&dir).unwrap();
    assert_caught(
        &report.problems,
        "CONTRIBUTING.md payload: require_code_owner_reviews",
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn payload_parser_fails_closed() {
    let text = read(CONTRIBUTING);
    for (from, to, want) in [
        (
            "### Branch protection (maintainers)",
            "### Protection",
            "no `### Branch protection",
        ),
        (
            "for branch in master rust-rewrite; do",
            "for b in x; do",
            "no `for branch in",
        ),
        (
            "    -H \"Accept: application/vnd.github+json\" --input - <<'JSON'",
            "    -H \"Accept: application/vnd.github+json\" --input payload.json",
            "no `<<'JSON'` heredoc",
        ),
        (
            "gh api --method POST",
            "gh api --method DELETE",
            "is not `gh api --method POST`",
        ),
        (
            "{ \"contexts\": [\"coverage-gate\", \"mutants-ok\", \"fixtures-ok\"] }",
            "[\"coverage-gate\", \"mutants-ok\", \"fixtures-ok\"]",
            "has no `contexts` array",
        ),
        (
            "branches/rust-rewrite/protection/required_status_checks/contexts",
            "branches/main/protection/required_status_checks/contexts",
            "which the loop does not protect",
        ),
        (
            "  \"enforce_admins\": false,",
            "  \"enforce_admins\": false,,",
            "not valid JSON",
        ),
    ] {
        let err = contributing_payload(&mutate(&text, from, to)).unwrap_err();
        assert!(format!("{err:#}").contains(want), "{from:?}: {err:#}");
    }
}

// --- Pattern matching (GitHub CODEOWNERS docs examples) ----------------------

#[test]
fn pattern_matching_follows_github_semantics() {
    let yes = [
        ("*", "a/b/c.rs"),
        ("*.js", "src/app/x.js"),
        ("/build/logs/", "build/logs/a/b.log"),
        ("apps/", "x/apps/y.rs"),
        ("/docs/", "docs/a/b.md"),
        ("docs/*", "docs/getting-started.md"),
        ("**/logs", "deeply/nested/logs/x.txt"),
        ("/apps/github", "apps/github/x.rs"),
        ("/crates/*/Cargo.toml", "crates/rr-core/Cargo.toml"),
        (
            "/crates/rr-rom/src/**/*write*",
            "crates/rr-rom/src/write.rs",
        ),
        (
            "/crates/rr-rom/src/**/*write*",
            "crates/rr-rom/src/a/b/table_write.rs",
        ),
        ("/crates/*/tests/t6_*", "crates/rr-core/tests/t6_x.rs"),
        (
            "/crates/rr-core/src/capability/",
            "crates/rr-core/src/capability/mod.rs",
        ),
        ("/docs/**", "docs/a/b/c.md"),
    ];
    for (p, path) in yes {
        assert!(matches(p, path), "{p} should match {path}");
    }
    let no = [
        ("docs/*", "docs/build-app/troubleshooting.md"),
        ("/docs/", "x/docs/a.md"),
        ("/build/logs/", "build/logs"),
        ("/crates/*/Cargo.toml", "crates/a/b/Cargo.toml"),
        (
            "/crates/rr-rom/src/**/*write*",
            "crates/rr-comms/src/write.rs",
        ),
        (
            "/crates/rr-rom/src/**/*write*",
            "crates/rr-rom/src/write/mod.rs",
        ),
        (
            "/crates/rr-core/src/capability.rs",
            "crates/rr-core/src/capability_x.rs",
        ),
        (
            "/crates/rr-comms/src/**/gate*",
            "crates/rr-comms/src/gating.rs",
        ),
        ("*.js", "src/app/x.jsx"),
    ];
    for (p, path) in no {
        assert!(!matches(p, path), "{p} should not match {path}");
    }
}

#[test]
fn last_matching_rule_wins() {
    let rules = codeowners::parse("/a/ @x\n/a/b.rs @y # trailing comment\n/a/c.rs\n");
    assert_eq!(rules[1].owners, ["@y"]);
    let owner = |p| codeowners::deciding_rule(&rules, p).map(|r| r.owners.clone());
    assert_eq!(owner("a/z.rs"), Some(vec!["@x".to_owned()]));
    assert_eq!(owner("a/b.rs"), Some(vec!["@y".to_owned()]));
    assert_eq!(owner("a/c.rs"), Some(vec![]));
    assert_eq!(owner("b.rs"), None);
}

// --- Live comparison (fixture JSON, fake Api) --------------------------------

/// Trimmed GET `repos/Biztactix-Ryan/RomRaider/branches/rust-rewrite/protection`
/// response (2026-10-19).
const LIVE_PROTECTION: &str = r#"{
  "url": "https://api.github.com/repos/Biztactix-Ryan/RomRaider/branches/rust-rewrite/protection",
  "required_status_checks": {"strict": true, "contexts": ["ci-ok"], "checks": [{"context": "ci-ok", "app_id": 15368}]},
  "required_pull_request_reviews": {"dismiss_stale_reviews": true, "require_code_owner_reviews": true, "require_last_push_approval": true, "required_approving_review_count": 1},
  "required_signatures": {"enabled": false},
  "enforce_admins": {"enabled": false},
  "required_linear_history": {"enabled": false},
  "allow_force_pushes": {"enabled": false},
  "allow_deletions": {"enabled": false},
  "block_creations": {"enabled": false},
  "required_conversation_resolution": {"enabled": true},
  "lock_branch": {"enabled": false},
  "allow_fork_syncing": {"enabled": false}
}"#;

#[cfg(test)]
fn expected() -> Protection {
    contributing_payload(&read(CONTRIBUTING))
        .unwrap()
        .protection
}

#[cfg(test)]
fn live(json: &str) -> Protection {
    Protection::from_json(&serde_json::from_str(json).unwrap()).unwrap()
}

#[test]
fn live_response_matching_the_payload_passes() {
    assert_eq!(
        compare(&expected(), &live(LIVE_PROTECTION)),
        Vec::<String>::new()
    );
    assert_eq!(
        policy_problems(&live(LIVE_PROTECTION)),
        Vec::<String>::new()
    );
}

/// The live `rust-rewrite` protection must have the added contexts too, and
/// passes with them.
#[test]
fn live_rust_rewrite_needs_its_extra_contexts() {
    let p = contributing_payload(&read(CONTRIBUTING)).unwrap();
    let rr = p.expected("rust-rewrite");
    assert_caught(
        &compare(&rr, &live(LIVE_PROTECTION)),
        "required_status_checks.contexts",
    );
    let all = mutate(
        LIVE_PROTECTION,
        "\"contexts\": [\"ci-ok\"]",
        "\"contexts\": [\"ci-ok\", \"coverage-gate\", \"mutants-ok\", \"fixtures-ok\"]",
    );
    assert_eq!(compare(&rr, &live(&all)), Vec::<String>::new());
    assert_caught(
        &compare(&p.expected("master"), &live(&all)),
        "required_status_checks.contexts",
    );
}

#[test]
fn live_drift_is_caught() {
    for (from, to, want) in [
        (
            "\"require_code_owner_reviews\": true",
            "\"require_code_owner_reviews\": false",
            "require_code_owner_reviews: CONTRIBUTING.md sets true, GitHub has false",
        ),
        (
            "\"required_approving_review_count\": 1",
            "\"required_approving_review_count\": 2",
            "required_approving_review_count: CONTRIBUTING.md sets 1, GitHub has 2",
        ),
        (
            "\"contexts\": [\"ci-ok\"]",
            "\"contexts\": []",
            "required_status_checks.contexts",
        ),
        (
            "\"allow_force_pushes\": {\"enabled\": false}",
            "\"allow_force_pushes\": {\"enabled\": true}",
            "allow_force_pushes: CONTRIBUTING.md sets false, GitHub has true",
        ),
    ] {
        let drifted = mutate(LIVE_PROTECTION, from, to);
        assert_caught(&compare(&expected(), &live(&drifted)), want);
    }
    let no_reviews = live(&mutate(
        LIVE_PROTECTION,
        "\"required_pull_request_reviews\": {\"dismiss_stale_reviews\": true, \"require_code_owner_reviews\": true, \"require_last_push_approval\": true, \"required_approving_review_count\": 1},",
        "",
    ));
    assert_caught(&compare(&expected(), &no_reviews), "GitHub has unset");
    assert_caught(
        &policy_problems(&no_reviews),
        "require_code_owner_reviews must be true",
    );
}

/// Fake GitHub: path -> body (`None` = 404). Records the paths it was asked.
struct Fake {
    responses: HashMap<String, Option<String>>,
    asked: RefCell<Vec<String>>,
}

impl Api for Fake {
    fn get(&self, path: &str, _raw: bool) -> Result<Option<String>> {
        self.asked.borrow_mut().push(path.to_owned());
        Ok(self.responses.get(path).cloned().flatten())
    }
}

const REPO: &str = "Biztactix-Ryan/RomRaider";

fn fake(branch: &str, protection: Option<&str>, codeowners: Option<&str>, errors: &str) -> Fake {
    let mut responses = HashMap::new();
    responses.insert(
        format!("repos/{REPO}/branches/{branch}/protection"),
        protection.map(str::to_owned),
    );
    responses.insert(
        format!("repos/{REPO}/contents/.github/CODEOWNERS?ref={branch}"),
        codeowners.map(str::to_owned),
    );
    responses.insert(
        format!("repos/{REPO}/codeowners/errors?ref={branch}"),
        Some(errors.to_owned()),
    );
    Fake {
        responses,
        asked: RefCell::new(Vec::new()),
    }
}

#[cfg(test)]
fn live_problems(api: &Fake, branch: &str) -> Vec<String> {
    let local = read(CODEOWNERS);
    let reports = check_live(api, REPO, &[branch.to_owned()], &expected(), &local).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].branch, branch);
    reports[0].problems.clone()
}

#[test]
fn live_check_passes_when_enforced_as_documented() {
    // The remote copy is LF; a CRLF local checkout must still compare equal.
    let local = read(CODEOWNERS);
    let api = fake(
        "rust-rewrite",
        Some(LIVE_PROTECTION),
        Some(&local),
        r#"{"errors":[]}"#,
    );
    let crlf = local.replace('\n', "\r\n");
    let reports = check_live(&api, REPO, &["rust-rewrite".to_owned()], &expected(), &crlf).unwrap();
    assert_eq!(reports[0].problems, Vec::<String>::new());
    assert_eq!(api.asked.borrow().len(), 3);
}

#[test]
fn live_check_catches_unenforced_branches() {
    let local = read(CODEOWNERS);
    // Not protected.
    let api = fake("master", None, Some(&local), r#"{"errors":[]}"#);
    assert_caught(&live_problems(&api, "master"), "branch is not protected");
    // Protected, but no CODEOWNERS on the base branch.
    let api = fake("master", Some(LIVE_PROTECTION), None, r#"{"errors":[]}"#);
    assert_caught(
        &live_problems(&api, "master"),
        "no .github/CODEOWNERS on this branch",
    );
    // CODEOWNERS out of date.
    let stale = local.replace("/deny.toml", "/deny-old.toml");
    let api = fake(
        "master",
        Some(LIVE_PROTECTION),
        Some(&stale),
        r#"{"errors":[]}"#,
    );
    assert_caught(
        &live_problems(&api, "master"),
        "differs from the local file",
    );
    // GitHub can't resolve an owner.
    let api = fake(
        "master",
        Some(LIVE_PROTECTION),
        Some(&local),
        r#"{"errors":[{"line":3,"kind":"Unknown owner","message":"@ghost is not a user"}]}"#,
    );
    assert_caught(&live_problems(&api, "master"), "Unknown owner");
    // Weakened protection.
    let weak = mutate(
        LIVE_PROTECTION,
        "\"require_last_push_approval\": true",
        "\"require_last_push_approval\": false",
    );
    let api = fake("master", Some(&weak), Some(&local), r#"{"errors":[]}"#);
    assert_caught(
        &live_problems(&api, "master"),
        "require_last_push_approval must be true",
    );
    // Drift that still passes the policy (stricter, but not as documented).
    let drifted = mutate(
        LIVE_PROTECTION,
        "\"required_approving_review_count\": 1",
        "\"required_approving_review_count\": 2",
    );
    let api = fake("master", Some(&drifted), Some(&local), r#"{"errors":[]}"#);
    let problems = live_problems(&api, "master");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_caught(
        &problems,
        "required_approving_review_count: CONTRIBUTING.md sets 1",
    );
}
