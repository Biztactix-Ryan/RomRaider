//! US-RR-12-1: every area of the legacy behaviour catalogue has a
//! `docs/legacy` page with `legacy/` file:line references for each described
//! behaviour (`cargo xtask check-legacy-docs`, rules in
//! `xtask/src/legacy_docs.rs`).
//!
//! * The committed catalogue passes with the pinned configuration, and the
//!   check is not vacuous: every page has many resolved, range-checked and
//!   content-checked references and many behaviour units.
//! * On planted trees the check rejects, naming page and line: a missing area
//!   page, a README that omits a page, misnames its area or lists an extra
//!   page, an unlisted page; a reference to a missing file, past the end of
//!   the file, with a reversed range, to an ambiguous suffix, a dangling
//!   `:N` continuation, a pointer to a missing section; a behaviour
//!   paragraph, list item or table row without a reference, a `Source` cell
//!   without one; a reference whose lines are in range but shifted away from
//!   the identifier the text names (the US-RR-12-4 defect: 39 lines off);
//!   stale allowlist entries. Lead-ins, `Sources:` trailers, worked examples
//!   and the excluded sections are accepted.
//! * The binary, as CI runs it, exits non-zero on a planted tree and zero on
//!   the committed one (wiring: `legacy_docs_ci_config.rs`).
//! * US-RR-12-2 (rule 6): every committed page ends with a "Fixtures to
//!   write" table of `LEG-<PREFIX>-NN` IDs, all of them (117) mapped in
//!   `docs/legacy/fixtures-map.toml` to existing files or live tasks that name
//!   them. On planted trees the check rejects: a page whose last section is
//!   not the fixtures list, has no such section or lists no ID; a row
//!   without an ID or with another page's prefix; an ID listed twice; a
//!   mention of an unlisted ID; an ID missing from the map (a new fixture
//!   added untracked); a stale map entry; an entry with neither `kat` nor
//!   `task`; a `kat` path that does not exist or leaves the repository; a
//!   task that does not exist, is archived or does not name the ID; a
//!   missing or malformed map; a page without a prefix.
//! * US-RR-12-3 (rule 7): the committed pages cite only entries of
//!   PROJECT.md's ledger, keep the retired `UL-` IDs only in their "Ledgered
//!   issues" tables (each mapped to an entry), and every bug statement cites
//!   its entry; the ledger rows are well formed and numbered without gaps.
//!   On planted trees the check rejects: an `MF-`/`EC-` ID not in the ledger;
//!   a `UL-` ID in prose; a `Ledger` cell with no entry, a bare "not a bug",
//!   a `UL-` ID listed twice or without the page's prefix, a table without a
//!   `Ledger` column; a paragraph, list item or table row that calls legacy
//!   behaviour a bug (`silently`, `wrong`, ...) without an entry, unless it
//!   leans on a citing lead-in or parent, the word is in inline code or
//!   quotes, or a `BUG_EXEMPT` entry covers it; stale `BUG_EXEMPT` entries;
//!   a missing ledger.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::legacy_docs::{self, Config, JAVA_ROOT};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn committed_catalogue_passes_and_is_not_vacuous() {
    let report = legacy_docs::run(&workspace_root(), &Config::pinned()).unwrap();
    assert!(
        report.problems.is_empty(),
        "docs/legacy fails check-legacy-docs:\n{}",
        report.problems.join("\n")
    );
    assert_eq!(report.pages.len(), legacy_docs::AREAS.len());
    for (page, stats) in &report.pages {
        assert!(stats.resolved >= 50, "{page}: {stats:?}");
        assert!(stats.content_checked >= 15, "{page}: {stats:?}");
        assert!(stats.pinned >= 8, "{page}: {stats:?}");
        assert!(stats.units >= 40, "{page}: {stats:?}");
        assert_eq!(stats.units, stats.referenced_units, "{page}: {stats:?}");
        assert!(stats.fixtures >= 10, "{page}: {stats:?}");
    }
    // Rule 6 is on for the committed configuration, for every page, and
    // every listed fixture is mapped.
    for (_, page, _) in legacy_docs::AREAS {
        assert!(
            legacy_docs::FIXTURE_PREFIXES.iter().any(|(p, _)| p == page),
            "{page} has no fixture prefix"
        );
    }
    let f = &report.fixtures;
    let listed: usize = report.pages.values().map(|s| s.fixtures).sum();
    assert_eq!(f.ids, listed, "{f:?}");
    assert!(f.ids >= 100, "{f:?}");
    assert_eq!(f.kat_only + f.task_only + f.both, f.ids, "{f:?}");
    assert!(
        f.kat_only >= 5 && f.task_only >= 50 && f.tasks >= 10,
        "{f:?}"
    );
}

/// A Java file of `n` lines, with `marker` on line `at`.
fn java(n: usize, marks: &[(usize, &str)]) -> String {
    (1..=n)
        .map(|i| {
            marks
                .iter()
                .find(|(at, _)| *at == i)
                .map_or_else(|| format!("    // line {i}\n"), |(_, m)| format!("{m}\n"))
        })
        .collect()
}

const AREAS: &[(&str, &str, &str)] = &[
    ("first area", "one.md", "first"),
    ("second area", "two.md", "second"),
];

const README: &str = "# Index\n\n## Pages\n\n| Page | Area |\n|------|------|\n| [`one.md`](one.md) | The first area |\n| [`two.md`](two.md) | The second area |\n";

const PAGE_HEAD: &str = "# Page\n\nIntro without references.\n\n## How to read this page\n\nConventions, no references.\n\n### Source files\n\n| Alias | Role |\n|---|---|\n| `maps/Codec.java` | codec |\n\n";

/// A page that passes: references in prose, lists, tables, lead-ins,
/// trailers, continuations, section pointers, worked examples and excluded
/// sections.
fn good_page() -> String {
    format!(
        "{PAGE_HEAD}## 1. Reading

`Codec.readValue` reads the cell (`maps/Codec.java:10-12`), then
`Codec.writeValue` writes it back (`:30`).

Steps (`maps/Codec.java:10-30`):

1. Read.
2. Write.

| What | Source |
|------|--------|
| read | `maps/Codec.java:10` |

| Code | Meaning |
|------|---------|
| 1 | one |

Sources: `maps/Codec.java:30`.

As section 1 says, and as (1.1) adds.

### 1.1 Notes

- `readValue` (`maps/Codec.java:11`) and the notes (`legacy/docs/notes.txt:2`).
- Corpus line (`logger_STD_EN_v290.xml:5`).

*Worked example (hand-computed).* No reference needed.

- `1 + 1` = 2.

## 2. Mapping to the port

Port guidance, no references.

## Ledgered issues

Notes on the ledger without references.

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-ONE-01 | MF-01 | a bug | `maps/Codec.java:30` |

## Fixtures to write

- A fixture, no reference.

```text
code, no reference
```
"
    )
}

