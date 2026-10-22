//! Rule 6 of `check-legacy-docs`: every fixture a page asks for is tracked
//! (US-RR-12-2).
//!
//! * Each area page's last `## ` section is [`FIXTURES_HEADING`], a table
//!   whose rows each start with a stable ID `LEG-<PREFIX>-NN`, `<PREFIX>` the
//!   page's own ([`super::FIXTURE_PREFIXES`]); at least one row; no ID twice
//!   across the pages. Any `LEG-...` ID mentioned anywhere on a page must be
//!   one of those.
//! * [`FIXTURES_MAP`] (in [`DOCS_DIR`]) has exactly the pages' IDs, each
//!   with `kat` (repository-relative paths of the KAT, fixture or test files
//!   that pin it, each an existing file), `task` (a ProjectMan task, whose
//!   `.project/tasks/<task>.md` exists, is not archived, and names the ID in
//!   its title or body), or both.
//!
//! So a new ID cannot be added untracked, and a mapping cannot point at a
//! file or task that does not exist.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;

use super::{Config, DOCS_DIR, heading, is_delimiter_row, split_cells};

/// The heading of the section every page ends with.
pub const FIXTURES_HEADING: &str = "Fixtures to write";

/// The fixture map, in [`DOCS_DIR`].
pub const FIXTURES_MAP: &str = "fixtures-map.toml";

/// Where ProjectMan keeps one Markdown file per task.
pub const TASKS_DIR: &str = ".project/tasks";

/// Counts over the fixture map, for the summary line and the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FixtureStats {
    /// IDs on the pages (each mapped, when there are no problems).
    pub ids: usize,
    /// Entries pinned by existing files only, by a task only, and by both
    /// (files that cover part of it, a task for the rest).
    pub kat_only: usize,
    pub task_only: usize,
    pub both: usize,
    /// Distinct tasks the map names.
    pub tasks: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    #[serde(default)]
    kat: Vec<String>,
    task: Option<String>,
}

/// One defined ID: page and line.
type Defined = BTreeMap<String, (String, usize)>;

