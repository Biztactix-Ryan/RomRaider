//! Doc-contract tests for the contribution rules (story US-RR-8).
//!
//! Each criterion is a pure check over file text that returns a list of
//! problems; one test runs it on the real tree (and so fails in CI's nextest
//! jobs if the docs regress), and further tests feed it deliberately broken
//! text to prove the check catches the violation.
//!
//! The real-tree tests read from the workspace root, or from
//! `RR_CONTRIB_DOCS_ROOT` when set, so a scratch copy of the docs can be
//! checked without touching the repository.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn docs_root() -> PathBuf {
    match std::env::var_os("RR_CONTRIB_DOCS_ROOT") {
        Some(root) => PathBuf::from(root),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
    }
}

fn read(rel: &str) -> io::Result<String> {
    read_text(&docs_root().join(rel))
}

/// Reads a text file with line endings normalised to `\n`, so the
/// `\n`-based checks and mutations in these tests also apply on a Windows
/// checkout, where git's autocrlf turns them into `\r\n`.
fn read_text(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
        .map(|s| s.replace("\r\n", "\n"))
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// Markdown lines outside fenced code blocks, with their 1-based numbers.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fenced = false;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push((i + 1, line));
        }
    }
    out
}

/// ATX headings as `(level, text)`, skipping fenced code.
fn headings(text: &str) -> Vec<(usize, String)> {
    prose_lines(text)
        .into_iter()
        .filter_map(|(_, line)| {
            // `level` counts single-byte '#' characters, so the slice is safe.
            let level = line.chars().take_while(|&c| c == '#').count();
            let rest = &line[level..];
            ((1..=6).contains(&level) && rest.starts_with(' '))
                .then(|| (level, rest.trim().trim_end_matches('#').trim().to_owned()))
        })
        .collect()
}

/// GitHub's heading anchor: lowercase, keep letters, digits, `-` and `_`,
/// spaces become `-`, everything else is dropped.
fn github_slug(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every anchor a document's headings produce, with GitHub's `-1`, `-2`
/// suffixes for duplicates.
fn anchors(text: &str) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    headings(text)
        .into_iter()
        .map(|(_, h)| {
            let slug = github_slug(&h);
            let n = seen.entry(slug.clone()).or_insert(0);
            let anchor = if *n == 0 { slug } else { format!("{slug}-{n}") };
            *n += 1;
            anchor
        })
        .collect()
}

/// The lines of the section introduced by the first heading (any level) for
/// which `pick` is true, up to the next heading of the same or higher level.
fn section(text: &str, pick: impl Fn(usize, &str) -> bool) -> Option<Vec<&str>> {
    let lines = prose_lines(text);
    let level_of = |line: &str| {
        let level = line.chars().take_while(|&c| c == '#').count();
        ((1..=6).contains(&level) && line[level..].starts_with(' ')).then_some(level)
    };
    let start = lines
        .iter()
        .position(|(_, l)| level_of(l).is_some_and(|lv| pick(lv, l[lv..].trim())))?;
    let level = level_of(lines[start].1)?;
    Some(
        lines[start + 1..]
            .iter()
            .take_while(|(_, l)| level_of(l).is_none_or(|lv| lv > level))
            .map(|(_, l)| *l)
            .collect(),
    )
}

/// Inline markdown link targets `](target)` in prose lines, with line numbers.
fn link_targets(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, line) in prose_lines(text) {
        let mut rest = line;
        while let Some(i) = rest.find("](") {
            rest = &rest[i + 2..];
            if let Some(end) = rest.find(')') {
                out.push((n, rest[..end].trim().to_owned()));
                rest = &rest[end..];
            }
        }
    }
    out
}

/// Checkbox items (`- [ ] ...`) in a set of lines, returning their text.
fn checkbox_items<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|l| l.trim_start().strip_prefix("- [ ] "))
        .collect()
}

// ---------------------------------------------------------------------------
// US-RR-8-1: PR template checklist
// ---------------------------------------------------------------------------