/// Writes a catalogue tree under the target dir and returns its root.
fn planted(name: &str, pages: &[(&str, &str)], readme: &str) -> io::Result<PathBuf> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("check-legacy-docs")
        .join(name);
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    let java_root = root.join(JAVA_ROOT);
    fs::create_dir_all(java_root.join("maps"))?;
    fs::create_dir_all(java_root.join("a"))?;
    fs::create_dir_all(java_root.join("b"))?;
    fs::create_dir_all(root.join("legacy/docs"))?;
    fs::create_dir_all(root.join("docs/legacy"))?;
    fs::write(
        java_root.join("maps/Codec.java"),
        java(
            60,
            &[
                (10, "    public int readValue() {"),
                (30, "    public void writeValue(int v) {"),
                (50, "    private int unrelatedHelper() {"),
            ],
        ),
    )?;
    fs::write(java_root.join("a/Dup.java"), java(5, &[]))?;
    fs::write(java_root.join("b/Dup.java"), java(5, &[]))?;
    fs::write(root.join("legacy/docs/notes.txt"), "one\ntwo\nthree\n")?;
    fs::write(root.join("docs/legacy/README.md"), readme)?;
    for (page, text) in pages {
        fs::write(root.join("docs/legacy").join(page), text)?;
    }
    Ok(root)
}

fn config() -> Config<'static> {
    Config {
        areas: AREAS,
        aliases: &[],
        external: &["logger_STD_EN_v290.xml"],
        exempt_units: &[],
        content_exempt: &[],
        pinned_anchors: &[],
        fixture_prefixes: &[],
        ledger: None,
        bug_words: &[],
        bug_exempt: &[],
    }
}

/// The problems of a planted tree with `one` as `one.md` (an I/O error is
/// returned as a problem, which fails the caller's assertions).
fn problems_with(name: &str, one: &str, cfg: &Config<'_>) -> Vec<String> {
    let good = good_page();
    planted(name, &[("one.md", one), ("two.md", &good)], README)
        .map_err(anyhow::Error::from)
        .and_then(|root| legacy_docs::run(&root, cfg))
        .map_or_else(|e| vec![format!("error: {e:#}")], |r| r.problems)
}

fn problems(name: &str, one: &str) -> Vec<String> {
    problems_with(name, one, &config())
}

fn assert_flags(problems: &[String], needle: &str) {
    assert!(
        problems.iter().any(|p| p.contains(needle)),
        "expected a problem mentioning {needle:?}, got {problems:#?}"
    );
}

#[test]
fn planted_good_catalogue_passes() {
    let good = good_page();
    let root = planted("good", &[("one.md", &good), ("two.md", &good)], README).unwrap();
    let cfg = Config {
        pinned_anchors: &[("one.md", "maps/Codec.java:10-12", "readValue")],
        ..config()
    };
    let report = legacy_docs::run(&root, &cfg).unwrap();
    assert!(report.problems.is_empty(), "{:#?}", report.problems);
    let one = report.pages.get("one.md").unwrap();
    assert_eq!(one.pinned, 1);
    // 11 behaviour units: 2 paragraphs, 2 + 2 items, 2 + 1 rows, the pointer
    // paragraph, the lead-in; each referenced.
    assert_eq!(one.units, one.referenced_units, "{one:?}");
    assert!(one.units >= 10, "{one:?}");
    // `readValue` / `writeValue` are named next to four references.
    assert!(one.content_checked >= 3, "{one:?}");
}

#[test]
fn rejects_missing_and_misindexed_pages() {
    let good = good_page();
    let root = planted("no-page", &[("one.md", &good)], README).unwrap();
    let p = legacy_docs::run(&root, &config()).unwrap().problems;
    assert_flags(
        &p,
        "docs/legacy/two.md: missing (the page for area \"second area\")",
    );

    let readme = README.replace("| [`two.md`](two.md) | The second area |\n", "");
    let root = planted(
        "readme-omits",
        &[("one.md", &good), ("two.md", &good)],
        &readme,
    )
    .unwrap();
    let p = legacy_docs::run(&root, &config()).unwrap().problems;
    assert_flags(
        &p,
        "Pages table does not list two.md (area \"second area\")",
    );

    let readme = README.replace("The second area", "Something else");
    let root = planted(
        "readme-area",
        &[("one.md", &good), ("two.md", &good)],
        &readme,
    )
    .unwrap();
    let p = legacy_docs::run(&root, &config()).unwrap().problems;
    assert_flags(&p, "the area cell of two.md does not name its area");

    let readme = format!("{README}| [`three.md`](three.md) | A third |\n");
    let root = planted(
        "extra",
        &[("one.md", &good), ("two.md", &good), ("three.md", &good)],
        &readme,
    )
    .unwrap();
    let p = legacy_docs::run(&root, &config()).unwrap().problems;
    assert_flags(&p, "Pages table lists three.md, which maps to no area");
    assert_flags(&p, "docs/legacy/three.md: a page that maps to no area");

    let root = planted(
        "no-index",
        &[("one.md", &good), ("two.md", &good)],
        "# Index\n",
    )
    .unwrap();
    let p = legacy_docs::run(&root, &config()).unwrap().problems;
    assert_flags(&p, "no \"## Pages\" table");
}

