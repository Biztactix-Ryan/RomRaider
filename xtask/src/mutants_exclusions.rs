//! Exclusion-justification check for `.cargo/mutants.toml`
//! (`cargo xtask check-mutants-exclusions`, story US-RR-4, task US-RR-4-7;
//! PROJECT.md Testing Strategy T7).
//!
//! The T7 gate is zero surviving mutants, so every mutant that
//! `.cargo/mutants.toml` hides from it must say why. This check fails unless
//! every entry of each list in [`EXCLUSION_KEYS`] (`exclude_globs`,
//! `exclude_re`, and `skip_calls`, which also hides mutants) carries a
//! **justification comment**, defined as:
//!
//! 1. **Adjacent.** One or more full-line `#` comments directly above the
//!    entry, inside the array: the line just above the entry is a comment,
//!    and the block is every contiguous comment line above that. A blank
//!    line ends the block, so a comment separated from its entry by a blank
//!    line does not count. Nor does a comment above the `key = [` line, a
//!    comment above the previous entry (each entry needs its own), or a
//!    trailing comment on the entry's own line (`"x", # why`): a trailing
//!    comment is allowed in addition but never stands in for the block.
//! 2. **Non-trivial.** The block's text (with the `#`s removed) has at
//!    least [`MIN_WORDS`] words (a word contains a letter, so `# ---` is
//!    none) and contains no placeholder word from [`PLACEHOLDERS`] (`TODO`,
//!    `FIXME`, ...). The header of `.cargo/mutants.toml` says what the text
//!    must contain (why the mutants cannot be killed and what proves the
//!    code right instead); that is left to review.
//!
//! The check fails closed on layouts it cannot read line by line: each entry
//! must be a single-line string on its own line (not on the `key = [` line,
//! not two on one line, no multi-line `"""` strings), the key must be a bare
//! top-level key, and the entries it finds must equal the ones the `toml`
//! crate parses from the same file, so a quoted, dotted or tabled spelling of
//! the key cannot slip entries past it. A file that does not parse fails.
//!
//! CI runs it in the `clippy` job of `ci.yml`; `xtask/tests/mutants_exclusions.rs`
//! runs it on the committed file and on planted samples.

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result};

/// The configuration file cargo-mutants reads by default, relative to the root.
pub const CONFIG: &str = ".cargo/mutants.toml";

/// Keys whose entries hide mutants from the gate.
pub const EXCLUSION_KEYS: &[&str] = &["exclude_globs", "exclude_re", "skip_calls"];

/// Minimum number of words in a justification.
pub const MIN_WORDS: usize = 6;

/// Words that mark a justification as a placeholder (case-insensitive).
pub const PLACEHOLDERS: &[&str] = &["todo", "fixme", "xxx", "tbd"];

/// One exclusion entry found in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The key whose array holds it (`exclude_re`, ...).
    pub key: String,
    /// 1-based line of the entry.
    pub line: usize,
    /// The string value, unescaped.
    pub value: String,
    /// The justification block's text (comment markers removed, lines joined
    /// with spaces), if the entry has an adjacent comment block.
    pub justification: Option<String>,
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} entry {:?}", self.key, self.value)
    }
}

/// Outcome of a check of one file.
#[derive(Debug, Default)]
pub struct Report {
    /// Every exclusion entry found, justified or not.
    pub entries: Vec<Entry>,
    /// Every problem, as `path:line: message` (empty = pass).
    pub problems: Vec<String>,
}

/// Why `text` (a justification block) is not an acceptable justification, or
/// `None` if it is.
#[must_use]
pub fn trivial(text: &str) -> Option<String> {
    let placeholder = text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .find(|w| PLACEHOLDERS.contains(&w.as_str()));
    if let Some(p) = placeholder {
        return Some(format!(
            "justification is a placeholder ({p:?}); say why these mutants cannot be killed by a test and what proves the code right instead"
        ));
    }
    let words = text
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .count();
    if words < MIN_WORDS {
        return Some(format!(
            "justification is too short ({words} word(s), at least {MIN_WORDS}); say why these mutants cannot be killed by a test and what proves the code right instead"
        ));
    }
    None
}

