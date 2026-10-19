//! Mutation scope of a pull request (`cargo xtask mutants-scope --diff
//! <file>`, story US-RR-4, task US-RR-4-6; PROJECT.md Testing Strategy T7).
//!
//! On a pull request, `.github/workflows/mutants.yml` runs
//! `cargo mutants --in-diff pr.diff`: only mutants in gated code the PR
//! touches. That catches new or changed logic that no test checks, but not a
//! weakened test: weakening an assertion, deleting a `#[test]` or editing a
//! known-answer fixture changes no gated code, so `--in-diff` finds no mutant
//! at all and passes. This module reads the PR's unified diff (`git diff
//! <base>...HEAD`) and answers [`Scope::Full`] (mutate every gated module, as
//! the weekly run does) when the diff touches test code that can kill gated
//! mutants, else [`Scope::Diff`].
//!
//! Test code, for this purpose (each a [`Scope::Full`] reason):
//!
//! * any file under `crates/<gated>/tests/` (integration tests and their
//!   data), for each crate in [`GATED_CRATES`];
//! * any file under `crates/rr-testkit/` (shared test support: fixtures
//!   loader, KAT runner, strategies) and under `fixtures/` (KAT files whose
//!   expected values the tests assert);
//! * the mutation configuration and its CI wiring: `.cargo/mutants.toml` (a
//!   new exclusion or a changed glob changes what the gate mutates),
//!   `.github/workflows/mutants.yml` and this module;
//! * in a `.rs` file under `crates/<gated>/src/`: a file named `tests.rs` or
//!   under a `tests/` directory (an out-of-line test module), or a changed
//!   line in the file's test region, or a removed line that is test code.
//!
//! A source file's test region runs from its first line that is a
//! `#[cfg(...)]` attribute mentioning `test` (normally `#[cfg(test)] mod
//! tests`, at the end of the file by convention) or a `#[test]` attribute, to
//! the end of the file, in the file as it is after the change. An added line
//! is in it if its new line number is; a removed line is placed where it
//! stood in the new file (before the next new line). Removed lines are also
//! checked for content, so deleting a whole test module, which leaves no test
//! region in the new file, is still seen: a removed `#[test]`, `#[cfg(..test..)]`
//! or `assert` line is test code. Both rules err towards [`Scope::Full`]: a
//! test-only helper marked `#[cfg(test)]` mid-file widens the region, which
//! costs a longer run, never a missed weakening.
//!
//! Only the gated crates matter: cargo-mutants runs the tests of the mutated
//! package (`test_workspace` is off), so tests in other crates cannot kill a
//! gated mutant.

use std::fmt;

/// The crates whose modules `.cargo/mutants.toml` gates.
pub const GATED_CRATES: &[&str] = &["rr-core", "rr-rom", "rr-comms"];

/// Files whose change always widens the run to every gated module.
pub const CONFIG_FILES: &[&str] = &[
    ".cargo/mutants.toml",
    ".github/workflows/mutants.yml",
    "xtask/src/mutants_scope.rs",
];

/// Directories (with trailing `/`) of shared test support and fixtures.
pub const TEST_SUPPORT_DIRS: &[&str] = &["crates/rr-testkit/", "fixtures/"];

/// What a pull request's mutation run covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// `cargo mutants --in-diff <diff>`: only mutants in changed gated code.
    Diff,
    /// `cargo mutants`: every gated module, for the given reasons.
    Full(Vec<String>),
}

impl Scope {
    /// The word the workflow branches on: `diff` or `full`.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            Self::Diff => "diff",
            Self::Full(_) => "full",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// One changed line of a file in the diff, placed in the new file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    /// 1-based line in the new file: the added line itself, or the new line
    /// a removed line stood before.
    new_line: usize,
    removed: bool,
    text: String,
}

/// One file in the diff.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct FileDiff {
    /// Paths named by the `diff --git a/<old> b/<new>` header (both sides of
    /// a rename; one when they are the same).
    paths: Vec<String>,
    /// The path after the change; `None` when the file was deleted.
    new_path: Option<String>,
    changes: Vec<Change>,
}

/// Strips a `a/` / `b/` prefix and git's quoting.
fn diff_path(raw: &str) -> String {
    let raw = raw.trim().trim_matches('"');
    raw.strip_prefix("a/")
        .or_else(|| raw.strip_prefix("b/"))
        .unwrap_or(raw)
        .to_owned()
}

