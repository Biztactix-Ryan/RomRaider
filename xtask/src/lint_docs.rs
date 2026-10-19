//! Lint documentation coverage: every ban in the lint configuration is
//! documented in CONTRIBUTING.md, with its reason.
//!
//! Proves US-RR-3-4 ("the lint configuration is documented in CONTRIBUTING
//! with the reason for each ban") and keeps it true: the integration test
//! `xtask/tests/lint_docs.rs` runs [`check`] on the real files in CI, so a
//! lint added to `Cargo.toml` or a method added to `clippy.toml` without a
//! CONTRIBUTING.md row fails the build.
//!
//! The configuration is read from:
//!
//! * the root `Cargo.toml`, `[workspace.lints.rust]` and
//!   `[workspace.lints.clippy]`: every entry not set to `allow`;
//! * `clippy.toml`: every `disallowed-methods` path (each must also carry a
//!   non-empty `reason`, which clippy prints), and every other key (the
//!   `allow-*-in-tests` switches).
//!
//! In CONTRIBUTING.md's `## Lint configuration` section, the table with the
//! columns `Pattern | Enforced by | Why (refs) | Use instead` must have, for
//! each ban, a row that names it in backticks (a lint in the "Enforced by"
//! column as `` `clippy::name` = level `` / `` `name` = level ``, a method in
//! the "Pattern" column as `` `path` ``) with the configured level, and
//! non-empty "Why" and "Use instead" cells. Each `clippy.toml` switch must be
//! named in backticks somewhere in the section.

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

/// Heading of the CONTRIBUTING.md section that documents the lints.
pub const SECTION: &str = "## Lint configuration";

/// One configured ban.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ban {
    /// A lint from `[workspace.lints.<tool>]`: documented name (`clippy::x`
    /// or a bare rustc name) and level.
    Lint {
        /// Name as written in the docs, e.g. `clippy::unwrap_used`.
        name: String,
        /// Configured level, e.g. `deny`.
        level: String,
    },
    /// A `clippy.toml` `disallowed-methods` path.
    Method {
        /// Path, e.g. `std::thread::sleep`.
        path: String,
    },
    /// Any other `clippy.toml` key, e.g. `allow-unwrap-in-tests`.
    Switch {
        /// Key name.
        key: String,
    },
}

impl fmt::Display for Ban {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lint { name, level } => write!(f, "lint `{name}` = {level}"),
            Self::Method { path } => write!(f, "disallowed method `{path}`"),
            Self::Switch { key } => write!(f, "clippy.toml switch `{key}`"),
        }
    }
}

/// Every ban in the configuration. A malformed config is an error.
pub fn bans(cargo_toml: &str, clippy_toml: &str) -> Result<Vec<Ban>> {
    let manifest: toml::Table = toml::from_str(cargo_toml).context("parsing Cargo.toml")?;
    let lints = manifest
        .get("workspace")
        .and_then(|w| w.get("lints"))
        .and_then(toml::Value::as_table)
        .context("Cargo.toml has no [workspace.lints]")?;
    let mut out = Vec::new();
    for (tool, table) in lints {
        let table = table
            .as_table()
            .with_context(|| format!("[workspace.lints.{tool}] is not a table"))?;
        for (lint, v) in table {
            let level = match v {
                toml::Value::String(s) => s.clone(),
                toml::Value::Table(t) => t
                    .get("level")
                    .and_then(toml::Value::as_str)
                    .with_context(|| format!("lint {tool}::{lint} has no level"))?
                    .to_owned(),
                _ => bail!("lint {tool}::{lint}: unexpected value {v}"),
            };
            if level == "allow" {
                continue;
            }
            let name = if tool == "rust" {
                lint.clone()
            } else {
                format!("{tool}::{lint}")
            };
            out.push(Ban::Lint { name, level });
        }
    }

    let clippy: toml::Table = toml::from_str(clippy_toml).context("parsing clippy.toml")?;
    for (key, v) in &clippy {
        if key != "disallowed-methods" {
            out.push(Ban::Switch { key: key.clone() });
            continue;
        }
        let entries = v
            .as_array()
            .context("clippy.toml disallowed-methods is not an array")?;
        for e in entries {
            let path = e
                .as_str()
                .or_else(|| e.get("path").and_then(toml::Value::as_str))
                .context("clippy.toml disallowed-methods entry without a path")?;
            let reason = e.get("reason").and_then(toml::Value::as_str).unwrap_or("");
            if reason.trim().is_empty() {
                bail!("clippy.toml disallowed-methods `{path}` has no reason");
            }
            out.push(Ban::Method {
                path: path.to_owned(),
            });
        }
    }
    out.sort();
    Ok(out)
}