/// The value of a one-line TOML string token (`"..."` or `'...'`), unescaped
/// by the `toml` crate itself so it compares equal to the parsed value.
fn unquote(token: &str) -> Option<String> {
    let t: toml::Table = format!("v = {token}").parse().ok()?;
    t.get("v")?.as_str().map(str::to_owned)
}

/// The end (exclusive byte index) of the string token starting at `start`
/// in `line`, or `None` if it does not close on this line.
fn string_end(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote == b'"' => i += 2,
            b if b == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// The bare key of a `key = ...` line and the text after `=`, if any.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    let k = k.trim();
    (!k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some((k, v))
}

/// The text of a full-line comment (`# ...`), or `None` if `line` is not one.
fn comment_text(line: &str) -> Option<&str> {
    line.trim_start()
        .strip_prefix('#')
        .map(|c| c.trim_start_matches('#').trim())
}

/// State of the comment block above the line being read.
#[derive(Default)]
struct Block {
    lines: Vec<String>,
    /// A blank line came after the block.
    gap: bool,
}

impl Block {
    fn clear(&mut self) {
        self.lines.clear();
        self.gap = false;
    }
}

/// Every exclusion entry in `src`, with the problems of the layout. `path` is
/// only used in messages.
fn scan(path: &str, src: &str, problems: &mut Vec<String>) -> Vec<Entry> {
    let lines: Vec<&str> = src.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut entries = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let Some((key, rest)) = key_value(line).filter(|(k, _)| EXCLUSION_KEYS.contains(k)) else {
            i += 1;
            continue;
        };
        let at = i + 1;
        let Some(after) = rest.trim_start().strip_prefix('[') else {
            problems.push(format!("{path}:{at}: `{key}` is not an array"));
            i += 1;
            continue;
        };
        // Read the array: the rest of the opening line, then following lines.
        let mut block = Block::default();
        let mut text = after.to_owned();
        let mut ln = i;
        let mut closed = false;
        loop {
            let first_line = ln == i;
            if !first_line {
                let l = lines[ln];
                if l.trim().is_empty() {
                    if !block.lines.is_empty() {
                        block.gap = true;
                    }
                    text = String::new();
                } else if let Some(c) = comment_text(l) {
                    if block.gap {
                        block.clear();
                    }
                    block.lines.push(c.to_owned());
                    text = String::new();
                } else {
                    text = l.to_owned();
                }
            }
            let (found, done, bad) = scan_array_line(&text);
            if let Some(why) = bad {
                problems.push(format!("{path}:{}: `{key}`: {why}", ln + 1));
                closed = true;
                break;
            }
            for (n, token) in found.iter().enumerate() {
                let Some(value) = unquote(token) else {
                    problems.push(format!(
                        "{path}:{}: `{key}`: cannot read string {token}",
                        ln + 1
                    ));
                    continue;
                };
                let mut entry = Entry {
                    key: key.to_owned(),
                    line: ln + 1,
                    value,
                    justification: None,
                };
                if first_line {
                    problems.push(format!(
                        "{path}:{}: {entry}: on the same line as `{key} = [`; put each exclusion on its own line under its justification comment",
                        ln + 1
                    ));
                } else if n > 0 {
                    problems.push(format!(
                        "{path}:{}: {entry}: more than one entry on this line; put each exclusion on its own line under its own justification comment",
                        ln + 1
                    ));
                } else if block.lines.is_empty() {
                    problems.push(format!(
                        "{path}:{}: {entry}: no justification comment on the line(s) directly above it",
                        ln + 1
                    ));
                } else if block.gap {
                    problems.push(format!(
                        "{path}:{}: {entry}: its justification comment is separated from it by a blank line; a justification must be directly above its entry",
                        ln + 1
                    ));
                } else {
                    let text = block.lines.join(" ");
                    if let Some(why) = trivial(&text) {
                        problems.push(format!("{path}:{}: {entry}: {why}", ln + 1));
                    }
                    entry.justification = Some(text);
                }
                entries.push(entry);
            }
            if !first_line && !found.is_empty() {
                block.clear();
            } else if !first_line && !text.trim().is_empty() && !text.trim_start().starts_with('#')
            {
                // A line holding only `,` or `]`: not a comment, ends the block.
                block.clear();
            }
            if done {
                closed = true;
                break;
            }
            ln += 1;
            if ln >= lines.len() {
                break;
            }
        }
        if !closed {
            problems.push(format!("{path}:{at}: `{key}` array is not closed"));
        }
        i = ln + 1;
    }
    entries
}