#[test]
fn rejects_unresolvable_references() {
    let good = good_page();
    for (name, from, to, needle) in [
        (
            "missing-file",
            "`maps/Codec.java:10-12`",
            "`maps/Codecs.java:10-12`",
            "one.md:17: `maps/Codecs.java:10-12`: no such file",
        ),
        (
            "past-end",
            "`maps/Codec.java:10-12`",
            "`maps/Codec.java:10-61`",
            "one.md:17: `maps/Codec.java:10-61`: line 61 is past the end of",
        ),
        (
            "reversed",
            "`maps/Codec.java:10-12`",
            "`maps/Codec.java:12-10`",
            "`maps/Codec.java:12-10`: malformed line range 12-10",
        ),
        (
            "zero",
            "`maps/Codec.java:10-12`",
            "`maps/Codec.java:0`",
            "`maps/Codec.java:0`: malformed line range 0-0",
        ),
        (
            "ambiguous",
            "`maps/Codec.java:10-12`",
            "`Dup.java:3`",
            "`Dup.java:3`: ambiguous, Dup.java could be any of",
        ),
        (
            "rooted-missing",
            "`legacy/docs/notes.txt:2`",
            "`legacy/docs/nope.txt:2`",
            "`legacy/docs/nope.txt:2`: no such file",
        ),
        (
            "rooted-past-end",
            "`legacy/docs/notes.txt:2`",
            "`legacy/docs/notes.txt:4`",
            "line 4 is past the end of legacy/docs/notes.txt (3 lines)",
        ),
        (
            "dangling-continuation",
            "`maps/Codec.java:10-12`), then\n`Codec.writeValue` writes it back (`:30`)",
            "no reference), then\n`Codec.writeValue` writes it back (`:30`)",
            "`:30` continues a reference, but no file reference precedes it",
        ),
        (
            "missing-section",
            "As section 1 says",
            "As section 7 says",
            "points to section 7, which this page does not have",
        ),
    ] {
        let one = good.replace(from, to);
        assert_ne!(one, good, "{name}: mutation did not apply");
        let p = problems(name, &one);
        assert_flags(&p, needle);
    }
    // A comma list is range-checked item by item.
    let one = good.replace("(`:30`)", "(`maps/Codec.java:30,61`)");
    assert_flags(&problems("list-past-end", &one), "line 61 is past the end");
}

#[test]
fn rejects_behaviour_without_references() {
    let good = good_page();
    for (name, from, to, needle) in [
        (
            "paragraph",
            "As section 1 says, and as (1.1) adds.",
            "Legacy also does something unreferenced.",
            "paragraph describes behaviour with no legacy file:line reference",
        ),
        (
            "item",
            "- Corpus line (`logger_STD_EN_v290.xml:5`).",
            "- An unreferenced claim.",
            "list item describes behaviour with no legacy file:line reference",
        ),
        (
            "source-cell",
            "| read | `maps/Codec.java:10` |",
            "| read `maps/Codec.java:10` | see above |",
            "table row (its Source cell) describes behaviour",
        ),
        (
            "row-without-trailer",
            "Sources: `maps/Codec.java:30`.",
            "Sources: none.",
            "table row describes behaviour",
        ),
        (
            "lead-in-without-ref",
            "Steps (`maps/Codec.java:10-30`):",
            "Steps:",
            "list item describes behaviour",
        ),
        (
            "ledgered-row",
            "| UL-ONE-01 | MF-01 | a bug | `maps/Codec.java:30` |",
            "| UL-ONE-01 | MF-01 | a bug | none |",
            "table row (its Source cell) describes behaviour",
        ),
        (
            // A repo file is a reference, but not a legacy one.
            "repo-ref-only",
            "- Corpus line (`logger_STD_EN_v290.xml:5`).",
            "- Only the port (`docs/legacy/one.md:1`).",
            "list item describes behaviour with no legacy file:line reference",
        ),
    ] {
        let one = good.replace(from, to);
        assert_ne!(one, good, "{name}: mutation did not apply");
        assert_flags(&problems(name, &one), needle);
    }
}

#[test]
fn rejects_shifted_references_that_stay_in_range() {
    let good = good_page();
    // The US-RR-12-4 defect: every line in range, all shifted off the code.
    let one = good.replace("(`maps/Codec.java:10-12`)", "(`maps/Codec.java:49-51`)");
    let p = problems("shifted", &one);
    assert_flags(
        &p,
        "one.md:17: `maps/Codec.java:49-51`: the cited lines of legacy/src/main/java/com/romraider/maps/Codec.java do not mention what the text names next to it (`readvalue` at 10)",
    );
    // A continuation is checked against its file too.
    let one = good.replace("(`:30`)", "(`:12`)");
    assert_flags(
        &problems("shifted-continuation", &one),
        "`:12`: the cited lines",
    );
    // Within the slack, or in the declaration the cited body sits in: fine.
    let one = good.replace("(`maps/Codec.java:10-12`)", "(`maps/Codec.java:12-14`)");
    assert!(problems("slack", &one).is_empty());
    let one = good.replace("(`maps/Codec.java:10-12`)", "(`maps/Codec.java:20-25`)");
    assert!(problems("in-declaration", &one).is_empty());
    // An identifier the file never mentions does not anchor anything.
    let one = good.replace(
        "`Codec.readValue` reads the cell (`maps/Codec.java:10-12`)",
        "`elsewhereOnly` reads the cell (`maps/Codec.java:49-51`)",
    );
    assert!(problems("no-anchor", &one).is_empty());
}

#[test]
fn allowlists_apply_and_go_stale() {
    let good = good_page();
    let unref = good.replace(
        "As section 1 says, and as (1.1) adds.",
        "Legacy also does something unreferenced.",
    );
    let shifted = unref.replace("(`maps/Codec.java:10-12`)", "(`maps/Codec.java:49-51`)");
    let cfg = Config {
        exempt_units: &[("one.md", "Legacy also does", "planted")],
        content_exempt: &[("one.md", "maps/Codec.java:49-51", "planted")],
        ..config()
    };
    let p = problems_with("allowlisted", &shifted, &cfg);
    assert!(p.is_empty(), "{p:#?}");
    let p = problems_with("stale", &good, &cfg);
    assert_flags(
        &p,
        "EXEMPT_UNITS entry \"Legacy also does\" matches no unreferenced",
    );
    assert_flags(
        &p,
        "CONTENT_EXEMPT entry \"maps/Codec.java:49-51\" matches no failing",
    );
}

