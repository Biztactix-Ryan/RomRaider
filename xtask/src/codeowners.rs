//! CODEOWNERS and branch protection for the safety-gated code (US-RR-8-2).
//!
//! Story US-RR-8 requires that a change under the gated safety modules needs
//! a second approval, and that branch protection enforces it. GitHub has no
//! per-path approval count, so the rule is built from two parts (see the
//! header of `.github/CODEOWNERS` and CONTRIBUTING.md "Branch protection
//! (maintainers)"):
//!
//! 1. `.github/CODEOWNERS` gives every gated path a code owner.
//! 2. Branch protection on every protected branch sets "Require review from
//!    Code Owners", at least one required approval, approval of the most
//!    recent push, stale-approval dismissal, the `ci-ok` check, and no force
//!    push or deletion. A PR author can't approve their own PR, so a PR that
//!    touches an owned path needs an owning reviewer other than the author.
//!
//! This module checks both:
//!
//! * Offline ([`check_tree`], run on the real tree by
//!   `xtask/tests/codeowners.rs` in CI): CODEOWNERS parses, every rule has
//!   valid owners and only syntax GitHub supports, there is no second
//!   CODEOWNERS file, and every path in [`required_paths`] has an owner
//!   (last matching rule wins, as on GitHub). [`required_paths`] is the one
//!   list of gated paths. It is built from [`GATED_MODULES`] (which must agree
//!   with CONTRIBUTING.md's "Safety-gated code" list, see
//!   [`gated_list_problems`]), the T6 constants in [`crate::writes`]
//!   ([`SCAN_CRATES`], [`TOKEN_TYPES`]), and the files in the tree: every
//!   crate manifest, every T6 test and compile-fail fixture, every workflow
//!   and every file that defines a capability token. The branch-protection
//!   payload in CONTRIBUTING.md is parsed ([`contributing_payload`]) and
//!   checked against the policy above ([`policy_problems`]). A branch may
//!   require more status checks than the shared payload: CONTRIBUTING.md
//!   adds them with a `POST …/branches/<branch>/protection/
//!   required_status_checks/contexts` command after the loop
//!   ([`Payload::extra_contexts`], [`Payload::expected`]).
//! * Live (`cargo xtask check-branch-protection`, opt-in, never in tests:
//!   it needs the network and repo-admin read access): for each protected
//!   branch, the protection GitHub reports must equal the CONTRIBUTING.md
//!   payload (plus that branch's extra contexts) and satisfy the policy, `.github/CODEOWNERS` must exist on that
//!   branch with the same content as the local file, and GitHub's
//!   `codeowners/errors` must be empty ([`check_live`]).
//!
//! Pattern matching ([`matches`]) follows GitHub's documented CODEOWNERS
//! rules, erring on "not owned": a pattern whose last segment has a wildcard
//! (`docs/*`, `**/*write*`) matches files only, not the contents of a
//! directory whose name matches (GitHub documents `docs/*` as not owning
//! `docs/a/b.md`). A trailing `/` matches a directory and everything under
//! it, and a pattern with no `/` except a trailing one matches at any depth.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::Value;

use crate::globals::SKIP_DIRS;
use crate::writes::{SCAN_CRATES, TOKEN_TYPES};

/// The CODEOWNERS file, relative to the workspace root.
pub const CODEOWNERS: &str = ".github/CODEOWNERS";

/// Other places GitHub looks for CODEOWNERS. GitHub uses only the first one it
/// finds (`.github/` first), so a file at one of these would be dead and
/// misleading.
pub const SHADOWED_LOCATIONS: &[&str] = &["CODEOWNERS", "docs/CODEOWNERS"];

/// The contribution guide, relative to the workspace root.
pub const CONTRIBUTING: &str = "CONTRIBUTING.md";

/// Heading of the CONTRIBUTING.md section that holds the protection payload.
pub const PROTECTION_SECTION: &str = "### Branch protection (maintainers)";

/// Line in CONTRIBUTING.md that opens the "Safety-gated code" bullet list.
pub const GATED_LIST_LEAD: &str = "**Safety-gated code** is:";

/// The aggregate CI job that branch protection must require.
pub const REQUIRED_CHECK: &str = "ci-ok";