/// The string tokens on one line of an array body (up to a `#` comment or
/// the closing `]`), whether the array closed on it, and a layout problem.
fn scan_array_line(text: &str) -> (Vec<String>, bool, Option<String>) {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b',' => i += 1,
            b'#' => break,
            b']' => {
                let rest = text[i + 1..].trim();
                if !(rest.is_empty() || rest.starts_with('#')) {
                    return (
                        found,
                        true,
                        Some(format!("unexpected text after `]`: {rest}")),
                    );
                }
                return (found, true, None);
            }
            b'"' | b'\'' => {
                let q = &text[i..];
                if q.starts_with("\"\"\"") || q.starts_with("'''") {
                    return (
                        found,
                        false,
                        Some("multi-line strings are not supported; write each entry as a one-line string".to_owned()),
                    );
                }
                let Some(end) = string_end(text, i) else {
                    return (found, false, Some("unterminated string".to_owned()));
                };
                found.push(text[i..end].to_owned());
                i = end;
            }
            _ => {
                return (
                    found,
                    false,
                    Some(format!(
                        "unexpected `{}`; entries must be one-line strings",
                        text[i..].trim()
                    )),
                );
            }
        }
    }
    (found, false, None)
}

/// Check the text of a mutants.toml; `path` is only used in messages.
#[must_use]
pub fn check_source(path: &str, src: &str) -> Report {
    let mut problems = Vec::new();
    let table: toml::Table = match src.parse() {
        Ok(t) => t,
        Err(e) => {
            return Report {
                entries: Vec::new(),
                problems: vec![format!("{path}: does not parse as TOML: {e}")],
            };
        }
    };
    let entries = scan(path, src, &mut problems);
    // Cross-check with the parser: every entry it sees must have been read.
    for key in EXCLUSION_KEYS {
        let parsed: Vec<String> = match table.get(*key) {
            None => Vec::new(),
            Some(toml::Value::Array(a)) => {
                let strings: Vec<String> = a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
                if strings.len() != a.len() {
                    problems.push(format!("{path}: `{key}` has a non-string entry"));
                }
                strings
            }
            Some(_) => {
                problems.push(format!("{path}: `{key}` is not an array"));
                Vec::new()
            }
        };
        let scanned: Vec<String> = entries
            .iter()
            .filter(|e| e.key == *key)
            .map(|e| e.value.clone())
            .collect();
        if parsed != scanned {
            problems.push(format!(
                "{path}: `{key}`: the TOML parser sees entries {parsed:?} but the line reader found {scanned:?}; write it as a bare top-level `{key} = [` followed by one string per line, each under its justification comment"
            ));
        }
    }
    Report { entries, problems }
}

