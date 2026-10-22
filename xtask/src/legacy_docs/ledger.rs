//! Rule 7 of `check-legacy-docs`: a behaviour that is a bug is
//! cross-referenced to the Must-Fix ledger, not described as intended
//! (US-RR-12-3).
//!
//! The ledger is PROJECT.md's "Must-Fix Bugs" table (rows `| MF-NN | ...`)
//! and its "Edge Cases" list (items `- EC-NN ...`, `EC-19a`), read from
//! [`Config::ledger`]. On every area page:
//!
//! * **Cited entries exist.** Every `MF-NN` and `EC-NN` token anywhere on the
//!   page (prose, tables, the fixtures list) names an entry of the ledger.
//! * **No triage IDs.** The `UL-<PREFIX>-NN` IDs the pages used for problems
//!   not yet in the ledger are retired: one may appear only as the first cell
//!   of a row of the page's [`LEDGERED_SECTION`] table, which maps it to its
//!   entry (tasks and fixtures written before the ledger rows existed still
//!   cite them). That table has a `Ledger` column; each row's `Ledger` cell
//!   cites an `MF-`/`EC-` entry or starts with [`NOT_A_BUG`] and says why;
//!   an ID is listed once and carries the page's prefix
//!   ([`super::FIXTURE_PREFIXES`]). Anywhere else a `UL-` ID is an error that
//!   names the entry to cite instead.
//! * **Bug statements carry their entry.** A behaviour unit (as rule 3 counts
//!   them: paragraphs, list items and table rows outside the excluded and
//!   worked-example sections, and the rows of [`LEDGERED_SECTION`]) whose
//!   text, outside inline code and double quotes (legacy's own messages),
//!   contains a word of [`Config::bug_words`] at a word start must cite an
//!   `MF-`/`EC-` entry itself, or lean on one: a list item or table row on
//!   the lead-in paragraph (ending with `:`) right above its list or table, a
//!   nested item on its parent item; a [`LEDGERED_SECTION`] row is settled
//!   by its `Ledger` cell. Otherwise it
//!   needs a [`Config::bug_exempt`] entry `(page, start of the unit's text,
//!   reason)`, for a sentence that uses such a word without describing a
//!   legacy bug. An entry that matches no such unit fails (stale).
//!
//! [`Config::pinned`] turns the rule on with [`LEDGER`], [`BUG_WORDS`] and
//! [`BUG_EXEMPT`]; a config with `ledger: None` (the planted trees of the
//! rule 1-6 tests) turns it off.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::{
    Block, Config, DOCS_DIR, Kind, Scope, blocks, heading, is_delimiter_row, is_example_para,
    split_cells, unit_body, unnumbered,
};

/// The ledger: PROJECT.md, relative to the repository root.
pub const LEDGER: &str = ".project/PROJECT.md";

/// The `##` section of a page that maps its retired `UL-` IDs to ledger
/// entries. Its prose is not behaviour (rule 3 checks only its rows).
pub const LEDGERED_SECTION: &str = "Ledgered issues";

/// How a [`LEDGERED_SECTION`] row's `Ledger` cell starts when the problem
/// turned out not to be a legacy bug (the rest of the cell says why).
pub const NOT_A_BUG: &str = "not a bug";

/// Words that call legacy behaviour a bug, matched case-insensitively at the
/// start of a word (so `corrupt` also matches `corrupts`, `corrupted`),
/// outside inline code. Curated against the pages (US-RR-12-3):
///
/// * left out: `never` (70-odd neutral uses: "never read", "never sent"),
///   `throws` (legacy's error path, reported as such), `clamp`, `wrap`,
///   `truncat`, `drop`, `discard`, `ignore` (each mostly specifies intended
///   behaviour, e.g. "`userlevel` int, clamped 1-5", "the counter wraps"),
///   `stale` (the protocol's "drain stale input"), `dead`, `unreachable`
///   (dead code with no behavioural effect, listed neutrally in
///   `definitions.md` section 9). The bug uses of those words were cited by
///   hand in US-RR-12-3, and most sit next to a word below.
pub const BUG_WORDS: &[&str] = &[
    "bug",
    "wrong",
    "incorrect",
    "silently",
    "crash",
    "corrupt",
    "overflow",
    "garbage",
    "bogus",
    "lost",
    "lose",
    "leak",
    "broken",
    "misfram",
    "mistak",
    "accident",
    "inconsisten",
    "disagree",
    "vanish",
    "fragile",
];