/// The gated modules: `(crate, module, why)`. Each must have an owner as a
/// file module (`src/<m>.rs`) and as a directory module (`src/<m>/…`),
/// including before the module exists, so it is owned from its first commit.
pub const GATED_MODULES: &[(&str, &str, &str)] = &[
    (
        "rr-core",
        "capability",
        "capability tokens RomWrite / EcuMemoryWrite / EcuReset (D-013)",
    ),
    ("rr-core", "encoding", "T7 mutation-gated module"),
    ("rr-core", "expr", "T7 mutation-gated module"),
    ("rr-rom", "save", "rr-rom write path: save"),
    ("rr-rom", "journal", "rr-rom write path: journal"),
    ("rr-rom", "checksum", "rr-rom write path: checksum"),
];

/// Write-path file names (under `src/` of each [`SCAN_CRATES`] crate) that
/// must be owned wherever they appear in the crate.
pub const WRITE_PATH_FILES: &[&str] = &[
    "write.rs",
    "table/write.rs",
    "table/table_write.rs",
    "a/b/write_memory.rs",
    "write/mod.rs",
    "table/write/block.rs",
];

/// rr-comms write-gating file names (under `crates/rr-comms/src/`).
pub const COMMS_GATING_FILES: &[&str] = &[
    "gate.rs",
    "gating.rs",
    "link/gate.rs",
    "gate/mod.rs",
    "x/gating/mod.rs",
];

/// The crate whose attribute markers T6 relies on: every file is gated.
pub const WRITE_MARKER_CRATE: &str = "crates/rr-write-marker";

/// Fixed files that enforce the safety rules: `(path, why)`.
pub const ENFORCEMENT_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", "workspace lints and members"),
    ("clippy.toml", "lint configuration"),
    ("deny.toml", "supply-chain policy (cargo deny)"),
    (
        ".cargo/mutants.toml",
        "T7 mutation gate: gated paths and exclusions",
    ),
    (
        "xtask/src/mutants_scope.rs",
        "T7 pull-request mutation scope rules",
    ),
    (
        "xtask/src/mutants_exclusions.rs",
        "T7 exclusion-justification check (check-mutants-exclusions)",
    ),
    (
        "xtask/tests/mutants_exclusions.rs",
        "T7 exclusion-justification check's tests",
    ),
    (
        "xtask/src/writes.rs",
        "T6 write-gating check (check-writes)",
    ),
    ("xtask/src/deps.rs", "dependency rule (check-deps)"),
    ("xtask/tests/architecture.rs", "dependency rule tests"),
    ("xtask/src/globals.rs", "global-state check (check-globals)"),
    ("xtask/tests/globals.rs", "global-state check tests"),
    ("xtask/globals-allowlist.toml", "global-state allowlist"),
    (
        "xtask/src/reference_isolation.rs",
        "reference-codec isolation rule (check-reference-isolation)",
    ),
    (
        "xtask/tests/reference_isolation.rs",
        "reference-codec isolation rule tests",
    ),
    (
        "crates/rr-testkit/src/reference.rs",
        "naive reference codec (T2(b) cross-check of Encoding)",
    ),
    (
        "crates/rr-testkit/tests/reference.rs",
        "reference codec known-answer and property tests",
    ),
    (
        "crates/rr-testkit/src/reference/synthetic.rs",
        "synthetic ROM generator (expected values for the Encoding cross-check)",
    ),
    (
        "crates/rr-testkit/tests/synthetic.rs",
        "synthetic ROM generator tests",
    ),
    (
        "crates/rr-testkit/tests/synthetic_coverage.rs",
        "generated synthetic ROM edge-case coverage (US-RR-10-4)",
    ),
    (
        "crates/rr-testkit/examples/gen-synthetic.rs",
        "synthetic ROM writer run by gen-synthetic",
    ),
    (
        "xtask/src/gen_synthetic.rs",
        "synthetic ROM regeneration check (gen-synthetic --check)",
    ),
    (
        "xtask/tests/gen_synthetic.rs",
        "synthetic ROM regeneration check tests",
    ),
    (
        "fixtures/public/synthetic/SHA256SUMS",
        "committed synthetic ROM hashes",
    ),
    (
        "fixtures/public/synthetic/.gitattributes",
        "keeps git from rewriting the committed synthetic ROM bytes",
    ),
    (
        "fuzz/Cargo.toml",
        "T3 fuzz per-input time and memory limits",
    ),
    (
        "xtask/src/fuzz.rs",
        "T3 fuzz runner that applies the limits (cargo xtask fuzz)",
    ),
    (
        "fuzz/src/replay.rs",
        "T3 stable replay of committed crash inputs under the limits",
    ),
    (
        "fuzz/tests/regressions.rs",
        "T3 regression replay harness: memory caps and its self-test",
    ),
    (
        "xtask/src/codeowners.rs",
        "this CODEOWNERS / branch-protection check",
    ),
    (
        "xtask/tests/codeowners.rs",
        "this CODEOWNERS / branch-protection check's tests",
    ),
    (CODEOWNERS, "CODEOWNERS itself"),
];