/// `@@ -a[,b] +c[,d] @@` → the first new line number of the hunk.
fn hunk_new_start(line: &str) -> Option<usize> {
    let plus = line.split(' ').find(|w| w.starts_with('+'))?;
    let mut it = plus[1..].split(',');
    let start: usize = it.next()?.parse().ok()?;
    let count: usize = it.next().map_or(Some(1), |c| c.parse().ok())?;
    // An empty new side (`+c,0`) names the line *before* the hunk.
    Some(if count == 0 { start + 1 } else { start })
}

/// Parses a unified `git diff` into per-file changes.
fn parse(diff: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cursor: Option<usize> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            cursor = None;
            let mut f = FileDiff::default();
            // `a/<old> b/<new>`; paths with spaces are rare and fall back to
            // the `---`/`+++` lines below.
            if let Some((old, new)) = rest.split_once(" b/") {
                f.paths.push(diff_path(old));
                let new = new.to_owned();
                if !f.paths.contains(&new) {
                    f.paths.push(new.clone());
                }
                f.new_path = Some(new);
            }
            files.push(f);
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if cursor.is_none() {
            if let Some(p) = line.strip_prefix("--- ") {
                if p.trim() != "/dev/null" {
                    let p = diff_path(p);
                    if !f.paths.contains(&p) {
                        f.paths.push(p);
                    }
                }
                continue;
            }
            if let Some(p) = line.strip_prefix("+++ ") {
                if p.trim() == "/dev/null" {
                    f.new_path = None;
                } else {
                    let p = diff_path(p);
                    if !f.paths.contains(&p) {
                        f.paths.push(p.clone());
                    }
                    f.new_path = Some(p);
                }
                continue;
            }
            if line.starts_with("deleted file mode") {
                f.new_path = None;
            }
            if let Some(p) = line
                .strip_prefix("rename to ")
                .or_else(|| line.strip_prefix("rename from "))
            {
                let p = p.trim().to_owned();
                if !f.paths.contains(&p) {
                    f.paths.push(p);
                }
            }
        }
        if line.starts_with("@@") {
            cursor = hunk_new_start(line);
            continue;
        }
        let Some(c) = cursor.as_mut() else { continue };
        if let Some(text) = line.strip_prefix('+') {
            f.changes.push(Change {
                new_line: *c,
                removed: false,
                text: text.to_owned(),
            });
            *c += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            f.changes.push(Change {
                new_line: *c,
                removed: true,
                text: text.to_owned(),
            });
        } else if line.starts_with(' ') || line.is_empty() {
            *c += 1;
        }
        // `\ No newline at end of file` and anything else: no line.
    }
    files
}

/// A `#[cfg(...)]` mentioning `test`, or `#[test]`.
fn is_test_attr(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("#[test]") || (t.starts_with("#[cfg(") && t.contains("test"))
}

/// A removed line that is test code wherever it stood.
fn is_test_line(line: &str) -> bool {
    is_test_attr(line) || line.contains("assert")
}

/// First 1-based line of `source`'s test region, if it has one.
fn test_region_start(source: &str) -> Option<usize> {
    source.lines().position(is_test_attr).map(|i| i + 1)
}

/// The gated crate whose `crates/<name>/<sub>` holds `path`, and the rest.
fn in_gated_crate<'a>(path: &'a str, sub: &str) -> Option<(&'static str, &'a str)> {
    GATED_CRATES.iter().find_map(|&c| {
        path.strip_prefix("crates/")?
            .strip_prefix(c)?
            .strip_prefix('/')?
            .strip_prefix(sub)?
            .strip_prefix('/')
            .map(|rest| (c, rest))
    })
}