/// Problems with `.github/pull_request_template.md` against US-RR-8-1: the
/// `## Definition of Done` section must hold checklist (`- [ ]`) items in
/// sub-sections for test tiers (T0..T10), ledger entries closed with named
/// tests (MF-/EC-), the concept catalogue (C1–C16 or a DECISIONS entry) and
/// safety-gated review (the `safety-gated` label).
fn pr_template_problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(dod) = section(text, |lv, h| lv == 2 && h == "Definition of Done") else {
        return vec!["no `## Definition of Done` section".to_owned()];
    };
    let dod_text = dod.join("\n");
    let sub = |keyword: &str| -> Option<Vec<&str>> {
        section(&dod_text, |lv, h| {
            lv == 3 && h.to_lowercase().contains(keyword)
        })
    };
    let mut need =
        |name: &str, keyword: &str, check: &dyn Fn(&[&str]) -> Vec<String>| {
            match sub(
        keyword,
    ) {
        None => problems.push(format!(
            "no `###` {name} section (heading containing `{keyword}`) under `## Definition of Done`"
        )),
        Some(lines) if checkbox_items(&lines).is_empty() => {
            problems.push(format!("{name} section has no `- [ ]` checklist items"));
        }
        Some(lines) => problems.extend(check(&lines).into_iter().map(|p| format!("{name}: {p}"))),
    }
        };

    need("test tiers", "test tier", &|lines| {
        let items = checkbox_items(lines);
        (0..=10)
            .filter(|t| {
                let tier = format!("T{t} ");
                !items.iter().any(|i| i.starts_with(&tier))
            })
            .map(|t| format!("no checklist item for tier T{t}"))
            .collect()
    });

    need("ledger entries", "ledger", &|lines| {
        let body = lines.join("\n");
        let items = checkbox_items(lines);
        let mut p = Vec::new();
        for id in ["MF-", "EC-"] {
            if !body.contains(id) {
                p.push(format!("does not mention `{id}xx` ledger entries"));
            }
        }
        if !items.iter().any(|i| i.to_lowercase().contains("test")) {
            p.push("no checklist item requiring closed entries to name their test".to_owned());
        }
        let has_test_column = lines.iter().any(|l| {
            let cols: Vec<&str> = l.split('|').map(str::trim).collect();
            l.trim_start().starts_with('|')
                && cols.contains(&"Entry")
                && cols.iter().any(|c| c.starts_with("Test"))
        });
        if !has_test_column {
            p.push("no `| Entry | Test ... |` table naming each closed entry's test".to_owned());
        }
        p
    });

    need("concept catalogue", "concept catalogue", &|lines| {
        let items = checkbox_items(lines);
        let mut p = Vec::new();
        if !items
            .iter()
            .any(|i| i.contains("C1–C16") || i.contains("C1-C16"))
        {
            p.push("no checklist item for reusing an existing concept (C1–C16)".to_owned());
        }
        if !items.iter().any(|i| i.contains("DECISIONS")) {
            p.push("no checklist item for a new concept's DECISIONS.md entry".to_owned());
        }
        p
    });

    need("safety-gated review", "safety", &|lines| {
        let items = checkbox_items(lines);
        if items
            .iter()
            .any(|i| i.contains("`safety-gated`") && i.contains("label"))
        {
            Vec::new()
        } else {
            vec!["no checklist item requiring the `safety-gated` label".to_owned()]
        }
    });

    problems
}

/// Problems with links from the PR template to CONTRIBUTING.md anchors: each
/// `.../CONTRIBUTING.md#anchor` must resolve to a CONTRIBUTING.md heading.
fn pr_template_link_problems(template: &str, contributing: &str) -> Vec<String> {
    let known = anchors(contributing);
    let mut all = link_targets(template);
    // The HTML comment carries a bare URL rather than a markdown link.
    for (n, line) in template.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("https://") && !t.contains(' ') {
            all.push((n + 1, t.to_owned()));
        }
    }
    all.into_iter()
        .filter_map(|(n, target)| {
            let (path, anchor) = target.split_once('#')?;
            (path.ends_with("CONTRIBUTING.md") && !known.iter().any(|a| a == anchor))
                .then(|| format!("line {n}: `{target}` has no matching heading in CONTRIBUTING.md"))
        })
        .collect()
}