// ---------------------------------------------------------------------------
// CODEOWNERS parsing and matching
// ---------------------------------------------------------------------------

/// One CODEOWNERS rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 1-based line number.
    pub line: usize,
    /// The path pattern.
    pub pattern: String,
    /// Owners (`@user`, `@org/team` or an e-mail address), possibly none.
    pub owners: Vec<String>,
}

/// Parses CODEOWNERS text into rules. Blank lines and `#` comments are
/// skipped; a `#` token ends the owner list.
pub fn parse(text: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut tokens = t.split_whitespace();
        let Some(pattern) = tokens.next() else {
            continue;
        };
        let owners = tokens
            .take_while(|o| !o.starts_with('#'))
            .map(str::to_owned)
            .collect();
        rules.push(Rule {
            line: i + 1,
            pattern: pattern.to_owned(),
            owners,
        });
    }
    rules
}

/// Whether `owner` is a syntactically valid CODEOWNERS owner: `@user`,
/// `@org/team` or an e-mail address.
pub fn valid_owner(owner: &str) -> bool {
    fn login(s: &str) -> bool {
        (1..=39).contains(&s.len())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !s.starts_with('-')
            && !s.ends_with('-')
            && !s.contains("--")
    }
    if let Some(name) = owner.strip_prefix('@') {
        return match name.split_once('/') {
            None => login(name),
            Some((org, team)) => {
                login(org)
                    && !team.is_empty()
                    && team
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            }
        };
    }
    match owner.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !owner.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

/// Problems with the rules themselves: unsupported syntax, a rule with no
/// owners (it would un-own every path it matches, since the last matching
/// rule wins), or an invalid owner.
pub fn rule_problems(rules: &[Rule]) -> Vec<String> {
    let mut out = Vec::new();
    if rules.is_empty() {
        out.push(format!("{CODEOWNERS}: no rules"));
    }
    for r in rules {
        let at = format!("{CODEOWNERS}:{}: `{}`", r.line, r.pattern);
        if r.pattern.starts_with('!') {
            out.push(format!(
                "{at}: `!` negation is not supported by GitHub CODEOWNERS"
            ));
        }
        if r.pattern.contains(['[', ']']) {
            out.push(format!(
                "{at}: `[ ]` character ranges are not supported by GitHub CODEOWNERS"
            ));
        }
        if r.owners.is_empty() {
            out.push(format!(
                "{at}: no owners; this un-owns every path it matches (the last matching rule wins)"
            ));
        }
        for o in r.owners.iter().filter(|o| !valid_owner(o)) {
            out.push(format!(
                "{at}: invalid owner `{o}` (expected @user, @org/team or an e-mail)"
            ));
        }
    }
    out
}

/// Glob match of one path segment: `*` is any run of characters, `?` one.
fn glob(pattern: &[char], s: &[char]) -> bool {
    match pattern.split_first() {
        None => s.is_empty(),
        Some(('*', rest)) => (0..=s.len()).any(|i| glob(rest, &s[i..])),
        Some(('?', rest)) => !s.is_empty() && glob(rest, &s[1..]),
        Some((c, rest)) => s.first() == Some(c) && glob(rest, &s[1..]),
    }
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| match_segments(rest, &path[i..])),
        Some((seg, rest)) => path.split_first().is_some_and(|(head, tail)| {
            let p: Vec<char> = seg.chars().collect();
            let h: Vec<char> = head.chars().collect();
            glob(&p, &h) && match_segments(rest, tail)
        }),
    }
}

/// Whether CODEOWNERS `pattern` matches the file at `path` (relative to the
/// repo root, `/`-separated). See the module docs for the semantics.
pub fn matches(pattern: &str, path: &str) -> bool {
    let dir_only = pattern.ends_with('/');
    let body = pattern.trim_end_matches('/');
    let anchored = body.contains('/');
    let body = body.trim_start_matches('/');
    if body.is_empty() {
        return false;
    }
    let mut pat: Vec<&str> = body.split('/').collect();
    if !anchored {
        pat.insert(0, "**");
    }
    let wild_tail = pat
        .last()
        .is_some_and(|s| *s != "**" && s.contains(['*', '?']));
    let segs: Vec<&str> = path.split('/').collect();
    (1..=segs.len()).any(|n| {
        let whole_path = n == segs.len();
        let allowed = if whole_path {
            !dir_only
        } else {
            dir_only || !wild_tail
        };
        allowed && match_segments(&pat, &segs[..n])
    })
}