/// The body of the `## Lint configuration` section (up to the next `## `).
pub fn section(contributing: &str) -> Result<&str> {
    let start = contributing
        .lines()
        .scan(0usize, |off, l| {
            let here = *off;
            *off += l.len() + 1;
            Some((here, l))
        })
        .find(|(_, l)| l.trim_end() == SECTION)
        .map(|(off, l)| off + l.len())
        .with_context(|| format!("CONTRIBUTING.md has no `{SECTION}` section"))?;
    let rest = &contributing[start..];
    let end = rest.find("\n## ").map_or(rest.len(), |i| i + 1);
    Ok(&rest[..end])
}

/// One row of the ban table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// "Pattern" cell.
    pub pattern: String,
    /// "Enforced by" cell.
    pub enforced_by: String,
    /// "Why (refs)" cell.
    pub why: String,
    /// "Use instead" cell.
    pub instead: String,
}

/// The rows of the `Pattern | Enforced by | Why | Use instead` table.
pub fn rows(section: &str) -> Result<Vec<Row>> {
    let mut lines = section.lines().map(str::trim);
    let header = lines
        .find(|l| l.starts_with("| Pattern"))
        .context("no `| Pattern | Enforced by | Why | Use instead |` table")?;
    let cols = cells(header);
    if cols.len() != 4
        || !cols[1].starts_with("Enforced by")
        || !cols[2].starts_with("Why")
        || !cols[3].starts_with("Use instead")
    {
        bail!("unexpected ban table header: {header}");
    }
    let mut out = Vec::new();
    for l in lines.skip(1).take_while(|l| l.starts_with('|')) {
        let c = cells(l);
        if c.len() != 4 {
            bail!("ban table row does not have 4 cells: {l}");
        }
        let [pattern, enforced_by, why, instead] = <[String; 4]>::try_from(c)
            .map_err(|_| anyhow::anyhow!("ban table row does not have 4 cells: {l}"))?;
        out.push(Row {
            pattern,
            enforced_by,
            why,
            instead,
        });
    }
    Ok(out)
}

fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_owned()).collect()
}

/// The level written after `name` in an "Enforced by" cell, e.g. `deny` in
/// `` `clippy::x`, `clippy::y` = deny ``.
fn documented_level(cell: &str, ticked: &str) -> Option<String> {
    let after = &cell[cell.find(ticked)? + ticked.len()..];
    let after = &after[after.find('=')? + 1..];
    let word: String = after
        .trim_start()
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    (!word.is_empty()).then_some(word)
}

fn reasoned(r: &Row) -> bool {
    !r.why.is_empty() && !r.instead.is_empty()
}

/// Every ban that CONTRIBUTING.md fails to document (empty = pass).
pub fn check(bans: &[Ban], contributing: &str) -> Result<Vec<String>> {
    let section = section(contributing)?;
    let rows = rows(section)?;
    let mut problems = Vec::new();
    for ban in bans {
        let ok = match ban {
            Ban::Lint { name, level } => {
                let ticked = format!("`{name}`");
                let mut found = rows
                    .iter()
                    .filter(|r| r.enforced_by.contains(&ticked))
                    .peekable();
                if found.peek().is_none() {
                    Err("no row names it in the `Enforced by` column".to_owned())
                } else if let Some(r) = found.find(|r| reasoned(r)) {
                    match documented_level(&r.enforced_by, &ticked) {
                        Some(l) if l == *level => Ok(()),
                        Some(l) => Err(format!("documented as `= {l}`")),
                        None => Err("its row does not state `= <level>`".to_owned()),
                    }
                } else {
                    Err("its row has an empty `Why` or `Use instead` cell".to_owned())
                }
            }
            Ban::Method { path } => {
                let ticked = format!("`{path}`");
                match rows.iter().find(|r| r.pattern.contains(&ticked)) {
                    None => Err("no row names it in the `Pattern` column".to_owned()),
                    Some(r) if !reasoned(r) => {
                        Err("its row has an empty `Why` or `Use instead` cell".to_owned())
                    }
                    Some(r) if !r.enforced_by.contains("`clippy::disallowed_methods`") => {
                        Err("its row is not enforced by `clippy::disallowed_methods`".to_owned())
                    }
                    Some(_) => Ok(()),
                }
            }
            Ban::Switch { key } => {
                if section.contains(&format!("`{key}`")) {
                    Ok(())
                } else {
                    Err(format!("`{key}` is not mentioned in the section"))
                }
            }
        };
        if let Err(why) = ok {
            problems.push(format!(
                "CONTRIBUTING.md `{SECTION}`: {ban} is not documented: {why}"
            ));
        }
    }
    Ok(problems)
}