/// Proves US-RR-8-1: the real PR template carries the Definition of Done
/// checklist (test tiers, ledger entries with named tests, concept catalogue,
/// `safety-gated` label review).
#[test]
fn pr_template_has_definition_of_done_checklist() {
    let problems = pr_template_problems(&read(".github/pull_request_template.md").unwrap());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// US-RR-8-1: the PR template's links into CONTRIBUTING.md land on real
/// headings, so each checklist group points at the rule it enforces.
#[test]
fn pr_template_contributing_links_resolve() {
    let problems = pr_template_link_problems(
        &read(".github/pull_request_template.md").unwrap(),
        &read("CONTRIBUTING.md").unwrap(),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// US-RR-8-1 violation demo: removing any required group, checkbox or
/// keyword from the real template is reported.
#[test]
fn pr_template_check_catches_violations() {
    let real = read(".github/pull_request_template.md").unwrap();
    assert!(pr_template_problems(&real).is_empty());
    let cases: [(&str, &str, &str); 8] = [
        (
            "## Definition of Done",
            "## Checklist",
            "no `## Definition of Done`",
        ),
        ("- [ ] T7 Mutation", "- T7 Mutation", "tier T7"),
        (
            "### 2. Ledger entries closed",
            "### 2. Bookkeeping",
            "ledger entries",
        ),
        ("| Entry | Test", "| Entry | Notes", "Entry | Test"),
        ("(C1–C16)", "(see catalogue)", "C1–C16"),
        ("DECISIONS.md entry (`D-0xx`)", "decision note", "DECISIONS"),
        (
            "the PR has the `safety-gated` label;",
            "a maintainer looked;",
            "`safety-gated` label",
        ),
        (
            "### 6. Safety-gated paths",
            "### 6. Sensitive paths",
            "safety-gated review",
        ),
    ];
    for (from, to, expect) in cases {
        assert!(
            real.contains(from),
            "fixture text `{from}` not in real template"
        );
        let broken = real.replacen(from, to, 1);
        let problems = pr_template_problems(&broken);
        assert!(
            problems.iter().any(|p| p.contains(expect)),
            "replacing `{from}` was not reported (want `{expect}`): {problems:?}"
        );
    }
    // Checklist items turned into plain bullets are not a checklist.
    let no_boxes = real.replace("- [ ] ", "- ");
    assert!(!pr_template_problems(&no_boxes).is_empty());
}

/// US-RR-8-1 violation demo: a dangling CONTRIBUTING.md anchor is reported.
#[test]
fn pr_template_link_check_catches_dangling_anchor() {
    let template = "See [x](https://example.invalid/blob/master/CONTRIBUTING.md#no-such-rule).\n";
    let contributing = "# Contributing\n\n## Definition of Done\n";
    let problems = pr_template_link_problems(template, contributing);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let ok = "See [x](https://example.invalid/CONTRIBUTING.md#definition-of-done).\n";
    assert!(pr_template_link_problems(ok, contributing).is_empty());
}

// ---------------------------------------------------------------------------
// US-RR-8-3: issue template asks for scrubbed traces
// ---------------------------------------------------------------------------

/// One `body:` element of a GitHub issue form: its `type`, its `id` (if any)
/// and its raw text.
struct FormItem {
    kind: String,
    id: Option<String>,
    text: String,
}

/// Indentation of a line, in spaces.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Splits an issue form's top-level `body:` sequence into its elements, by
/// indentation (no YAML crate needed for the shape GitHub issue forms use).
fn form_items(form: &str) -> Vec<FormItem> {
    let lines: Vec<&str> = form.lines().collect();
    let Some(body) = lines.iter().position(|l| l.trim_end() == "body:") else {
        return Vec::new();
    };
    let mut items: Vec<FormItem> = Vec::new();
    let mut item_indent = None;
    for line in &lines[body + 1..] {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            if let Some(item) = items.last_mut() {
                item.text.push('\n');
            }
            continue;
        }
        if indent(line) == 0 {
            break; // next top-level key
        }
        if let Some(kind) = t.strip_prefix("- type:")
            && item_indent.is_none_or(|i| i == indent(line))
        {
            item_indent = Some(indent(line));
            items.push(FormItem {
                kind: kind.trim().to_owned(),
                id: None,
                text: String::new(),
            });
            continue;
        }
        if let Some(item) = items.last_mut() {
            if item.id.is_none()
                && item_indent.is_some_and(|i| indent(line) == i + 2)
                && let Some(id) = t.strip_prefix("id:")
            {
                item.id = Some(id.trim().to_owned());
            }
            item.text.push_str(line);
            item.text.push('\n');
        }
    }
    items
}

/// Problems with the bug-report form, `config.yml` and any other issue
/// templates against US-RR-8-3: the bug form tells users to run
/// `romraider-cli trace scrub` before attaching, has an attachments field
/// repeating that, and a required "scrubbed" checkbox; blank issues are
/// disabled so the form can't be bypassed; no legacy markdown template
/// (with its Java version field) is reintroduced.
fn issue_template_problems(
    bug_form: &str,
    config: &str,
    other_templates: &[(String, String)],
) -> Vec<String> {
    const SCRUB: &str = "romraider-cli trace scrub";
    let mut problems = Vec::new();
    let items = form_items(bug_form);
    if items.is_empty() {
        problems.push("bug_report.yml has no `body:` form elements".to_owned());
    }

    // The instruction is shown before the fields (a leading markdown block).
    let first_input = items.iter().position(|i| i.kind != "markdown");
    let intro_says_scrub = items
        .iter()
        .take(first_input.unwrap_or(items.len()))
        .any(|i| i.kind == "markdown" && i.text.contains(SCRUB));
    if !intro_says_scrub {
        problems.push(format!(
            "bug form has no markdown instructions mentioning `{SCRUB}` before its first field"
        ));
    }

    match items
        .iter()
        .find(|i| i.kind == "textarea" && i.id.as_deref() == Some("attachments"))
    {
        None => problems.push("bug form has no `textarea` with `id: attachments`".to_owned()),
        Some(a) if !a.text.contains(SCRUB) => problems.push(format!(
            "attachments field does not tell users to run `{SCRUB}`"
        )),
        Some(_) => {}
    }

    let scrubbed_box = items.iter().find(|i| {
        i.kind == "checkboxes"
            && i.text.lines().any(|l| {
                l.trim_start().starts_with("- label:") && l.to_lowercase().contains("scrub")
            })
    });
    match scrubbed_box {
        None => {
            problems.push("bug form has no checkbox confirming traces were scrubbed".to_owned())
        }
        Some(b) => {
            // `required: true` must sit on the checkbox option itself.
            let lines: Vec<&str> = b.text.lines().collect();
            let required = lines.iter().enumerate().any(|(n, l)| {
                l.trim_start().starts_with("- label:")
                    && l.to_lowercase().contains("scrub")
                    && lines[n + 1..]
                        .iter()
                        .take_while(|next| indent(next) > indent(l))
                        .any(|next| next.trim() == "required: true")
            });
            if !required {
                problems.push("scrubbed checkbox is not `required: true`".to_owned());
            }
            if !b.text.contains(SCRUB) {
                problems.push(format!("scrubbed checkbox does not name `{SCRUB}`"));
            }
        }
    }

    let blank_disabled = config.lines().any(|l| {
        l.split('#')
            .next()
            .is_some_and(|v| v.trim_end() == "blank_issues_enabled: false")
    });
    if !blank_disabled {
        problems.push(
            "config.yml does not set `blank_issues_enabled: false` (blank issues bypass the form)"
                .to_owned(),
        );
    }

    for (name, text) in other_templates {
        if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
            problems.push(format!(
                "ISSUE_TEMPLATE/{name}: non-form template bypasses the scrub confirmation"
            ));
        }
        if text.to_lowercase().contains("java version") {
            problems.push(format!(
                "ISSUE_TEMPLATE/{name}: asks for a Java version (legacy template)"
            ));
        }
    }
    problems
}

/// Every issue template other than `bug_report.yml` and `config.yml`.
fn other_issue_templates() -> io::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(docs_root().join(".github/ISSUE_TEMPLATE"))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        if path.is_file() && name != "bug_report.yml" && name != "config.yml" {
            out.push((name, read_text(&path)?));
        }
    }
    out.sort();
    Ok(out)
}