/// The rule that decides ownership of `path`: the last matching one.
pub fn deciding_rule<'a>(rules: &'a [Rule], path: &str) -> Option<&'a Rule> {
    rules.iter().rev().find(|r| matches(&r.pattern, path))
}

// ---------------------------------------------------------------------------
// The gated paths
// ---------------------------------------------------------------------------

/// A path that must have a code owner, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Required {
    /// Path relative to the repo root, `/`-separated.
    pub path: String,
    /// Why it is gated.
    pub why: String,
}

/// Every file under `root/dir` (relative, `/`-separated), skipping
/// [`SKIP_DIRS`]; empty if `dir` doesn't exist.
fn files_under(root: &Path, dir: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let start = root.join(dir);
    if start.is_dir() {
        walk(&start, dir, &mut out)?;
    }
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        if entry.file_type()?.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&entry.path(), &child, out)?;
            }
        } else {
            out.push(child);
        }
    }
    Ok(())
}

/// Whether `src` declares `struct <name>` for an exact identifier `name`.
fn defines_struct(src: &str, name: &str) -> bool {
    let needle = format!("struct {name}");
    src.match_indices(&needle).any(|(i, _)| {
        let before_ok = i == 0
            || src[..i]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric() && c != '_');
        let after_ok = src[i + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        before_ok && after_ok
    })
}

/// Every path that must have a code owner: the single list of gated paths.
/// Hypothetical paths (modules not written yet) are included so they are
/// owned from their first commit; the rest is found in the tree under
/// `root`. Sorted, without duplicates.
pub fn required_paths(root: &Path) -> Result<Vec<Required>> {
    let mut out = BTreeSet::new();
    let mut add = |path: String, why: &str| {
        out.insert(Required {
            path,
            why: why.to_owned(),
        });
    };
    for (krate, module, why) in GATED_MODULES {
        let src = format!("crates/{krate}/src");
        add(format!("{src}/{module}.rs"), why);
        add(format!("{src}/{module}/mod.rs"), why);
        add(format!("{src}/{module}/inner.rs"), why);
    }
    for krate in SCAN_CRATES {
        for f in WRITE_PATH_FILES {
            add(
                format!("{krate}/src/{f}"),
                "write path in a T6-scanned crate",
            );
        }
    }
    for f in COMMS_GATING_FILES {
        add(format!("crates/rr-comms/src/{f}"), "rr-comms write gating");
    }
    for (path, why) in ENFORCEMENT_FILES {
        add((*path).to_owned(), why);
    }
    for f in files_under(root, WRITE_MARKER_CRATE)? {
        add(f, "T6 write markers (rr-write-marker)");
    }
    for f in files_under(root, ".github/workflows")? {
        add(f, "CI workflow");
    }
    for f in files_under(root, "xtask/tests")? {
        if f["xtask/tests/".len()..].starts_with("t6_") {
            add(f, "T6 safety-invariant suite");
        }
    }
    for f in files_under(root, "crates")? {
        let parts: Vec<&str> = f.split('/').collect();
        // crates/<crate>/...
        match parts.as_slice() {
            [_, _, "Cargo.toml"] => add(
                f.clone(),
                "crate manifest (can enable the confirmation-authority feature)",
            ),
            [_, _, "tests", first, ..] if first.starts_with("t6_") => {
                add(f.clone(), "T6 safety-invariant suite");
            }
            [_, _, "tests", "ui", _, ..] => {
                add(f.clone(), "T6 compile-fail fixture (trybuild)");
            }
            [_, _, "src", .., last] if last.ends_with(".rs") => {
                let src =
                    fs::read_to_string(root.join(&f)).with_context(|| format!("reading {f}"))?;
                if TOKEN_TYPES.iter().any(|t| defines_struct(&src, t)) {
                    add(f.clone(), "defines a capability token type");
                }
            }
            _ => {}
        }
    }
    Ok(out.into_iter().collect())
}

/// Every required path that the last matching rule in `codeowners` doesn't
/// give an owner, plus [`rule_problems`].
pub fn ownership_problems(codeowners: &str, required: &[Required]) -> Vec<String> {
    let rules = parse(codeowners);
    let mut out = rule_problems(&rules);
    for r in required {
        match deciding_rule(&rules, &r.path) {
            None => out.push(format!("{} ({}) has no code owner", r.path, r.why)),
            Some(rule) if rule.owners.is_empty() => out.push(format!(
                "{} ({}) is un-owned by {CODEOWNERS}:{} `{}`, which has no owners",
                r.path, r.why, rule.line, rule.pattern
            )),
            Some(_) => {}
        }
    }
    out
}