/// Runs rule 6 over the area pages' `texts` (page name → text). Returns the
/// map's counts; an empty `config.fixture_prefixes` turns the rule off (the
/// planted trees of the rule 1-5 tests), which [`Config::pinned`] never is.
pub fn check(
    root: &Path,
    config: &Config<'_>,
    texts: &BTreeMap<String, String>,
    problems: &mut Vec<String>,
) -> (FixtureStats, BTreeMap<String, usize>) {
    let mut stats = FixtureStats::default();
    let mut per_page = BTreeMap::new();
    if config.fixture_prefixes.is_empty() {
        return (stats, per_page);
    }
    let mut seen_prefix = BTreeSet::new();
    for (page, prefix) in config.fixture_prefixes {
        if !config.areas.iter().any(|(_, p, _)| p == page) {
            problems.push(format!(
                "{DOCS_DIR}/{page}: FIXTURE_PREFIXES names a page that is not an area page"
            ));
        }
        if !seen_prefix.insert(*prefix) {
            problems.push(format!(
                "FIXTURE_PREFIXES: prefix LEG-{prefix} is given to two pages"
            ));
        }
    }

    let mut defined = Defined::new();
    let mut mentions = Vec::new();
    for (_, page, _) in config.areas {
        let Some((_, prefix)) = config.fixture_prefixes.iter().find(|(p, _)| p == page) else {
            problems.push(format!(
                "{DOCS_DIR}/{page}: no FIXTURE_PREFIXES entry (the LEG-<PREFIX> of its fixture IDs)"
            ));
            continue;
        };
        let Some(text) = texts.get(*page) else {
            continue;
        };
        let ids = page_ids(page, prefix, text, problems);
        per_page.insert((*page).to_owned(), ids.len());
        for (id, line) in ids {
            if let Some((p, l)) = defined.get(&id) {
                problems.push(format!(
                    "{DOCS_DIR}/{page}:{line}: {id} is listed twice (also at {DOCS_DIR}/{p}:{l})"
                ));
            } else {
                defined.insert(id, ((*page).to_owned(), line));
            }
        }
        for (id, line) in id_tokens(text) {
            mentions.push((id, (*page).to_owned(), line));
        }
    }
    for (id, page, line) in mentions {
        if !defined.contains_key(&id) {
            problems.push(format!(
                "{DOCS_DIR}/{page}:{line}: {id} is not an ID of any page's \"{FIXTURES_HEADING}\" list"
            ));
        }
    }

    let map_at = format!("{DOCS_DIR}/{FIXTURES_MAP}");
    let map: BTreeMap<String, Entry> = match fs::read_to_string(root.join(&map_at)) {
        Err(e) => {
            problems.push(format!(
                "{map_at}: unreadable ({e}); it maps every fixture ID to its KAT file or task"
            ));
            return (stats, per_page);
        }
        Ok(text) => match toml::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                problems.push(format!("{map_at}: not a valid fixture map: {e}"));
                return (stats, per_page);
            }
        },
    };

    stats.ids = defined.len();
    for (id, (page, line)) in &defined {
        if !map.contains_key(id) {
            problems.push(format!(
                "{DOCS_DIR}/{page}:{line}: {id} is not in {map_at} (map it to its KAT file or a tracked task)"
            ));
        }
    }
    let mut tasks = BTreeSet::new();
    for (id, entry) in &map {
        let at = format!("{map_at}: [{id}]");
        if !defined.contains_key(id) {
            problems.push(format!(
                "{at} is on no page's \"{FIXTURES_HEADING}\" list (stale: remove it)"
            ));
        }
        match (entry.kat.is_empty(), &entry.task) {
            (true, None) => problems.push(format!(
                "{at} has neither `kat` nor `task`: an untracked fixture"
            )),
            (false, None) => stats.kat_only += 1,
            (true, Some(_)) => stats.task_only += 1,
            (false, Some(_)) => stats.both += 1,
        }
        for path in &entry.kat {
            if let Some(p) = kat_problem(root, path) {
                problems.push(format!("{at}: kat {path:?} {p}"));
            }
        }
        if let Some(task) = &entry.task {
            tasks.insert(task.clone());
            if let Some(p) = task_problem(root, task, id) {
                problems.push(format!("{at}: task {task} {p}"));
            }
        }
    }
    stats.tasks = tasks.len();
    (stats, per_page)
}

/// The IDs of `page`'s final fixtures section, with their lines; problems
/// for a missing or misplaced section, a row without an ID, an ID of
/// another page's prefix.
fn page_ids(
    page: &str,
    prefix: &str,
    text: &str,
    problems: &mut Vec<String>,
) -> Vec<(String, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut fence = false;
    let mut last_h2: Option<(usize, &str)> = None;
    let mut code = vec![false; lines.len()];
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            code[i] = true;
            continue;
        }
        code[i] = fence;
        if !fence && let Some((2, title)) = heading(line) {
            last_h2 = Some((i, title));
        }
    }
    let at = |i: usize| format!("{DOCS_DIR}/{page}:{}", i + 1);
    let start = match last_h2 {
        Some((i, title)) if title == FIXTURES_HEADING => i,
        Some((i, title)) => {
            problems.push(format!(
                "{}: the page ends with section {title:?}, not \"{FIXTURES_HEADING}\" (every page ends with its fixtures list)",
                at(i)
            ));
            return Vec::new();
        }
        None => {
            problems.push(format!(
                "{DOCS_DIR}/{page}: no \"## {FIXTURES_HEADING}\" section (every page ends with its fixtures list)"
            ));
            return Vec::new();
        }
    };
    let mut ids = Vec::new();
    for i in start + 1..lines.len() {
        let line = lines[i];
        if code[i] || !line.trim_start().starts_with('|') || is_delimiter_row(line) {
            continue;
        }
        // A header row is the one right above the delimiter row.
        if lines.get(i + 1).is_some_and(|n| is_delimiter_row(n)) {
            continue;
        }
        let cells = split_cells(line);
        let first = cells
            .first()
            .map_or("", |c| c.trim_matches(['`', '*', ' ']));
        match parse_id(first) {
            Some((area, _)) if area == prefix => ids.push((first.to_owned(), i)),
            Some(_) => problems.push(format!(
                "{}: {first} does not use this page's prefix LEG-{prefix}",
                at(i)
            )),
            None => problems.push(format!(
                "{}: fixtures row {first:?} does not start with a stable ID LEG-{prefix}-NN",
                at(i)
            )),
        }
    }
    if ids.is_empty() {
        problems.push(format!(
            "{}: \"{FIXTURES_HEADING}\" lists no fixture ID (a table of LEG-{prefix}-NN rows)",
            at(start)
        ));
    }
    ids.into_iter().map(|(id, i)| (id, i + 1)).collect()
}