/// The scope of a pull request whose `git diff <base>...HEAD` is `diff`.
/// `new_source(path)` returns a changed file's content after the change
/// (`None` if unreadable, which is treated as all test code: fail closed).
pub fn scope(diff: &str, new_source: impl Fn(&str) -> Option<String>) -> Scope {
    let mut reasons = Vec::new();
    for f in parse(diff) {
        let mut why = |r: String| {
            if !reasons.contains(&r) {
                reasons.push(r);
            }
        };
        for p in &f.paths {
            if CONFIG_FILES.contains(&p.as_str()) {
                why(format!("{p}: mutation gate configuration changed"));
            } else if TEST_SUPPORT_DIRS.iter().any(|d| p.starts_with(d)) {
                why(format!("{p}: shared test support or fixture changed"));
            } else if let Some((c, _)) = in_gated_crate(p, "tests") {
                why(format!("{p}: integration test of {c} changed"));
            } else if let Some((c, rest)) = in_gated_crate(p, "src") {
                let file = rest.rsplit('/').next().unwrap_or(rest);
                if rest.split('/').any(|s| s == "tests") || file == "tests.rs" {
                    why(format!("{p}: test module of {c} changed"));
                }
            }
        }
        let Some(new) = f.new_path.as_deref() else {
            // Deleted: what it tested is gone too, or will be caught by the
            // removed-line check of the file that keeps the code.
            let old = f.paths.first().map_or("", String::as_str);
            if in_gated_crate(old, "src").is_some()
                && let Some(ch) = f.changes.iter().find(|c| is_test_line(&c.text))
            {
                why(format!("{old}: removed test code `{}`", ch.text.trim()));
            }
            continue;
        };
        if in_gated_crate(new, "src").is_none() || !new.ends_with(".rs") {
            continue;
        }
        let region = match new_source(new) {
            Some(src) => test_region_start(&src),
            None => {
                why(format!("{new}: could not read the changed file"));
                continue;
            }
        };
        let in_region = |line: usize| region.is_some_and(|r| line >= r);
        if let Some(ch) = f.changes.iter().find(|c| in_region(c.new_line)) {
            why(format!(
                "{new}:{}: test code changed (test region starts at line {})",
                ch.new_line,
                region.unwrap_or_default()
            ));
        } else if let Some(ch) = f
            .changes
            .iter()
            .find(|c| c.removed && is_test_line(&c.text))
        {
            why(format!("{new}: removed test code `{}`", ch.text.trim()));
        }
    }
    if reasons.is_empty() {
        Scope::Diff
    } else {
        Scope::Full(reasons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
pub fn is_error(n: u32) -> bool {
    n > 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn errors() {
        assert!(super::is_error(1));
        assert!(!super::is_error(0));
    }
}
";

    fn src(_: &str) -> Option<String> {
        Some(SRC.to_owned())
    }

    fn file_diff(path: &str, hunks: &str) -> String {
        format!(
            "diff --git a/{path} b/{path}\nindex 1..2 100644\n--- a/{path}\n+++ b/{path}\n{hunks}"
        )
    }

    fn full_reason(s: &Scope, needle: &str) {
        match s {
            Scope::Full(r) => assert!(
                r.iter().any(|r| r.contains(needle)),
                "no reason mentioning {needle:?}: {r:#?}"
            ),
            Scope::Diff => panic!("expected full, got diff"),
        }
    }

    #[test]
    fn source_change_outside_tests_is_diff() {
        let d = file_diff(
            "crates/rr-core/src/diag.rs",
            "@@ -1,3 +1,3 @@\n pub fn is_error(n: u32) -> bool {\n-    n != 0\n+    n > 0\n }\n",
        );
        assert_eq!(scope(&d, src), Scope::Diff);
        assert_eq!(scope("", src), Scope::Diff);
    }

    #[test]
    fn non_gated_changes_are_diff() {
        let d = format!(
            "{}{}{}",
            file_diff("README.md", "@@ -1 +1 @@\n-a\n+b\n"),
            file_diff(
                "crates/rr-cli/tests/cli.rs",
                "@@ -1 +1 @@\n-assert!(x)\n+\n"
            ),
            file_diff("crates/rr-core/Cargo.toml", "@@ -1 +1 @@\n-a\n+b\n"),
        );
        assert_eq!(scope(&d, src), Scope::Diff);
    }

    #[test]
    fn weakened_inline_assert_is_full() {
        let d = file_diff(
            "crates/rr-core/src/diag.rs",
            "@@ -9,3 +9,2 @@\n         assert!(super::is_error(1));\n-        assert!(!super::is_error(0));\n     }\n",
        );
        full_reason(&scope(&d, src), "diag.rs:10: test code changed");
    }

    #[test]
    fn changed_expected_value_in_tests_is_full() {
        let d = file_diff(
            "crates/rr-rom/src/journal.rs",
            "@@ -10,1 +10,1 @@\n-        assert!(!super::is_error(0));\n+        let _ = super::is_error(0);\n",
        );
        full_reason(&scope(&d, src), "journal.rs:10");
    }

    #[test]
    fn deleted_test_module_is_full() {
        // The new file has no test region; the removed lines are test code.
        let no_tests = |_: &str| Some("pub fn f() {}\n".to_owned());
        let d = file_diff(
            "crates/rr-comms/src/engine.rs",
            "@@ -1,5 +1,1 @@\n pub fn f() {}\n-#[cfg(test)]\n-mod tests {\n-    #[test] fn t() {}\n-}\n",
        );
        full_reason(&scope(&d, no_tests), "removed test code `#[cfg(test)]`");
    }

    #[test]
    fn pure_deletion_hunk_is_placed_after_its_line() {
        // `+8,0`: lines removed after new line 8, i.e. before new line 9,
        // which is inside the region (line 5 onwards).
        let d = file_diff(
            "crates/rr-core/src/diag.rs",
            "@@ -9 +8,0 @@\n-        let x = 1;\n",
        );
        full_reason(&scope(&d, src), "diag.rs:9");
        // `+3,0`: before new line 4, outside the region.
        let d = file_diff("crates/rr-core/src/diag.rs", "@@ -4 +3,0 @@\n-// gone\n");
        assert_eq!(scope(&d, src), Scope::Diff);
    }

    #[test]
    fn integration_tests_and_test_modules_are_full() {
        for (path, needle) in [
            ("crates/rr-core/tests/kat.rs", "integration test of rr-core"),
            (
                "crates/rr-rom/tests/data/a.toml",
                "integration test of rr-rom",
            ),
            (
                "crates/rr-comms/src/engine/tests.rs",
                "test module of rr-comms",
            ),
            (
                "crates/rr-core/src/layer/tests/merge.rs",
                "test module of rr-core",
            ),
            ("crates/rr-testkit/src/kat.rs", "shared test support"),
            ("fixtures/kat/encoding/u16.toml", "fixture changed"),
            (".cargo/mutants.toml", "configuration changed"),
            (".github/workflows/mutants.yml", "configuration changed"),
        ] {
            let d = file_diff(path, "@@ -1 +1 @@\n-a\n+b\n");
            full_reason(&scope(&d, src), needle);
        }
    }

    #[test]
    fn renamed_away_and_deleted_files_count() {
        let d = "diff --git a/crates/rr-core/tests/a.rs b/crates/other/a.rs\n\
                 similarity index 100%\nrename from crates/rr-core/tests/a.rs\n\
                 rename to crates/other/a.rs\n";
        full_reason(&scope(d, src), "crates/rr-core/tests/a.rs");
        let d = "diff --git a/crates/rr-core/src/diag.rs b/crates/rr-core/src/diag.rs\n\
                 deleted file mode 100644\n--- a/crates/rr-core/src/diag.rs\n+++ /dev/null\n\
                 @@ -1,2 +0,0 @@\n-#[test]\n-fn t() {}\n";
        full_reason(&scope(d, src), "removed test code");
    }

    #[test]
    fn unreadable_changed_file_fails_closed() {
        let d = file_diff("crates/rr-core/src/diag.rs", "@@ -1 +1 @@\n-a\n+b\n");
        full_reason(&scope(&d, |_| None), "could not read");
    }

    #[test]
    fn mid_file_cfg_test_widens_the_region() {
        let s = "fn a() {}\n#[cfg(all(test, feature = \"x\"))]\nfn helper() {}\nfn b() {}\n";
        assert_eq!(test_region_start(s), Some(2));
        assert_eq!(test_region_start("fn a() {}\n"), None);
    }

    #[test]
    fn hunk_header_parsing() {
        assert_eq!(hunk_new_start("@@ -1,3 +4,5 @@ fn x"), Some(4));
        assert_eq!(hunk_new_start("@@ -1 +4 @@"), Some(4));
        assert_eq!(hunk_new_start("@@ -1,3 +4,0 @@"), Some(5));
        assert_eq!(hunk_new_start("@@ garbage @@"), None);
        assert_eq!(Scope::Diff.to_string(), "diff");
        assert_eq!(Scope::Full(vec![]).word(), "full");
    }
}