/// Lines of the bullet list that follows [`GATED_LIST_LEAD`].
fn gated_list(contributing: &str) -> Option<Vec<&str>> {
    let mut lines = contributing
        .lines()
        .skip_while(|l| l.trim() != GATED_LIST_LEAD);
    lines.next()?;
    let list: Vec<&str> = lines
        .skip_while(|l| l.trim().is_empty())
        .take_while(|l| l.trim_start().starts_with("- ") || l.starts_with("  "))
        .collect();
    (!list.is_empty()).then_some(list)
}

/// CONTRIBUTING.md's "Safety-gated code" list and [`GATED_MODULES`] must
/// agree: every module is named there (as `` `crate::module` `` or as
/// `` `crate` `` plus the module name), and every `` `rr-*::module` `` path
/// there is a gated module.
pub fn gated_list_problems(contributing: &str) -> Vec<String> {
    let Some(list) = gated_list(contributing) else {
        return vec![format!(
            "{CONTRIBUTING}: no bullet list after `{GATED_LIST_LEAD}`"
        )];
    };
    let text = list.join("\n");
    let mut out = Vec::new();
    for (krate, module, _) in GATED_MODULES {
        let as_path = text.contains(&format!("`{krate}::{module}`"));
        let as_words = text.contains(&format!("`{krate}`"))
            && text
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w == *module);
        if !as_path && !as_words {
            out.push(format!(
                "{CONTRIBUTING}: the safety-gated list doesn't name {krate}::{module}"
            ));
        }
    }
    for (i, chunk) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue;
        }
        if let Some((krate, module)) = chunk.split_once("::")
            && krate.starts_with("rr-")
            && !GATED_MODULES
                .iter()
                .any(|(k, m, _)| *k == krate && *m == module)
        {
            out.push(format!(
                "{CONTRIBUTING}: safety-gated `{chunk}` is not in codeowners::GATED_MODULES, so its CODEOWNERS coverage is untested"
            ));
        }
    }
    out
}

/// Result of [`check_tree`].
#[derive(Debug)]
pub struct TreeReport {
    /// Number of CODEOWNERS rules.
    pub rules: usize,
    /// The gated paths checked.
    pub required: Vec<Required>,
    /// Everything wrong.
    pub problems: Vec<String>,
}

/// Reads a text file with line endings normalised to `\n`.
pub fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .map(|s| s.replace("\r\n", "\n"))
        .with_context(|| format!("reading {}", path.display()))
}

/// The offline check on the tree at `root`.
pub fn check_tree(root: &Path) -> Result<TreeReport> {
    let codeowners = read_text(&root.join(CODEOWNERS))?;
    let contributing = read_text(&root.join(CONTRIBUTING))?;
    let required = required_paths(root)?;
    let mut problems = ownership_problems(&codeowners, &required);
    for loc in SHADOWED_LOCATIONS {
        if root.join(loc).exists() {
            problems.push(format!(
                "{loc} exists; GitHub uses only {CODEOWNERS}, so {loc} is ignored"
            ));
        }
    }
    problems.extend(gated_list_problems(&contributing));
    match contributing_payload(&contributing) {
        Ok(p) => problems.extend(
            policy_problems(&p.protection)
                .into_iter()
                .map(|e| format!("{CONTRIBUTING} payload: {e}")),
        ),
        Err(e) => problems.push(format!("{e:#}")),
    }
    Ok(TreeReport {
        rules: parse(&codeowners).len(),
        required,
        problems,
    })
}

// ---------------------------------------------------------------------------
// Branch protection
// ---------------------------------------------------------------------------

/// The branch-protection settings this project cares about. `None` means the
/// setting is absent (e.g. no required reviews at all).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Protection {
    /// Required status checks must be up to date with the base branch.
    pub strict: Option<bool>,
    /// Required status-check contexts.
    pub contexts: BTreeSet<String>,
    /// Whether admins are subject to the rules too.
    pub enforce_admins: Option<bool>,
    /// New commits dismiss existing approvals.
    pub dismiss_stale_reviews: Option<bool>,
    /// "Require review from Code Owners".
    pub require_code_owner_reviews: Option<bool>,
    /// Number of approvals required.
    pub required_approving_review_count: Option<u64>,
    /// The most recent push must be approved by someone other than its pusher.
    pub require_last_push_approval: Option<bool>,
    /// All conversations must be resolved.
    pub required_conversation_resolution: Option<bool>,
    /// Force pushes allowed.
    pub allow_force_pushes: Option<bool>,
    /// Branch deletion allowed.
    pub allow_deletions: Option<bool>,
}