/// Sentences that use a [`BUG_WORDS`] word without describing a legacy bug:
/// `(page, start of the unit's text after any list marker, why)`.
pub const BUG_EXEMPT: &[(&str, &str, &str)] = &[];

/// Rule 7's counts, for the summary line and the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LedgerStats {
    /// Entries in the ledger.
    pub mf: usize,
    pub ec: usize,
    /// `MF-`/`EC-` tokens on the pages, all checked against the ledger.
    pub citations: usize,
    /// Distinct entries the pages cite.
    pub cited: usize,
    /// Behaviour units with a bug word, and how many cite or lean on an
    /// entry (the rest are [`BUG_EXEMPT`]).
    pub bug_units: usize,
    pub bug_units_cited: usize,
    /// Rows of the pages' [`LEDGERED_SECTION`] tables.
    pub ledgered_rows: usize,
}

/// The ledger IDs in PROJECT.md's `text`: `MF-NN` of the Must-Fix table's
/// rows, `EC-NN` (and `EC-19a`) of the edge-case list items.
pub fn ledger_ids(text: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut mf = BTreeSet::new();
    let mut ec = BTreeSet::new();
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix('|')
            && let Some((id, _)) = tokens(rest.trim_start(), "MF-").first()
            && rest.trim_start().starts_with(id.as_str())
        {
            mf.insert(id.clone());
        }
        if let Some(rest) = t.strip_prefix("- ")
            && let Some((id, _)) = tokens(rest, "EC-").first()
            && rest.starts_with(id.as_str())
        {
            ec.insert(id.clone());
        }
    }
    (mf, ec)
}

/// `MF-NN`, `EC-NN[a-z]` or `UL-ABC-NN` tokens of `text` with `prefix`
/// (`"MF-"`, `"EC-"`, `"UL-"`), with their byte offsets. A token starts at
/// a word start and ends at a word end.
fn tokens(text: &str, prefix: &str) -> Vec<(String, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = text.get(from..).and_then(|t| t.find(prefix)) {
        let start = from + off;
        from = start + prefix.len();
        if start > 0 && b.get(start - 1).is_some_and(u8::is_ascii_alphanumeric) {
            continue;
        }
        let mut i = start + prefix.len();
        if prefix == "UL-" {
            let letters = b.get(i..).map_or(0, |r| {
                r.iter().take_while(|c| c.is_ascii_uppercase()).count()
            });
            if letters == 0 || b.get(i + letters) != Some(&b'-') {
                continue;
            }
            i += letters + 1;
        }
        let digits = b
            .get(i..)
            .map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
        if digits == 0 {
            continue;
        }
        i += digits;
        if prefix == "EC-" && b.get(i).is_some_and(u8::is_ascii_lowercase) {
            i += 1;
        }
        if b.get(i).is_some_and(u8::is_ascii_alphanumeric) {
            continue;
        }
        if let Some(tok) = text.get(start..i) {
            out.push((tok.to_owned(), start));
        }
    }
    out
}

/// Whether `text` cites a ledger entry (form only; existence is checked
/// separately).
fn cites(text: &str) -> bool {
    !tokens(text, "MF-").is_empty() || !tokens(text, "EC-").is_empty()
}

/// `text` without its inline code spans and double-quoted text (legacy's
/// own words: popup messages, code comments).
fn without_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut tick = false;
    let mut quote = false;
    for c in text.chars() {
        if c == '`' && !quote {
            tick = !tick;
            out.push(' ');
        } else if c == '"' && !tick {
            quote = !quote;
            out.push(' ');
        } else if !tick && !quote {
            out.push(c);
        }
    }
    out
}

/// The first word of `words` that starts a word of `text` (outside inline
/// code and quotes), if any.
pub fn bug_word<'w>(text: &str, words: &[&'w str]) -> Option<&'w str> {
    let lower = without_code(text).to_lowercase();
    let b = lower.as_bytes();
    words.iter().copied().find(|w| {
        lower.match_indices(w).any(|(at, _)| {
            at == 0
                || b.get(at - 1)
                    .is_some_and(|c| !c.is_ascii_alphanumeric() && *c != b'_')
        })
    })
}

fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(0, |t| t.bytes().filter(|b| *b == b'\n').count())
        + 1
}