/// Proves US-RR-8-3: the real bug-report form instructs users to run
/// `romraider-cli trace scrub` before attaching traces, requires the
/// "scrubbed" confirmation, and cannot be bypassed via blank or legacy
/// templates.
#[test]
fn issue_template_requires_scrubbed_traces() {
    let problems = issue_template_problems(
        &read(".github/ISSUE_TEMPLATE/bug_report.yml").unwrap(),
        &read(".github/ISSUE_TEMPLATE/config.yml").unwrap(),
        &other_issue_templates().unwrap(),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// US-RR-8-3: the indentation-based form reader sees the real form's shape,
/// so the check above cannot pass vacuously.
#[test]
fn issue_form_reader_sees_real_form_elements() {
    let items = form_items(&read(".github/ISSUE_TEMPLATE/bug_report.yml").unwrap());
    let ids: Vec<&str> = items.iter().filter_map(|i| i.id.as_deref()).collect();
    for want in ["app", "version", "steps", "attachments", "scrubbed"] {
        assert!(ids.contains(&want), "missing id {want} in {ids:?}");
    }
    assert_eq!(items.first().map(|i| i.kind.as_str()), Some("markdown"));
}

/// US-RR-8-3 violation demo: each way of weakening the scrub requirement is
/// reported.
#[test]
fn issue_template_check_catches_violations() {
    assert_issue_template_violations_caught(
        &read(".github/ISSUE_TEMPLATE/bug_report.yml").unwrap(),
        &read(".github/ISSUE_TEMPLATE/config.yml").unwrap(),
    );
}

/// The violation demo above on a `\r\n` copy of the issue templates, as a
/// Windows runner checks them out: the reader normalises them to the same
/// text as the `\n` originals, and every mutation still applies. Exercises
/// the CRLF path on every platform.
#[test]
fn issue_template_check_works_on_crlf_checkout() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("contribution-docs-crlf");
    fs::create_dir_all(&dir).unwrap();
    let crlf_copy = |rel: &str| {
        let lf = read(rel).unwrap().replace("\r\n", "\n");
        let path = dir.join(Path::new(rel).file_name().unwrap());
        fs::write(&path, lf.replace('\n', "\r\n")).unwrap();
        (lf, read_text(&path).unwrap())
    };
    let (form_lf, form) = crlf_copy(".github/ISSUE_TEMPLATE/bug_report.yml");
    let (config_lf, config) = crlf_copy(".github/ISSUE_TEMPLATE/config.yml");
    assert_eq!(form, form_lf, "CRLF bug_report.yml not normalised");
    assert_eq!(config, config_lf, "CRLF config.yml not normalised");
    assert_issue_template_violations_caught(&form, &config);
}

fn assert_issue_template_violations_caught(form: &str, config: &str) {
    let (form, config) = (form.to_owned(), config.to_owned());
    assert!(issue_template_problems(&form, &config, &[]).is_empty());

    let check = |form: &str, config: &str, others: &[(String, String)], expect: &str| {
        let problems = issue_template_problems(form, config, others);
        assert!(
            problems.iter().any(|p| p.contains(expect)),
            "want `{expect}`, got {problems:?}"
        );
    };

    // Intro no longer tells users to scrub.
    let intro = form.replacen(
        "romraider-cli trace scrub <file.rrtrace>",
        "cat <file.rrtrace>",
        1,
    );
    check(&intro, &config, &[], "markdown instructions");
    // Attachments field removed / renamed.
    check(
        &form.replace("id: attachments", "id: files"),
        &config,
        &[],
        "id: attachments",
    );
    // Attachments description no longer names the scrub command.
    let attach = form.replacen(
        "Run `romraider-cli trace scrub <file>` on every",
        "Include every",
        1,
    );
    check(&attach, &config, &[], "attachments field does not tell");
    // Checkbox made optional.
    let optional = form.replacen(
        "romraider-cli log scrub` (or I attached none).\n          required: true",
        "romraider-cli log scrub` (or I attached none).\n          required: false",
        1,
    );
    assert_ne!(optional, form, "checkbox fixture text not found");
    check(&optional, &config, &[], "not `required: true`");
    // Checkbox removed.
    let no_box = form.replace("type: checkboxes", "type: textarea");
    check(&no_box, &config, &[], "no checkbox confirming");
    // Blank issues re-enabled, or the key commented out.
    check(
        &form,
        &config.replace("blank_issues_enabled: false", "blank_issues_enabled: true"),
        &[],
        "blank_issues_enabled: false",
    );
    check(
        &form,
        &config.replace(
            "blank_issues_enabled: false",
            "# blank_issues_enabled: false",
        ),
        &[],
        "blank_issues_enabled: false",
    );
    // Legacy markdown template with the Java version field reintroduced.
    let legacy = vec![(
        "issue-report.md".to_owned(),
        "---\nname: Issue report\n---\n**Java Version:**\n".to_owned(),
    )];
    check(&form, &config, &legacy, "non-form template");
    check(&form, &config, &legacy, "Java version");
}

// ---------------------------------------------------------------------------
// US-RR-8-4: CONTRIBUTING.md Definition of Done
// ---------------------------------------------------------------------------

/// The PROJECT.md anchors the Definition of Done must link.
const DOD_PROJECT_ANCHORS: [&str; 3] = [
    "testing-strategy",
    "must-fix-bugs-from-the-java-codebase",
    "edge-cases-must-have-tests",
];

/// Problems with CONTRIBUTING.md against US-RR-8-4: it has a
/// `## Definition of Done` section that links `.project/PROJECT.md`'s Testing
/// Strategy and both ledgers, and those anchors are real PROJECT.md headings.
fn dod_problems(contributing: &str, project: &str) -> Vec<String> {
    let Some(dod) = section(contributing, |lv, h| lv == 2 && h == "Definition of Done") else {
        return vec!["CONTRIBUTING.md has no `## Definition of Done` heading".to_owned()];
    };
    let mut problems = Vec::new();
    if !dod.iter().any(|l| {
        let t = l.trim_start();
        t.starts_with("1. ") || t.starts_with("- ")
    }) {
        problems.push("Definition of Done section lists no criteria".to_owned());
    }
    let targets: Vec<String> = link_targets(&dod.join("\n"))
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    let project_anchors = anchors(project);
    for anchor in DOD_PROJECT_ANCHORS {
        let want = format!(".project/PROJECT.md#{anchor}");
        if !targets.contains(&want) {
            problems.push(format!("Definition of Done does not link `{want}`"));
        }
        if !project_anchors.iter().any(|a| a == anchor) {
            problems.push(format!(
                "`#{anchor}` does not resolve to a heading in .project/PROJECT.md"
            ));
        }
    }
    problems
}

/// Problems with relative links in a markdown file at `rel` (relative to
/// `root`): each must point at an existing file, and an anchor into a
/// markdown file must match one of its headings.
fn relative_link_problems(root: &Path, rel: &str, text: &str) -> Vec<String> {
    let base = Path::new(rel).parent().unwrap_or(Path::new(""));
    let mut problems = Vec::new();
    for (n, target) in link_targets(text) {
        if target.contains("://") || target.starts_with("mailto:") {
            continue;
        }
        let (path, anchor) = match target.split_once('#') {
            Some((p, a)) => (p, Some(a)),
            None => (target.as_str(), None),
        };
        let file_rel = if path.is_empty() {
            PathBuf::from(rel)
        } else {
            base.join(path)
        };
        let file = root.join(&file_rel);
        if !file.exists() {
            problems.push(format!(
                "{rel}:{n}: `{target}` -> {} does not exist",
                file_rel.display()
            ));
            continue;
        }
        if let Some(anchor) = anchor
            && file.extension().is_some_and(|e| e == "md")
        {
            let linked = read_text(&file).unwrap_or_default();
            if !anchors(&linked).iter().any(|a| a == anchor) {
                problems.push(format!(
                    "{rel}:{n}: `{target}`: no heading `#{anchor}` in {}",
                    file_rel.display()
                ));
            }
        }
    }
    problems
}

/// Proves US-RR-8-4: the real CONTRIBUTING.md states the Definition of Done
/// and links PROJECT.md's Testing Strategy, Must-Fix and Edge-Case ledgers,
/// at anchors that resolve.
#[test]
fn contributing_states_definition_of_done_with_ledger_links() {
    let problems = dod_problems(
        &read("CONTRIBUTING.md").unwrap(),
        &read(".project/PROJECT.md").unwrap(),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// US-RR-8-4 (bonus): every relative link and anchor in CONTRIBUTING.md
/// resolves.
#[test]
fn contributing_relative_links_resolve() {
    let problems = relative_link_problems(
        &docs_root(),
        "CONTRIBUTING.md",
        &read("CONTRIBUTING.md").unwrap(),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// US-RR-8-4 violation demo: dropping the heading or a ledger link, or
/// renaming the PROJECT.md heading it points at, is reported.
#[test]
fn dod_check_catches_violations() {
    let contributing = read("CONTRIBUTING.md").unwrap();
    let project = read(".project/PROJECT.md").unwrap();
    assert!(dod_problems(&contributing, &project).is_empty());

    let check = |c: &str, p: &str, expect: &str| {
        let problems = dod_problems(c, p);
        assert!(
            problems.iter().any(|x| x.contains(expect)),
            "want `{expect}`, got {problems:?}"
        );
    };
    check(
        &contributing.replace("## Definition of Done", "## Done-ness"),
        &project,
        "no `## Definition of Done` heading",
    );
    for anchor in DOD_PROJECT_ANCHORS {
        let link = format!("(.project/PROJECT.md#{anchor})");
        assert!(contributing.contains(&link), "fixture link {link} missing");
        check(
            &contributing.replace(&link, "(.project/PROJECT.md)"),
            &project,
            &format!("does not link `.project/PROJECT.md#{anchor}`"),
        );
    }
    check(
        &contributing,
        &project.replace("## Edge Cases (must have tests)", "## Edge Cases"),
        "`#edge-cases-must-have-tests` does not resolve",
    );
    check(
        &contributing,
        &project.replace("## Testing Strategy", "## Test Strategy"),
        "`#testing-strategy` does not resolve",
    );
    // A link elsewhere in the file does not count for the DoD section.
    let moved = contributing.replacen(
        "[Must-Fix](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)",
        "Must-Fix",
        1,
    ) + "\n## Elsewhere\n\n[x](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)\n";
    check(&moved, &project, "must-fix-bugs-from-the-java-codebase");
}

/// US-RR-8-4 violation demo for the link checker: a missing file and a
/// dangling anchor are both reported, a good link is not.
#[test]
fn relative_link_check_catches_dangling_links() {
    let root = docs_root();
    let text = "[a](.project/PROJECT.md#testing-strategy)\n\
                [b](.project/NOPE.md)\n\
                [c](.project/PROJECT.md#no-such-heading)\n\
                [d](#no-such-local-heading)\n\
                [e](https://example.invalid/x#y)\n";
    let problems = relative_link_problems(&root, "CONTRIBUTING.md", text);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].contains("NOPE.md"));
    assert!(problems[1].contains("no-such-heading"));
    assert!(problems[2].contains("no-such-local-heading"));
}

/// GitHub slug rules the anchor checks rely on.
#[test]
fn github_slug_matches_github_anchors() {
    assert_eq!(
        github_slug("Must-Fix Bugs (from the Java codebase)"),
        "must-fix-bugs-from-the-java-codebase"
    );
    assert_eq!(
        github_slug("Edge Cases (must have tests)"),
        "edge-cases-must-have-tests"
    );
    assert_eq!(github_slug("No-Java rule (D-024)"), "no-java-rule-d-024");
    assert_eq!(
        github_slug("D-003: Fixture-first, multi-tier testing"),
        "d-003-fixture-first-multi-tier-testing"
    );
    assert_eq!(
        anchors("# A\n## A\n```\n# not a heading\n```\n## A\n"),
        ["a", "a-1", "a-2"]
    );
}