/// A flag written as `true` (the PUT payload) or `{"enabled": true}` (what
/// GET returns).
fn flag(v: Option<&Value>) -> Option<bool> {
    let v = v?;
    v.as_bool()
        .or_else(|| v.get("enabled").and_then(Value::as_bool))
}

impl Protection {
    /// Reads either the PUT payload or the GET response of
    /// `repos/{repo}/branches/{branch}/protection`.
    pub fn from_json(v: &Value) -> Result<Self> {
        if !v.is_object() {
            bail!("branch protection JSON is not an object");
        }
        let checks = v.get("required_status_checks").filter(|c| !c.is_null());
        let reviews = v
            .get("required_pull_request_reviews")
            .filter(|r| !r.is_null());
        let review = |k: &str| reviews.and_then(|r| r.get(k));
        let contexts = checks
            .and_then(|c| c.get("contexts"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            strict: flag(checks.and_then(|c| c.get("strict"))),
            contexts,
            enforce_admins: flag(v.get("enforce_admins")),
            dismiss_stale_reviews: flag(review("dismiss_stale_reviews")),
            require_code_owner_reviews: flag(review("require_code_owner_reviews")),
            required_approving_review_count: review("required_approving_review_count")
                .and_then(Value::as_u64),
            require_last_push_approval: flag(review("require_last_push_approval")),
            required_conversation_resolution: flag(v.get("required_conversation_resolution")),
            allow_force_pushes: flag(v.get("allow_force_pushes")),
            allow_deletions: flag(v.get("allow_deletions")),
        })
    }

    fn fields(&self) -> Vec<(&'static str, String)> {
        fn show<T: fmt::Debug>(v: Option<T>) -> String {
            v.map_or_else(|| "unset".to_owned(), |v| format!("{v:?}"))
        }
        vec![
            ("required_status_checks.strict", show(self.strict)),
            (
                "required_status_checks.contexts",
                format!("{:?}", self.contexts),
            ),
            ("enforce_admins", show(self.enforce_admins)),
            ("dismiss_stale_reviews", show(self.dismiss_stale_reviews)),
            (
                "require_code_owner_reviews",
                show(self.require_code_owner_reviews),
            ),
            (
                "required_approving_review_count",
                show(self.required_approving_review_count),
            ),
            (
                "require_last_push_approval",
                show(self.require_last_push_approval),
            ),
            (
                "required_conversation_resolution",
                show(self.required_conversation_resolution),
            ),
            ("allow_force_pushes", show(self.allow_force_pushes)),
            ("allow_deletions", show(self.allow_deletions)),
        ]
    }
}

/// What CODEOWNERS needs from branch protection to turn into a required
/// second approval, and what would let a push bypass it.
pub fn policy_problems(p: &Protection) -> Vec<String> {
    let mut out = Vec::new();
    if p.require_code_owner_reviews != Some(true) {
        out.push(
            "require_code_owner_reviews must be true: without it CODEOWNERS requires no approval"
                .to_owned(),
        );
    }
    if p.required_approving_review_count.is_none_or(|n| n < 1) {
        out.push("required_approving_review_count must be at least 1".to_owned());
    }
    if p.require_last_push_approval != Some(true) {
        out.push(
            "require_last_push_approval must be true: otherwise the author can push after the code owner approves"
                .to_owned(),
        );
    }
    if p.dismiss_stale_reviews != Some(true) {
        out.push(
            "dismiss_stale_reviews must be true: otherwise an approval survives new commits"
                .to_owned(),
        );
    }
    if !p.contexts.contains(REQUIRED_CHECK) {
        out.push(format!(
            "required_status_checks.contexts must include `{REQUIRED_CHECK}`"
        ));
    }
    if p.allow_force_pushes != Some(false) {
        out.push("allow_force_pushes must be false".to_owned());
    }
    if p.allow_deletions != Some(false) {
        out.push("allow_deletions must be false".to_owned());
    }
    out
}