/// Check `config` (default `<root>/.cargo/mutants.toml`).
pub fn run(root: &Path, config: Option<&Path>) -> Result<Report> {
    let (path, shown) = match config {
        Some(p) => (p.to_path_buf(), p.display().to_string()),
        None => (root.join(CONFIG), CONFIG.to_owned()),
    };
    let src = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(check_source(&shown, &src))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHY: &str = "    # Compiled only with a feature the crate's own tests never enable, so\n    # no test can see it; the wrapped function is gated and fully killed.\n";

    fn problems(src: &str) -> Vec<String> {
        check_source("m.toml", src).problems
    }

    #[test]
    fn justified_entries_pass_in_every_key() {
        for key in EXCLUSION_KEYS {
            let src = format!("{key} = [\n{WHY}    \"a\",\n{WHY}    'b\\c',\n]\n");
            let r = check_source("m.toml", &src);
            assert!(r.problems.is_empty(), "{key}: {:?}", r.problems);
            assert_eq!(r.entries.len(), 2);
            assert_eq!(r.entries[1].value, "b\\c");
            assert_eq!(r.entries[1].line, 7);
            assert!(
                r.entries[0]
                    .justification
                    .as_deref()
                    .unwrap()
                    .starts_with("Compiled only")
            );
        }
        assert!(problems("exclude_re = []\nexclude_globs = [\n]\n").is_empty());
        assert!(problems("examine_globs = [\"x\"]\n").is_empty());
    }

    #[test]
    fn uncommented_entry_fails_naming_it() {
        let p = problems("exclude_re = [\n    \"replace foo\",\n]\n");
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(
            p[0],
            "m.toml:2: exclude_re entry \"replace foo\": no justification comment on the line(s) directly above it"
        );
    }

    #[test]
    fn each_entry_needs_its_own_comment() {
        let p = problems(&format!(
            "exclude_globs = [\n{WHY}    \"a\",\n    \"b\",\n]\n"
        ));
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].starts_with("m.toml:5: exclude_globs entry \"b\": no justification"),
            "{p:?}"
        );
    }

    #[test]
    fn blank_line_separated_comment_fails() {
        let p = problems(&format!("exclude_re = [\n{WHY}\n    \"a\",\n]\n"));
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("separated from it by a blank line"), "{p:?}");
        // A fresh block after the gap is adjacent again.
        assert!(
            problems(&format!(
                "exclude_re = [\n    # stale\n\n{WHY}    \"a\",\n]\n"
            ))
            .is_empty()
        );
    }

    #[test]
    fn trivial_comments_fail() {
        for c in [
            "#",
            "# TODO",
            "# todo: justify this exclusion later on please",
            "# ---",
            "# equivalent mutant",
            "# FIXME(ryan) explain why this cannot be killed",
        ] {
            let p = problems(&format!("exclude_re = [\n    {c}\n    \"a\",\n]\n"));
            assert_eq!(p.len(), 1, "{c}: {p:?}");
            assert!(
                p[0].contains("placeholder") || p[0].contains("too short"),
                "{c}: {p:?}"
            );
        }
    }

    #[test]
    fn comments_outside_the_array_or_trailing_do_not_count() {
        let above_key = format!("{WHY}exclude_re = [\n    \"a\",\n]\n");
        assert_eq!(problems(&above_key).len(), 1);
        let trailing = "exclude_re = [\n    \"a\", # Compiled only with a feature no test enables, so never seen\n]\n";
        assert_eq!(problems(trailing).len(), 1);
        // ...but a trailing comment next to a justified entry is fine.
        assert!(problems(&format!("exclude_re = [\n{WHY}    \"a\", # see above\n]\n")).is_empty());
        // `#` inside a string is not a comment.
        assert!(problems(&format!("exclude_re = [\n{WHY}    \"a # b\",\n]\n")).is_empty());
    }

    #[test]
    fn unreadable_layouts_fail_closed() {
        for src in [
            "exclude_re = [\"a\"]\n".to_owned(),
            format!("exclude_re = [\n{WHY}    \"a\", \"b\",\n]\n"),
            format!("exclude_re = [\n{WHY}    \"\"\"a\"\"\",\n]\n"),
            format!("\"exclude_re\" = [\n{WHY}    \"a\",\n]\n"),
            "exclude_re = \"a\"\n".to_owned(),
            "exclude_re = [\n".to_owned(),
            format!("[t]\nexclude_re = [\n{WHY}    \"a\",\n]\n"),
        ] {
            assert!(!problems(&src).is_empty(), "{src}");
        }
    }

    #[test]
    fn trivial_counts_words_with_letters() {
        assert!(trivial("one two three four five six").is_none());
        assert!(trivial("one two three four five").is_some());
        assert!(trivial("one two three four five 6 --").is_some());
        assert!(trivial("one two three four five six Tbd.").is_some());
    }
}