#[test]
fn cli_fails_on_a_planted_tree_and_passes_on_the_committed_one() -> io::Result<()> {
    // Every pinned page, each one line with a reference past the end.
    let bad = "# P\n\n## 1. A\n\nReads (`maps/Codec.java:99`).\n";
    let pages: Vec<(&str, &str)> = legacy_docs::AREAS
        .iter()
        .map(|(_, p, _)| (*p, bad))
        .collect();
    let rows: String = legacy_docs::AREAS
        .iter()
        .map(|(_, p, k)| format!("| [`{p}`]({p}) | {k} |\n"))
        .collect();
    let readme = format!("## Pages\n\n| Page | Area |\n|---|---|\n{rows}");
    let root = planted("cli", &pages, &readme)?;
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check-legacy-docs", "--root"])
        .arg(&root)
        .output()?;
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(
        err.contains("docs/legacy/ssm.md:5: `maps/Codec.java:99`: line 99 is past the end"),
        "{err}"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-legacy-docs")
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("check-legacy-docs: ok (7 page(s)"),
        "{stdout}"
    );
    assert!(stdout.contains("fixture ID(s) mapped"), "{stdout}");

    // A missing catalogue fails closed.
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check-legacy-docs", "--root"])
        .arg(Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-root"))
        .output()?;
    assert!(!out.status.success());
    Ok(())
}

#[test]
fn pinned_anchors_must_hold() {
    let good = good_page();
    // The pin's needle is not in the cited lines.
    let cfg = Config {
        pinned_anchors: &[("one.md", "maps/Codec.java:10-12", "writeValue")],
        ..config()
    };
    assert_flags(
        &problems_with("pin-needle", &good, &cfg),
        "one.md: `maps/Codec.java:10-12`: the cited lines of legacy/src/main/java/com/romraider/maps/Codec.java do not contain the pinned anchor \"writeValue\" (it is at line(s) 30)",
    );
    // The pinned reference was edited on the page: re-pin.
    let cfg = Config {
        pinned_anchors: &[("one.md", "maps/Codec.java:10-12", "readValue")],
        ..config()
    };
    let one = good.replace("(`maps/Codec.java:10-12`)", "(`maps/Codec.java:10-13`)");
    assert_flags(
        &problems_with("pin-gone", &one, &cfg),
        "PINNED_ANCHORS reference `maps/Codec.java:10-12` is no longer on the page",
    );
}

/// The US-RR-12-4 defect, replayed on the committed pages: every reference
/// of a page moved by 39 lines (either way) makes the check fail, through
/// range, content or pinned-anchor problems, on many references at once;
/// and every `iso9141/SSMResponseProcessor.java` reference of `ssm.md`, moved
/// 39 lines (as they once were), fails on its own.
#[test]
fn committed_pages_with_shifted_references_fail() {
    let root = workspace_root();
    let cfg = Config::pinned();
    for (_, page, _) in legacy_docs::AREAS {
        let text = fs::read_to_string(root.join("docs/legacy").join(page)).unwrap();
        let (base, stats) = legacy_docs::check_page(&root, page, &text, &cfg).unwrap();
        assert!(base.is_empty(), "{page}: {base:#?}");
        for d in [39, -39] {
            let shifted = shift_refs(&text, d, |_| true);
            let (p, _) = legacy_docs::check_page(&root, page, &shifted, &cfg).unwrap();
            // Out of range, content or pin problems: at least a third as
            // many as there are content-checked references, and at least 10.
            assert!(
                p.len() * 3 >= stats.content_checked && p.len() >= 10,
                "{page} shifted by {d}: {} problem(s) for {} content-checked references",
                p.len(),
                stats.content_checked
            );
        }
    }
    let text = fs::read_to_string(root.join("docs/legacy/ssm.md")).unwrap();
    let shifted = shift_refs(&text, 39, |p| {
        p.ends_with("iso9141/SSMResponseProcessor.java")
    });
    let root_tmp = planted_copy("ssm-shifted", "ssm.md", &shifted).unwrap();
    let report = legacy_docs::run(&root_tmp, &cfg).unwrap();
    let pins = legacy_docs::PINNED_ANCHORS
        .iter()
        .filter(|(p, r, _)| *p == "ssm.md" && r.starts_with("iso9141/SSMResponseProcessor.java:"))
        .count();
    let occurrences: std::collections::BTreeSet<&str> = text
        .match_indices("iso9141/SSMResponseProcessor.java:")
        .map(|(i, _)| {
            let rest = &text[i..];
            let end = rest.find('`').unwrap_or(rest.len());
            &rest[..end]
        })
        .collect();
    assert_eq!(
        pins,
        occurrences.len(),
        "pin every SSMResponseProcessor reference: {occurrences:?}"
    );
    // Each moved reference is reported: out of range, or its pin no longer
    // on the page.
    let flagged = report
        .problems
        .iter()
        .filter(|p| p.contains("SSMResponseProcessor.java:"))
        .count();
    assert!(
        flagged >= pins,
        "{flagged} < {pins}: {:#?}",
        report.problems
    );
}

/// A copy of the committed catalogue with `page` replaced by `text`, under
/// a root whose `legacy` is the committed tree (copied: no symlinks, so it
/// works on Windows too). Only the files the pages cite are copied.
fn planted_copy(name: &str, page: &str, text: &str) -> io::Result<PathBuf> {
    let src = workspace_root();
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("check-legacy-docs")
        .join(name);
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    copy_dir(&src.join("legacy"), &root.join("legacy"))?;
    copy_dir(&src.join("docs/legacy"), &root.join("docs/legacy"))?;
    fs::create_dir_all(root.join(".project"))?;
    fs::copy(
        src.join(legacy_docs::ledger::LEDGER),
        root.join(legacy_docs::ledger::LEDGER),
    )?;
    for top in ["crates", "fixtures"] {
        fs::create_dir_all(root.join(top))?;
    }
    fs::write(root.join("docs/legacy").join(page), text)?;
    Ok(root)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Adds `d` to every line number of every reference whose path (`X.java`,
/// or a page alias such as `Unmarshaller`) satisfies `which`.
fn shift_refs(text: &str, d: i64, which: impl Fn(&str) -> bool) -> String {
    let mut out = String::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        out.push(c as char);
        if c == b':' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let start = text[..i]
                .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || "_./-".contains(ch)))
                .map_or(0, |k| k + 1);
            let path = &text[start..i];
            let is_ref = path.ends_with(".java") || path == "Unmarshaller" || path == "Builder";
            if is_ref && which(path) {
                let mut j = i + 1;
                loop {
                    let st = j;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    let n: i64 = text[st..j].parse().unwrap_or(0);
                    out.push_str(&(n + d).max(1).to_string());
                    if j + 1 < b.len()
                        && (b[j] == b'-' || b[j] == b',')
                        && b[j + 1].is_ascii_digit()
                    {
                        out.push(b[j] as char);
                        j += 1;
                    } else {
                        break;
                    }
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Rule 6: fixtures to write (US-RR-12-2).

const PREFIXES: &[(&str, &str)] = &[("one.md", "ONE"), ("two.md", "TWO")];

fn fx_config() -> Config<'static> {
    Config {
        fixture_prefixes: PREFIXES,
        ..config()
    }
}

/// `good_page()` with its fixtures section replaced by `section`.
fn with_fixtures(section: &str) -> String {
    let base = good_page();
    let head = base
        .find("## Fixtures to write")
        .map_or(base.as_str(), |i| &base[..i]);
    format!("{head}{section}")
}

fn fixtures_table(rows: &str) -> String {
    format!(
        "## Fixtures to write\n\nStable IDs.\n\n| ID | Kind | What it pins |\n|----|------|--------------|\n{rows}\n```text\n| not | a row |\n```\n"
    )
}

const ONE_ROWS: &str = "| LEG-ONE-01 | KAT `a.toml` | pinned by a file |\n| LEG-ONE-02 | Test | tracked by a task; shares vectors with LEG-TWO-01 |\n";
const TWO_ROWS: &str =
    "| LEG-TWO-01 | KAT | a file |\n| LEG-TWO-02 | KAT + test | a file and a task |\n";

const MAP: &str = "# map\n\n[LEG-ONE-01]\nkat = [\"fixtures/kat/a.toml\"]\n\n[LEG-ONE-02]\ntask = \"US-XX-1-1\"\n\n[LEG-TWO-01]\nkat = [\"fixtures/kat/b.toml\", \"crates/x/tests/t.rs\"]\n\n[LEG-TWO-02]\nkat = [\"fixtures/kat/b.toml\"]\ntask = \"US-XX-2-1\"\n";

const TASK_1: &str = "---\narchived: false\nid: US-XX-1-1\ntitle: Write fixture LEG-ONE-02\n---\n\nWrite fixtures LEG-ONE-02: what it pins.\n";
const TASK_2: &str = "---\narchived: false\nid: US-XX-2-1\ntitle: Finish LEG-TWO-02\n---\n\nThe rest of LEG-TWO-02.\n";

/// A planted tree with `one` and `two` as pages, `map` as the fixture map
/// (none when `None`), and `tasks` as `.project/tasks` files.
fn fx_planted(
    name: &str,
    one: &str,
    two: &str,
    map: Option<&str>,
    tasks: &[(&str, &str)],
) -> io::Result<PathBuf> {
    let root = planted(name, &[("one.md", one), ("two.md", two)], README)?;
    for f in [
        "fixtures/kat/a.toml",
        "fixtures/kat/b.toml",
        "crates/x/tests/t.rs",
    ] {
        let p = root.join(f);
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(p, "x\n")?;
    }
    if let Some(map) = map {
        fs::write(root.join("docs/legacy/fixtures-map.toml"), map)?;
    }
    fs::create_dir_all(root.join(".project/tasks"))?;
    for (id, body) in tasks {
        fs::write(root.join(".project/tasks").join(format!("{id}.md")), body)?;
    }
    Ok(root)
}

const TASKS: &[(&str, &str)] = &[("US-XX-1-1", TASK_1), ("US-XX-2-1", TASK_2)];

fn fx_problems_with(
    name: &str,
    one_rows: &str,
    map: Option<&str>,
    tasks: &[(&str, &str)],
    cfg: &Config<'_>,
) -> Vec<String> {
    let one = with_fixtures(&fixtures_table(one_rows));
    let two = with_fixtures(&fixtures_table(TWO_ROWS));
    fx_planted(name, &one, &two, map, tasks)
        .map_err(anyhow::Error::from)
        .and_then(|root| legacy_docs::run(&root, cfg))
        .map_or_else(|e| vec![format!("error: {e:#}")], |r| r.problems)
}

fn fx_problems(name: &str, one_rows: &str, map: &str) -> Vec<String> {
    fx_problems_with(name, one_rows, Some(map), TASKS, &fx_config())
}

#[test]
fn planted_fixture_map_passes_and_counts() {
    let one = with_fixtures(&fixtures_table(ONE_ROWS));
    let two = with_fixtures(&fixtures_table(TWO_ROWS));
    let root = fx_planted("fx-good", &one, &two, Some(MAP), TASKS).unwrap();
    let report = legacy_docs::run(&root, &fx_config()).unwrap();
    assert!(report.problems.is_empty(), "{:#?}", report.problems);
    let f = &report.fixtures;
    assert_eq!(
        (f.ids, f.kat_only, f.task_only, f.both, f.tasks),
        (4, 2, 1, 1, 2),
        "{f:?}"
    );
    assert_eq!(report.pages.get("one.md").unwrap().fixtures, 2);
    // Rule 6 is off without prefixes: the plain planted pages pass.
    let good = good_page();
    let root = planted("fx-off", &[("one.md", &good), ("two.md", &good)], README).unwrap();
    let report = legacy_docs::run(&root, &config()).unwrap();
    assert!(report.problems.is_empty(), "{:#?}", report.problems);
}

#[test]
fn rejects_untracked_and_stale_fixture_ids() {
    // A new fixture added to a page but not to the map.
    let rows = format!("{ONE_ROWS}| LEG-ONE-03 | KAT | new |\n");
    assert_flags(
        &fx_problems("fx-untracked", &rows, MAP),
        "LEG-ONE-03 is not in docs/legacy/fixtures-map.toml (map it to its KAT file or a tracked task)",
    );
    // A map entry for an ID no page lists.
    let map = format!("{MAP}\n[LEG-ONE-09]\ntask = \"US-XX-1-1\"\n");
    assert_flags(
        &fx_problems("fx-stale", ONE_ROWS, &map),
        "docs/legacy/fixtures-map.toml: [LEG-ONE-09] is on no page's \"Fixtures to write\" list (stale: remove it)",
    );
    // An entry with neither kat nor task.
    let map = MAP.replace("task = \"US-XX-1-1\"\n", "kat = []\n");
    assert_flags(
        &fx_problems("fx-empty-entry", ONE_ROWS, &map),
        "[LEG-ONE-02] has neither `kat` nor `task`",
    );
}

#[test]
fn rejects_mappings_to_missing_files_and_tasks() {
    let map = MAP.replace("fixtures/kat/a.toml", "fixtures/kat/missing.toml");
    assert_flags(
        &fx_problems("fx-missing-kat", ONE_ROWS, &map),
        "[LEG-ONE-01]: kat \"fixtures/kat/missing.toml\" does not exist",
    );
    // A directory is not a fixture file.
    let map = MAP.replace("fixtures/kat/a.toml", "fixtures/kat");
    assert_flags(
        &fx_problems("fx-dir-kat", ONE_ROWS, &map),
        "kat \"fixtures/kat\" does not exist",
    );
    for bad in [
        "../outside.toml",
        "/etc/passwd",
        "fixtures\\\\kat\\\\a.toml",
    ] {
        let map = MAP.replace("fixtures/kat/a.toml", bad);
        assert_flags(
            &fx_problems("fx-bad-path", ONE_ROWS, &map),
            "is not a plain repository-relative path",
        );
    }
    let map = MAP.replace("US-XX-1-1", "US-XX-9-9");
    assert_flags(
        &fx_problems("fx-missing-task", ONE_ROWS, &map),
        "[LEG-ONE-02]: task US-XX-9-9 does not exist (.project/tasks/US-XX-9-9.md)",
    );
    let map = MAP.replace("US-XX-1-1", "../../x");
    assert_flags(
        &fx_problems("fx-bad-task", ONE_ROWS, &map),
        "task ../../x is not a task ID",
    );
    // The task exists but is about something else.
    let tasks = [
        (
            "US-XX-1-1",
            "---\ntitle: Other work\n---\n\nWrite LEG-ONE-020.\n",
        ),
        ("US-XX-2-1", TASK_2),
    ];
    assert_flags(
        &fx_problems_with("fx-task-unnamed", ONE_ROWS, Some(MAP), &tasks, &fx_config()),
        "[LEG-ONE-02]: task US-XX-1-1 does not name LEG-ONE-02 in its title or body",
    );
    let archived = TASK_1.replace("archived: false", "archived: true");
    let tasks = [("US-XX-1-1", archived.as_str()), ("US-XX-2-1", TASK_2)];
    assert_flags(
        &fx_problems_with(
            "fx-task-archived",
            ONE_ROWS,
            Some(MAP),
            &tasks,
            &fx_config(),
        ),
        "task US-XX-1-1 is archived",
    );
    // No map, and a malformed one.
    assert_flags(
        &fx_problems_with("fx-no-map", ONE_ROWS, None, TASKS, &fx_config()),
        "docs/legacy/fixtures-map.toml: unreadable",
    );
    let map = MAP.replace(
        "kat = [\"fixtures/kat/a.toml\"]",
        "kats = [\"fixtures/kat/a.toml\"]",
    );
    assert_flags(
        &fx_problems("fx-bad-map", ONE_ROWS, &map),
        "docs/legacy/fixtures-map.toml: not a valid fixture map",
    );
}

#[test]
fn rejects_pages_without_a_final_fixtures_list() {
    let fx = |name: &str, one: &str| {
        let two = with_fixtures(&fixtures_table(TWO_ROWS));
        fx_planted(name, one, &two, Some(MAP), TASKS)
            .map_err(anyhow::Error::from)
            .and_then(|root| legacy_docs::run(&root, &fx_config()))
            .map_or_else(|e| vec![format!("error: {e:#}")], |r| r.problems)
    };
    // A section after the fixtures list.
    let one = format!(
        "{}\n## Appendix\n\nMore.\n",
        with_fixtures(&fixtures_table(ONE_ROWS))
    );
    assert_flags(
        &fx("fx-not-last", &one),
        "the page ends with section \"Appendix\", not \"Fixtures to write\"",
    );
    // A renamed heading; a `## ` inside a code block does not count.
    let one =
        with_fixtures(&fixtures_table(ONE_ROWS)).replace("## Fixtures to write", "## Fixtures");
    assert_flags(
        &fx("fx-renamed", &one),
        "the page ends with section \"Fixtures\"",
    );
    let one = format!(
        "{}\n```text\n## Not a heading\n```\n",
        with_fixtures(&fixtures_table(ONE_ROWS))
    );
    let p = fx("fx-fenced", &one);
    assert!(p.is_empty(), "{p:#?}");
    // No H2 at all.
    assert_flags(
        &fx("fx-none", "# Page\n\nNothing.\n"),
        "docs/legacy/one.md: no \"## Fixtures to write\" section",
    );
    // A list instead of a table of IDs.
    let one = with_fixtures("## Fixtures to write\n\n- a fixture without an ID\n");
    assert_flags(
        &fx("fx-no-ids", &one),
        "\"Fixtures to write\" lists no fixture ID (a table of LEG-ONE-NN rows)",
    );
}

#[test]
fn rejects_malformed_and_duplicate_fixture_ids() {
    let rows = format!("{ONE_ROWS}| Write more | KAT | no ID |\n");
    assert_flags(
        &fx_problems("fx-no-id", &rows, MAP),
        "fixtures row \"Write more\" does not start with a stable ID LEG-ONE-NN",
    );
    let rows = format!("{ONE_ROWS}| LEG-TWO-05 | KAT | wrong page |\n");
    assert_flags(
        &fx_problems("fx-prefix", &rows, MAP),
        "LEG-TWO-05 does not use this page's prefix LEG-ONE",
    );
    let rows = format!("{ONE_ROWS}| LEG-ONE-01 | KAT | again |\n");
    assert_flags(
        &fx_problems("fx-dup", &rows, MAP),
        "LEG-ONE-01 is listed twice (also at docs/legacy/one.md:",
    );
    // A mention of an ID no list defines (a typo, or a removed fixture).
    let rows = ONE_ROWS.replace("LEG-TWO-01", "LEG-TWO-07");
    assert_flags(
        &fx_problems("fx-mention", &rows, MAP),
        "LEG-TWO-07 is not an ID of any page's \"Fixtures to write\" list",
    );
    // A page with no prefix.
    let cfg = Config {
        fixture_prefixes: &[("one.md", "ONE")],
        ..config()
    };
    assert_flags(
        &fx_problems_with("fx-no-prefix", ONE_ROWS, Some(MAP), TASKS, &cfg),
        "docs/legacy/two.md: no FIXTURE_PREFIXES entry",
    );
}

/// The committed catalogue with one new, unmapped fixture ID fails; the
/// committed map names only live tasks and existing files (checked by the
/// committed-catalogue test through `run`).
#[test]
fn committed_catalogue_with_an_untracked_fixture_fails() {
    let root = workspace_root();
    let text = fs::read_to_string(root.join("docs/legacy/csv.md")).unwrap();
    let added = format!(
        "{}\n| LEG-CSV-99 | Test | an untracked fixture | 1 |\n",
        text.trim_end()
    );
    let tmp = planted_copy("fx-committed", "csv.md", &added).unwrap();
    let report = legacy_docs::run(&tmp, &Config::pinned()).unwrap();
    assert_flags(
        &report.problems,
        "LEG-CSV-99 is not in docs/legacy/fixtures-map.toml",
    );
}

// ---------------------------------------------------------------------------
// Rule 7: bugs cite the ledger (US-RR-12-3).

const LEDGER_TEXT: &str = "# Project\n\n## Must-Fix Bugs\n\n| ID | Pri | Area | Java behaviour (where) | Required behaviour | Proving test |\n|----|-----|------|------|------|------|\n| MF-01 | P0 | Codec | Writes garbage. | Does not. | T1 |\n| MF-02 | P1 | Codec | Reads zero. | Does not. | T1 |\n\n## Edge Cases\n\n- EC-01 Overlapping cells [T1]\n- EC-19a Linux port access denied [T0]\n";

fn ledger_config() -> Config<'static> {
    Config {
        ledger: Some(legacy_docs::ledger::LEDGER),
        bug_words: legacy_docs::ledger::BUG_WORDS,
        ..config()
    }
}

/// `good_page()` plus a section of bug statements that pass rule 7.
fn ledger_page() -> String {
    good_page().replace(
        "## 2. Mapping to the port",
        "## 1a. Bugs

Legacy silently writes zero here (`maps/Codec.java:30`; MF-02).

The write is wrong in two ways (`maps/Codec.java:30`, MF-01):

- the first item is wrong;
- the second is corrupt.

Bit order, EC-19a and EC-01 (`maps/Codec.java:30`).

The `silentlyIgnored` flag and the popup \"Value is wrong\" (`maps/Codec.java:30`).

## 2. Mapping to the port",
    )
}

/// The problems of a planted tree with `one` as `one.md`, the good ledger
/// page as `two.md` and `ledger` as PROJECT.md (`None`: no ledger file).
fn ledger_problems_with(
    name: &str,
    one: &str,
    ledger: Option<&str>,
    cfg: &Config<'_>,
) -> Vec<String> {
    let two = ledger_page();
    planted(name, &[("one.md", one), ("two.md", &two)], README)
        .and_then(|root| {
            if let Some(text) = ledger {
                fs::create_dir_all(root.join(".project"))?;
                fs::write(root.join(legacy_docs::ledger::LEDGER), text)?;
            }
            Ok(root)
        })
        .map_err(anyhow::Error::from)
        .and_then(|root| legacy_docs::run(&root, cfg))
        .map_or_else(|e| vec![format!("error: {e:#}")], |r| r.problems)
}

fn ledger_problems(name: &str, one: &str) -> Vec<String> {
    ledger_problems_with(name, one, Some(LEDGER_TEXT), &ledger_config())
}

#[test]
fn ledger_ids_are_read_from_the_tables_and_the_edge_case_list() {
    let (mf, ec) = legacy_docs::ledger::ledger_ids(LEDGER_TEXT);
    assert_eq!(mf.into_iter().collect::<Vec<_>>(), ["MF-01", "MF-02"]);
    assert_eq!(ec.into_iter().collect::<Vec<_>>(), ["EC-01", "EC-19a"]);
    // A mention inside another row or item is not an entry.
    let (mf, ec) = legacy_docs::ledger::ledger_ids("| x | see MF-07 |\n- see EC-04\n");
    assert!(mf.is_empty() && ec.is_empty());
    let words = legacy_docs::ledger::BUG_WORDS;
    assert_eq!(
        legacy_docs::ledger::bug_word("It silently drops", words),
        Some("silently")
    );
    assert_eq!(
        legacy_docs::ledger::bug_word("Corrupts the image", words),
        Some("corrupt")
    );
    // Word starts only; not in inline code or quotes.
    assert_eq!(
        legacy_docs::ledger::bug_word("the prolost and `wrong` and \"lost\"", words),
        None
    );
}

#[test]
fn planted_ledger_page_passes() {
    let page = ledger_page();
    let root = planted(
        "ledger-good",
        &[("one.md", &page), ("two.md", &page)],
        README,
    )
    .unwrap();
    fs::create_dir_all(root.join(".project")).unwrap();
    fs::write(root.join(legacy_docs::ledger::LEDGER), LEDGER_TEXT).unwrap();
    let report = legacy_docs::run(&root, &ledger_config()).unwrap();
    assert!(report.problems.is_empty(), "{:#?}", report.problems);
    let l = &report.ledger;
    assert_eq!((l.mf, l.ec), (2, 2), "{l:?}");
    assert_eq!(l.ledgered_rows, 2, "{l:?}");
    assert_eq!(l.cited, 4, "{l:?}");
    // Per page: the citing paragraph, the citing lead-in and its two
    // items, and the Ledgered issues row ("a bug").
    assert_eq!(l.bug_units, 10, "{l:?}");
    assert_eq!(l.bug_units, l.bug_units_cited, "{l:?}");
}

#[test]
fn rejects_unknown_entries_and_triage_ids() {
    let good = ledger_page();
    for (name, from, to, needle) in [
        (
            "unknown-mf",
            "(`maps/Codec.java:30`; MF-02)",
            "(`maps/Codec.java:30`; MF-99)",
            "one.md:48: MF-99 is not an entry of the ledger (.project/PROJECT.md)",
        ),
        (
            "unknown-ec",
            "EC-19a and EC-01",
            "EC-19b and EC-01",
            "EC-19b is not an entry of the ledger",
        ),
        (
            "ul-in-prose",
            "Bit order, EC-19a and EC-01",
            "Bit order, UL-ONE-01",
            "UL-ONE-01 is a retired triage ID, allowed only as the first cell of a \"Ledgered issues\" row; cite MF-01 instead",
        ),
        (
            "ul-unmapped",
            "Bit order, EC-19a and EC-01",
            "Bit order, UL-ONE-02",
            "UL-ONE-02 is a retired triage ID, allowed only as the first cell of a \"Ledgered issues\" row; enter it in the ledger and cite that entry",
        ),
        (
            "ledger-cell-empty",
            "| UL-ONE-01 | MF-01 | a bug |",
            "| UL-ONE-01 | later | a bug |",
            "UL-ONE-01's `Ledger` cell cites no MF-/EC- entry and does not start with \"not a bug\"",
        ),
        (
            "not-a-bug-bare",
            "| UL-ONE-01 | MF-01 | a bug |",
            "| UL-ONE-01 | not a bug | a bug |",
            "UL-ONE-01 is \"not a bug\" without saying why",
        ),
        (
            "ul-twice",
            "| UL-ONE-01 | MF-01 | a bug | `maps/Codec.java:30` |\n",
            "| UL-ONE-01 | MF-01 | a bug | `maps/Codec.java:30` |\n| UL-ONE-01 | MF-02 | a bug | `maps/Codec.java:30` |\n",
            "UL-ONE-01 is listed twice",
        ),
        (
            "no-ledger-column",
            "| ID | Ledger | Problem | Source |",
            "| ID | Entry | Problem | Source |",
            "the \"Ledgered issues\" table has no `Ledger` column",
        ),
    ] {
        let one = good.replace(from, to);
        assert_ne!(one, good, "{name}: mutation did not apply");
        assert_flags(&ledger_problems(name, &one), needle);
    }
    // A "not a bug" row that says why passes.
    let one = good.replace(
        "| UL-ONE-01 | MF-01 | a bug |",
        "| UL-ONE-01 | not a bug: test data only | a bug |",
    );
    let p = ledger_problems("not-a-bug-why", &one);
    assert!(p.is_empty(), "{p:#?}");
    // With rule 6's prefixes, two.md's row must be UL-TWO-NN.
    let cfg = Config {
        fixture_prefixes: &[("one.md", "ONE"), ("two.md", "TWO")],
        ..ledger_config()
    };
    assert_flags(
        &ledger_problems_with("ul-prefix", &good, Some(LEDGER_TEXT), &cfg),
        "UL-ONE-01 does not carry this page's prefix (UL-TWO-NN)",
    );
    // No ledger file: fails closed.
    assert_flags(
        &ledger_problems_with("no-ledger", &good, None, &ledger_config()),
        ".project/PROJECT.md: unreadable",
    );
    assert_flags(
        &ledger_problems_with("empty-ledger", &good, Some("# Project\n"), &ledger_config()),
        "no Must-Fix ledger rows",
    );
}

#[test]
fn rejects_bug_statements_without_an_entry() {
    let good = ledger_page();
    for (name, from, to, needle) in [
        (
            "paragraph",
            "(`maps/Codec.java:30`; MF-02)",
            "(`maps/Codec.java:30`)",
            "one.md:48: calls legacy behaviour a bug (`silently`) but cites no MF-/EC- ledger entry",
        ),
        (
            "item-without-lead-in",
            "(`maps/Codec.java:30`, MF-01):",
            "(`maps/Codec.java:30`):",
            "calls legacy behaviour a bug (`wrong`)",
        ),
        (
            "table-row",
            "| read | `maps/Codec.java:10` |",
            "| read, garbage out | `maps/Codec.java:10` |",
            "calls legacy behaviour a bug (`garbage`)",
        ),
        (
            "quotes-closed-early",
            "the popup \"Value is wrong\"",
            "the popup \"Value is\" wrong",
            "calls legacy behaviour a bug (`wrong`)",
        ),
    ] {
        let one = good.replace(from, to);
        assert_ne!(one, good, "{name}: mutation did not apply");
        assert_flags(&ledger_problems(name, &one), needle);
    }
    // Excluded and worked-example sections are not checked.
    let one = good.replace(
        "Port guidance, no references.",
        "Port guidance: legacy is wrong.",
    );
    let p = ledger_problems("excluded-section", &one);
    assert!(p.is_empty(), "{p:#?}");
    // BUG_EXEMPT covers a neutral use, and goes stale.
    let one = good.replace("(`maps/Codec.java:30`; MF-02)", "(`maps/Codec.java:30`)");
    let cfg = Config {
        bug_exempt: &[("one.md", "Legacy silently writes", "planted")],
        ..ledger_config()
    };
    let p = ledger_problems_with("bug-exempt", &one, Some(LEDGER_TEXT), &cfg);
    assert!(p.is_empty(), "{p:#?}");
    assert_flags(
        &ledger_problems_with("bug-exempt-stale", &good, Some(LEDGER_TEXT), &cfg),
        "BUG_EXEMPT entry \"Legacy silently writes\" matches no uncited bug statement",
    );
}

/// The committed ledger: rows `| MF-NN | P0..P2 | area | ... |` with six
/// cells, numbered 01..N without gaps or repeats, each with a file
/// reference and a proving test; and the committed pages cite only its
/// entries, and map every retired `UL-` ID.
#[test]
fn committed_ledger_is_well_formed_and_cited() {
    let root = workspace_root();
    let text = fs::read_to_string(root.join(legacy_docs::ledger::LEDGER)).unwrap();
    let mut numbers = Vec::new();
    for line in text.lines().filter(|l| l.starts_with("| MF-")) {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split(" | ")
            .map(str::trim)
            .collect();
        assert_eq!(cells.len(), 6, "{line}");
        let n: usize = cells[0].trim_start_matches("MF-").parse().unwrap();
        numbers.push(n);
        assert!(["P0", "P1", "P2"].contains(&cells[1]), "{line}");
        assert!(!cells[2].is_empty() && !cells[4].is_empty(), "{line}");
        if n >= 45 {
            // Rows entered by US-RR-12-3 cite their legacy lines and name
            // the test that will prove them.
            assert!(cells[3].contains(".java:"), "{line}");
            assert!(cells[5] != "—" && !cells[5].is_empty(), "{line}");
        }
    }
    let expect: Vec<usize> = (1..=numbers.len()).collect();
    assert_eq!(numbers, expect, "MF rows must be numbered 1..N in order");
    assert!(numbers.len() >= 87);
    // MF-01 states the E38 symptom legacy actually has (US-RR-12-3).
    let mf01 = text.lines().find(|l| l.starts_with("| MF-01 |")).unwrap();
    assert!(mf01.contains("writes **nothing**"), "{mf01}");
    assert!(!mf01.contains("writes zeroed sums"), "{mf01}");

    let report = legacy_docs::run(&root, &Config::pinned()).unwrap();
    assert!(report.problems.is_empty(), "{:#?}", report.problems);
    let l = &report.ledger;
    assert_eq!(l.mf, numbers.len(), "{l:?}");
    assert!(l.ec >= 30, "{l:?}");
    // 62 problems were triaged out of the pages' Unledgered issues.
    assert_eq!(l.ledgered_rows, 62, "{l:?}");
    assert!(l.citations >= 500 && l.cited >= 90, "{l:?}");
    assert!(l.bug_units >= 50, "{l:?}");
    assert_eq!(
        l.bug_units - l.bug_units_cited,
        legacy_docs::ledger::BUG_EXEMPT.len(),
        "{l:?}"
    );
}

/// A committed page that calls legacy behaviour a bug without an entry,
/// or cites a retired triage ID, fails.
#[test]
fn committed_page_with_an_uncited_bug_fails() {
    let root = workspace_root();
    let text = fs::read_to_string(root.join("docs/legacy/ssm.md")).unwrap();
    let marker = "\n## 1. ";
    let at = text.find(marker).unwrap();
    let mut planted_text = text.clone();
    planted_text.insert_str(
        at,
        "\n## 0. Planted\n\nLegacy silently drops the frame (`iso9141/SSMProtocol.java:98`), see UL-SSM-04.\n",
    );
    let tmp = planted_copy("ledger-committed", "ssm.md", &planted_text).unwrap();
    let report = legacy_docs::run(&tmp, &Config::pinned()).unwrap();
    assert_flags(
        &report.problems,
        "calls legacy behaviour a bug (`silently`)",
    );
    assert_flags(
        &report.problems,
        "UL-SSM-04 is a retired triage ID, allowed only as the first cell of a \"Ledgered issues\" row; cite MF-80 instead",
    );
}