/// Every setting where `live` differs from `expected`.
pub fn compare(expected: &Protection, live: &Protection) -> Vec<String> {
    expected
        .fields()
        .into_iter()
        .zip(live.fields())
        .filter(|((_, e), (_, l))| e != l)
        .map(|((name, e), (_, l))| format!("{name}: {CONTRIBUTING} sets {e}, GitHub has {l}"))
        .collect()
}

/// The branch-protection command documented in CONTRIBUTING.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// `owner/name` from the `repos/{owner}/{name}/branches` URL.
    pub repo: String,
    /// The branches the `for branch in …` loop protects.
    pub branches: Vec<String>,
    /// The JSON payload.
    pub protection: Protection,
    /// Status-check contexts a branch requires on top of the payload's,
    /// from the `POST …/protection/required_status_checks/contexts`
    /// commands after the loop.
    pub extra_contexts: BTreeMap<String, BTreeSet<String>>,
}

impl Payload {
    /// The protection `branch` must have: the shared payload plus the
    /// branch's extra contexts.
    #[must_use]
    pub fn expected(&self, branch: &str) -> Protection {
        let mut p = self.protection.clone();
        if let Some(extra) = self.extra_contexts.get(branch) {
            p.contexts.extend(extra.iter().cloned());
        }
        p
    }
}

/// The `<<'JSON'` heredoc of the command on `section[at]`: opened on that
/// line or on its continuation line, as text.
fn heredoc_after(section: &[&str], at: usize) -> Result<String> {
    let start = section[at..]
        .iter()
        .take(2)
        .position(|l| l.trim_end().ends_with("<<'JSON'"))
        .map(|i| at + i)
        .ok_or_else(|| {
            anyhow!("{CONTRIBUTING}: no `<<'JSON'` heredoc in `{PROTECTION_SECTION}`")
        })?;
    let len = section[start + 1..]
        .iter()
        .position(|l| l.trim() == "JSON")
        .ok_or_else(|| anyhow!("{CONTRIBUTING}: the `<<'JSON'` heredoc is not closed"))?;
    Ok(section[start + 1..start + 1 + len].join("\n"))
}

/// The `{"contexts": [...]}` commands that add required checks to one
/// branch, by branch.
fn extra_contexts(section: &[&str], repo: &str) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let prefix = format!("repos/{repo}/branches/");
    for (i, l) in section.iter().enumerate() {
        let Some((_, rest)) = l.split_once(&prefix) else {
            continue;
        };
        let Some((branch, _)) = rest.split_once("/protection/required_status_checks/contexts")
        else {
            continue;
        };
        if !l.contains("--method POST") {
            bail!("{CONTRIBUTING}: the `{branch}` contexts command is not `gh api --method POST`");
        }
        let json = heredoc_after(section, i)?;
        let value: Value = serde_json::from_str(&json).with_context(|| {
            format!("{CONTRIBUTING}: the `{branch}` contexts payload is not valid JSON")
        })?;
        let contexts = value
            .get("contexts")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                anyhow!("{CONTRIBUTING}: the `{branch}` contexts payload has no `contexts` array")
            })?;
        let set = out.entry(branch.to_owned()).or_default();
        for c in contexts {
            let c = c
                .as_str()
                .ok_or_else(|| anyhow!("{CONTRIBUTING}: a `{branch}` context is not a string"))?;
            set.insert(c.to_owned());
        }
    }
    Ok(out)
}