/// `LEG-AREA-NN` → `(AREA, NN)`: `AREA` capital letters, `NN` two or more
/// digits.
fn parse_id(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix("LEG-")?;
    let (area, num) = rest.split_once('-')?;
    let ok = !area.is_empty()
        && area.bytes().all(|b| b.is_ascii_uppercase())
        && num.len() >= 2
        && num.bytes().all(|b| b.is_ascii_digit());
    ok.then_some((area, num))
}

/// Every `LEG-AREA-NN` token of `text`, with its line.
fn id_tokens(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let b = line.as_bytes();
        let mut from = 0;
        while let Some(k) = line.get(from..).and_then(|s| s.find("LEG-")) {
            let s = from + k;
            from = s + 4;
            if s > 0 && b[s - 1].is_ascii_alphanumeric() {
                continue;
            }
            let mut e = s + 4;
            while e < b.len() && b[e].is_ascii_uppercase() {
                e += 1;
            }
            if e == s + 4 || b.get(e) != Some(&b'-') {
                continue;
            }
            e += 1;
            let d = e;
            while e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            if e - d >= 2 {
                out.push((line[s..e].to_owned(), n + 1));
            }
        }
    }
    out
}

/// Whether `text` names `id` as a whole token (`LEG-DEF-1` is not in
/// `LEG-DEF-10`).
fn names(text: &str, id: &str) -> bool {
    text.match_indices(id).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + id.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

fn kat_problem(root: &Path, path: &str) -> Option<String> {
    let p = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || !p.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Some("is not a plain repository-relative path".to_owned());
    }
    (!root.join(p).is_file()).then(|| "does not exist".to_owned())
}

fn task_problem(root: &Path, task: &str, id: &str) -> Option<String> {
    let shaped = task.starts_with("US-")
        && task.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && task
            .rsplit('-')
            .next()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if !shaped {
        return Some("is not a task ID (US-<PREFIX>-<story>-<n>)".to_owned());
    }
    let rel = format!("{TASKS_DIR}/{task}.md");
    let Ok(text) = fs::read_to_string(root.join(&rel)) else {
        return Some(format!("does not exist ({rel})"));
    };
    if text.lines().any(|l| l.trim() == "archived: true") {
        return Some(format!("is archived ({rel}): track {id} with a live task"));
    }
    (!names(&text, id)).then(|| format!("does not name {id} in its title or body ({rel})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_shapes() {
        assert_eq!(parse_id("LEG-DEF-01"), Some(("DEF", "01")));
        assert_eq!(parse_id("LEG-SSM-120"), Some(("SSM", "120")));
        for no in [
            "LEG-DEF-1",
            "LEG-def-01",
            "LEG--01",
            "LEG-DEF-0a",
            "DEF-01",
            "LEG-DEF",
        ] {
            assert_eq!(parse_id(no), None, "{no}");
        }
    }

    #[test]
    fn tokens_and_whole_names() {
        let toks = id_tokens("a LEG-DEF-25, xLEG-DEF-26\n(LEG-SSM-04) LEG-SSM-5 LEG-DEF-");
        assert_eq!(
            toks,
            vec![("LEG-DEF-25".to_owned(), 1), ("LEG-SSM-04".to_owned(), 2)]
        );
        assert!(names("covers LEG-DEF-01, LEG-DEF-10", "LEG-DEF-10"));
        assert!(!names("covers LEG-DEF-010", "LEG-DEF-01"));
        assert!(!names("covers LEG-DEF-10", "LEG-DEF-1"));
    }
}