/// Run [`check`] on the files of the workspace at `root`.
pub fn run(root: &Path) -> Result<Vec<String>> {
    let read = |name: &str| {
        let p = root.join(name);
        fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))
    };
    let bans = bans(&read("Cargo.toml")?, &read("clippy.toml")?)?;
    if bans.is_empty() {
        bail!("no lint configuration found");
    }
    check(&bans, &read("CONTRIBUTING.md")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO: &str = r#"
[workspace.lints.rust]
unused_must_use = "deny"
rust_2018_idioms = { level = "warn", priority = -1 }
missing_docs = "allow"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
unwrap_used = "deny"
todo = "warn"
"#;

    const CLIPPY: &str = r#"
allow-unwrap-in-tests = true
disallowed-methods = [
    { path = "std::thread::sleep", reason = "use Clock::sleep" },
]
"#;

    const DOC: &str = "# C\n\n## Lint configuration\n\nSwitch: `allow-unwrap-in-tests`.\n\n\
| Pattern | Enforced by | Why (refs) | Use instead |\n\
|---------|-------------|------------|-------------|\n\
| `.unwrap()` | `clippy::unwrap_used` = deny. | Never panic (D-019). | `?` |\n\
| `todo!()` | `clippy::todo` = warn. | Panics (D-019). | Finish it. |\n\
| `std::thread::sleep` | `clippy::disallowed_methods` = deny. | Clock (D-010). | `Clock::sleep` |\n\
| Must-use | `unused_must_use` = deny (rustc). | Swallowed errors. | `?` |\n\
| Idioms | `rust_2018_idioms`, `clippy::all` = warn. | Style. | Follow it. |\n\
\n## Next\n\n| `clippy::new_lint` = deny | x | y | z |\n";

    fn problems(cargo: &str, clippy: &str, doc: &str) -> Vec<String> {
        check(&bans(cargo, clippy).unwrap(), doc).unwrap()
    }

    #[test]
    fn bans_are_read_from_both_files_and_allow_is_skipped() {
        let b = bans(CARGO, CLIPPY).unwrap();
        let names: Vec<String> = b.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            [
                "lint `clippy::all` = warn",
                "lint `clippy::todo` = warn",
                "lint `clippy::unwrap_used` = deny",
                "lint `rust_2018_idioms` = warn",
                "lint `unused_must_use` = deny",
                "disallowed method `std::thread::sleep`",
                "clippy.toml switch `allow-unwrap-in-tests`",
            ]
        );
    }

    #[test]
    fn fully_documented_config_passes() {
        assert_eq!(problems(CARGO, CLIPPY, DOC), Vec::<String>::new());
    }

    #[test]
    fn undocumented_new_lint_fails() {
        let cargo = format!("{CARGO}new_lint = \"deny\"\n");
        let p = problems(&cargo, CLIPPY, DOC);
        assert_eq!(p.len(), 1, "{p:?}");
        // A row outside the section does not count.
        assert!(
            p[0].contains("`clippy::new_lint` = deny is not documented"),
            "{p:?}"
        );
    }

    #[test]
    fn undocumented_disallowed_method_fails() {
        let clippy = CLIPPY.replace(
            "]\n",
            "    { path = \"std::time::Instant::now\", reason = \"use Clock::now\" },\n]\n",
        );
        let p = problems(CARGO, &clippy, DOC);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("`std::time::Instant::now`"), "{p:?}");
    }

    #[test]
    fn disallowed_method_without_reason_is_an_error() {
        let clippy = "disallowed-methods = [\"std::thread::sleep\"]\n";
        assert!(bans(CARGO, clippy).is_err());
    }

    #[test]
    fn row_without_reason_fails() {
        let doc = DOC.replace("Never panic (D-019).", "");
        let p = problems(CARGO, CLIPPY, &doc);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("clippy::unwrap_used") && p[0].contains("empty `Why`"));
    }

    #[test]
    fn level_drift_fails() {
        let cargo = CARGO.replace("todo = \"warn\"", "todo = \"deny\"");
        let p = problems(&cargo, CLIPPY, DOC);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("documented as `= warn`"), "{p:?}");
    }

    #[test]
    fn undocumented_switch_fails() {
        let clippy = format!("allow-panic-in-tests = true\n{CLIPPY}");
        let p = problems(CARGO, &clippy, DOC);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("`allow-panic-in-tests`"), "{p:?}");
    }

    #[test]
    fn missing_section_is_an_error() {
        let b = bans(CARGO, CLIPPY).unwrap();
        assert!(check(&b, "# C\n\n## Other\n").is_err());
    }
}