/// Parses the `gh api --method PUT …/protection` loop in CONTRIBUTING.md's
/// [`PROTECTION_SECTION`].
pub fn contributing_payload(contributing: &str) -> Result<Payload> {
    let section: Vec<&str> = contributing
        .lines()
        .skip_while(|l| l.trim_end() != PROTECTION_SECTION)
        .skip(1)
        .take_while(|l| !(l.starts_with("## ") || l.starts_with("### ")))
        .collect();
    if section.is_empty() {
        bail!("{CONTRIBUTING}: no `{PROTECTION_SECTION}` section");
    }
    let branches = section
        .iter()
        .find_map(|l| {
            l.trim()
                .strip_prefix("for branch in ")?
                .split_once(';')
                .map(|(list, _)| {
                    list.split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
        })
        .filter(|b| !b.is_empty())
        .ok_or_else(|| {
            anyhow!("{CONTRIBUTING}: no `for branch in …; do` loop in `{PROTECTION_SECTION}`")
        })?;
    let repo = section
        .iter()
        .find_map(|l| {
            let (_, rest) = l.split_once("repos/")?;
            let (repo, _) = rest.split_once("/branches/")?;
            (l.contains("--method PUT")).then(|| repo.to_owned())
        })
        .ok_or_else(|| {
            anyhow!("{CONTRIBUTING}: no `gh api --method PUT repos/…/branches/…` line")
        })?;
    let put = section
        .iter()
        .position(|l| l.contains("--method PUT"))
        .unwrap_or(0);
    let json = heredoc_after(&section, put)?;
    let value: Value = serde_json::from_str(&json)
        .with_context(|| format!("{CONTRIBUTING}: branch-protection payload is not valid JSON"))?;
    let extra_contexts = extra_contexts(&section, &repo)?;
    for b in extra_contexts.keys() {
        if !branches.contains(b) {
            bail!("{CONTRIBUTING}: extra contexts for `{b}`, which the loop does not protect");
        }
    }
    Ok(Payload {
        repo,
        branches,
        protection: Protection::from_json(&value)?,
        extra_contexts,
    })
}

// ---------------------------------------------------------------------------
// Live check
// ---------------------------------------------------------------------------

/// Read-only access to the GitHub REST API.
pub trait Api {
    /// GETs `path` (e.g. `repos/o/r/branches/b/protection`). `raw` asks for
    /// raw file content. `Ok(None)` on 404.
    fn get(&self, path: &str, raw: bool) -> Result<Option<String>>;
}

/// [`Api`] through the `gh` CLI (`gh api`, GET only).
pub struct Gh;

impl Api for Gh {
    fn get(&self, path: &str, raw: bool) -> Result<Option<String>> {
        let mut cmd = Command::new("gh");
        cmd.arg("api");
        if raw {
            cmd.args(["-H", "Accept: application/vnd.github.raw"]);
        }
        cmd.arg(path);
        let out = cmd
            .output()
            .context("running `gh api` (is the GitHub CLI installed and authenticated?)")?;
        if out.status.success() {
            return Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("HTTP 404") {
            return Ok(None);
        }
        bail!("gh api {path}: {}", stderr.trim())
    }
}

/// Problems found on one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchReport {
    /// Branch name.
    pub branch: String,
    /// Everything wrong; empty means enforced as documented.
    pub problems: Vec<String>,
}

/// For each branch in `branches` of `repo`: live protection equals
/// `expected` and passes [`policy_problems`]; `.github/CODEOWNERS` exists on
/// the branch with the same content as `local_codeowners` (line endings
/// normalised); GitHub reports no CODEOWNERS errors.
pub fn check_live(
    api: &impl Api,
    repo: &str,
    branches: &[String],
    expected: &Protection,
    local_codeowners: &str,
) -> Result<Vec<BranchReport>> {
    let local = local_codeowners.replace("\r\n", "\n");
    let mut reports = Vec::new();
    for branch in branches {
        let mut problems = Vec::new();
        match api.get(&format!("repos/{repo}/branches/{branch}/protection"), false)? {
            None => problems.push("branch is not protected".to_owned()),
            Some(body) => {
                let value: Value = serde_json::from_str(&body)
                    .with_context(|| format!("{branch}: protection response is not JSON"))?;
                let live = Protection::from_json(&value)?;
                problems.extend(compare(expected, &live));
                problems.extend(policy_problems(&live));
            }
        }
        let path = format!("repos/{repo}/contents/{CODEOWNERS}?ref={branch}");
        match api.get(&path, true)? {
            None => problems.push(format!(
                "no {CODEOWNERS} on this branch: require_code_owner_reviews has nothing to enforce for PRs into it"
            )),
            Some(remote) => {
                if remote.replace("\r\n", "\n") != local {
                    problems.push(format!(
                        "{CODEOWNERS} on this branch differs from the local file (commit and push it)"
                    ));
                }
                let errors = api
                    .get(&format!("repos/{repo}/codeowners/errors?ref={branch}"), false)?
                    .ok_or_else(|| anyhow!("{branch}: codeowners/errors returned 404"))?;
                let value: Value = serde_json::from_str(&errors)
                    .with_context(|| format!("{branch}: codeowners/errors is not JSON"))?;
                let list = value
                    .get("errors")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow!("{branch}: codeowners/errors has no `errors` array"))?;
                for e in list {
                    let field = |k: &str| e.get(k).map(ToString::to_string).unwrap_or_default();
                    problems.push(format!(
                        "GitHub reports a CODEOWNERS error at line {}: {} {}",
                        field("line"),
                        field("kind"),
                        field("message")
                    ));
                }
            }
        }
        reports.push(BranchReport {
            branch: branch.clone(),
            problems,
        });
    }
    Ok(reports)
}