/// Runs rule 7 over the area pages' `texts` (page name → text).
pub fn check(
    root: &Path,
    config: &Config<'_>,
    texts: &BTreeMap<String, String>,
    problems: &mut Vec<String>,
) -> LedgerStats {
    let mut stats = LedgerStats::default();
    let Some(ledger) = config.ledger else {
        return stats;
    };
    let (mf, ec) = match fs::read_to_string(root.join(ledger)) {
        Ok(text) => ledger_ids(&text.replace("\r\n", "\n")),
        Err(e) => {
            problems.push(format!(
                "{ledger}: unreadable ({e}); it holds the Must-Fix ledger the pages cite"
            ));
            return stats;
        }
    };
    if mf.is_empty() {
        problems.push(format!(
            "{ledger}: no Must-Fix ledger rows (`| MF-NN | ...`) found"
        ));
    }
    stats.mf = mf.len();
    stats.ec = ec.len();
    let mut cited = BTreeSet::new();
    let mut used_exempt = BTreeSet::new();
    for (_, page, _) in config.areas {
        let Some(text) = texts.get(*page) else {
            continue;
        };
        let at = |line: usize| format!("{DOCS_DIR}/{page}:{line}");
        // Cited entries exist.
        for (prefix, known) in [("MF-", &mf), ("EC-", &ec)] {
            for (id, off) in tokens(text, prefix) {
                stats.citations += 1;
                if known.contains(&id) {
                    cited.insert(id);
                } else {
                    problems.push(format!(
                        "{}: {id} is not an entry of the ledger ({ledger})",
                        at(line_of(text, off))
                    ));
                }
            }
        }
        // The Ledgered issues table, and no triage ID outside it.
        let prefix = config
            .fixture_prefixes
            .iter()
            .find(|(p, _)| p == page)
            .map(|(_, x)| *x);
        let rows = ledgered_rows(text);
        let mut mapped: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for row in &rows {
            stats.ledgered_rows += 1;
            match &row.ledger {
                None => problems.push(format!(
                    "{}: the \"{LEDGERED_SECTION}\" table has no `Ledger` column",
                    at(row.line)
                )),
                Some(cell) => {
                    let lower = cell.to_lowercase();
                    if !cites(cell) && !lower.starts_with(NOT_A_BUG) {
                        problems.push(format!(
                            "{}: {}'s `Ledger` cell cites no MF-/EC- entry and does not start with \"{NOT_A_BUG}\": {cell:?}",
                            at(row.line),
                            row.id
                        ));
                    } else if lower.trim() == NOT_A_BUG {
                        problems.push(format!(
                            "{}: {} is \"{NOT_A_BUG}\" without saying why",
                            at(row.line),
                            row.id
                        ));
                    }
                }
            }
            if let Some(px) = prefix
                && !row.id.starts_with(&format!("UL-{px}-"))
            {
                problems.push(format!(
                    "{}: {} does not carry this page's prefix (UL-{px}-NN)",
                    at(row.line),
                    row.id
                ));
            }
            let target = row.ledger.clone().unwrap_or_default();
            if let Some((l, _)) = mapped.insert(row.id.clone(), (row.line, target)) {
                problems.push(format!(
                    "{}: {} is listed twice (also at line {l})",
                    at(row.line),
                    row.id
                ));
            }
        }
        let allowed: BTreeSet<usize> = rows.iter().map(|r| r.offset).collect();
        for (id, off) in tokens(text, "UL-") {
            if allowed.contains(&off) {
                continue;
            }
            let hint = match mapped.get(&id) {
                Some((_, target)) => format!("cite {target} instead"),
                None => "enter it in the ledger and cite that entry".to_owned(),
            };
            problems.push(format!(
                "{}: {id} is a retired triage ID, allowed only as the first cell of a \"{LEDGERED_SECTION}\" row; {hint}",
                at(line_of(text, off))
            ));
        }
        // Bug statements carry their entry.
        for (line, body, leans) in behaviour_units(text) {
            let Some(word) = bug_word(&body, config.bug_words) else {
                continue;
            };
            stats.bug_units += 1;
            if cites(&body) || leans {
                stats.bug_units_cited += 1;
                continue;
            }
            if let Some(i) = config
                .bug_exempt
                .iter()
                .position(|(p, s, _)| p == page && body.starts_with(s))
            {
                used_exempt.insert(i);
                continue;
            }
            let shown: String = body.chars().take(100).collect();
            problems.push(format!(
                "{}: calls legacy behaviour a bug (`{word}`) but cites no MF-/EC- ledger entry (nor leans on a lead-in or parent that does, nor has a BUG_EXEMPT entry): {shown:?}",
                at(line)
            ));
        }
    }
    stats.cited = cited.len();
    for (i, (page, starts, _)) in config.bug_exempt.iter().enumerate() {
        if !used_exempt.contains(&i) {
            problems.push(format!(
                "{DOCS_DIR}/{page}: BUG_EXEMPT entry {starts:?} matches no uncited bug statement (stale: remove it)"
            ));
        }
    }
    stats
}

/// A row of a page's [`LEDGERED_SECTION`] table that starts with a `UL-` ID.
struct LedgeredRow {
    id: String,
    /// Byte offset of the ID in the page, and its line.
    offset: usize,
    line: usize,
    /// The row's `Ledger` cell; `None` when the table has no such column.
    ledger: Option<String>,
}

fn ledgered_rows(text: &str) -> Vec<LedgeredRow> {
    let mut out = Vec::new();
    let mut in_section = false;
    let mut ledger_col: Option<usize> = None;
    let mut offset = 0;
    let mut prev_header: Option<Vec<String>> = None;
    for (idx, line) in text.split('\n').enumerate() {
        let here = offset;
        offset += line.len() + 1;
        if let Some((level, title)) = heading(line) {
            if level <= 2 {
                in_section = level == 2 && unnumbered(title) == LEDGERED_SECTION;
            }
            ledger_col = None;
            prev_header = None;
            continue;
        }
        if !in_section {
            continue;
        }
        let t = line.trim_start();
        if !t.starts_with('|') {
            ledger_col = None;
            prev_header = None;
            continue;
        }
        if is_delimiter_row(line) {
            ledger_col = prev_header
                .take()
                .and_then(|h| h.iter().position(|c| c.eq_ignore_ascii_case("ledger")));
            continue;
        }
        let cells = split_cells(line);
        let first = cells.first().cloned().unwrap_or_default();
        let ids = tokens(&first, "UL-");
        match ids.first() {
            Some((id, 0)) if first == *id => {
                let lead = line.len() - t.len() + t.find(id.as_str()).unwrap_or(0);
                out.push(LedgeredRow {
                    id: id.clone(),
                    offset: here + lead,
                    line: idx + 1,
                    ledger: ledger_col.and_then(|c| cells.get(c).cloned()),
                });
            }
            _ => prev_header = Some(cells),
        }
    }
    out
}

/// Rule 3's behaviour units with their text (list marker removed) and
/// whether they lean on a lead-in paragraph or parent item that cites a
/// ledger entry: `(line, text, leans)`.
fn behaviour_units(text: &str) -> Vec<(usize, String, bool)> {
    let (blocks, _) = blocks(text);
    let mut out = Vec::new();
    let mut in_example = false;
    for (k, b) in blocks.iter().enumerate() {
        if b.kind == Kind::Heading {
            in_example = false;
            continue;
        }
        if b.kind == Kind::Para {
            in_example = false;
        }
        if b.kind == Kind::Code || b.scope == Scope::None {
            continue;
        }
        if b.scope == Scope::RowsOnly && b.kind != Kind::Table {
            continue;
        }
        let lead_cites = k
            .checked_sub(1)
            .and_then(|p| blocks.get(p))
            .filter(|p: &&Block| p.kind == Kind::Para)
            .and_then(|p| p.units.first())
            .is_some_and(|u| u.text.trim_end().ends_with(':') && cites(&u.text));
        match b.kind {
            Kind::Para => {
                let Some(u) = b.units.first() else { continue };
                if is_example_para(&u.text) {
                    in_example = true;
                    continue;
                }
                out.push((u.line, u.text.clone(), false));
            }
            Kind::List if !in_example => {
                let mut parents: Vec<(usize, bool)> = Vec::new();
                for u in &b.units {
                    while parents.last().is_some_and(|(ind, _)| *ind >= u.indent) {
                        parents.pop();
                    }
                    let body = unit_body(&u.text).to_owned();
                    let leans = parents.last().map_or(lead_cites, |(_, c)| *c);
                    let passes = (cites(&body) || leans) && body.trim_end().ends_with(':');
                    parents.push((u.indent, passes));
                    out.push((u.line, body, leans));
                }
            }
            Kind::Table if !in_example => {
                // A Ledgered issues row is settled by its `Ledger` cell,
                // checked on its own (an entry, or "not a bug" and why).
                let settled = lead_cites || b.scope == Scope::RowsOnly;
                for u in b.units.iter().skip(1) {
                    out.push((u.line, u.text.clone(), settled));
                }
            }
            _ => {}
        }
    }
    out
}
