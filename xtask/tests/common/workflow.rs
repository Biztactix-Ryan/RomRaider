//! A std-only reader for the subset of GitHub Actions YAML that
//! `.github/workflows/ci.yml` uses, shared by the ci-config guard tests
//! (`t6_ci_config.rs`, `ci_config_gates.rs`).
//!
//! The workspace has no YAML parser (and `cargo deny` keeps the dependency
//! set small), so [`Workflow`] reads: top-level keys at column 0, jobs at
//! two-space indent, steps as `- ` items at six-space indent, `run:` plain
//! and block scalars, and `needs:` as a flow or block list. Anything it cannot
//! find is reported as missing by the callers, so a layout change fails
//! closed rather than passing vacuously.

// Each test binary that includes this module uses a different subset of it.
#![allow(dead_code, reason = "shared by several test binaries")]

use std::fs;
use std::path::Path;

/// The aggregate job that branch protection requires.
pub const GATE_JOB: &str = "ci-ok";

/// The committed `.github/workflows/ci.yml` (an `<unreadable ...>` marker if
/// missing, which then fails every check).
pub fn ci_yml() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/ci.yml");
    fs::read_to_string(&path).unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// One job (or one step): its id and its raw lines (header excluded for a
/// job; the `- ` line included for a step).
pub struct Job<'a> {
    pub id: &'a str,
    pub lines: Vec<&'a str>,
}

/// The parsed subset of a workflow file.
pub struct Workflow<'a> {
    pub top_level_keys: Vec<&'a str>,
    /// Lines of each top-level section, by key (`on`, `env`, ...).
    pub sections: Vec<(&'a str, Vec<&'a str>)>,
    pub jobs: Vec<Job<'a>>,
}

pub fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

pub fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// `key:` at exactly `at` spaces of indent, returning the key.
pub fn key_at(line: &str, at: usize) -> Option<&str> {
    if indent(line) != at || is_blank_or_comment(line) {
        return None;
    }
    let t = line.trim_end();
    let key = t.trim_start().strip_suffix(':')?;
    (!key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some(key)
}

impl<'a> Workflow<'a> {
    pub fn parse(src: &'a str) -> Self {
        let mut top_level_keys = Vec::new();
        let mut sections: Vec<(&'a str, Vec<&'a str>)> = Vec::new();
        let mut jobs: Vec<Job<'a>> = Vec::new();
        let mut section = "";
        for line in src.lines() {
            if !is_blank_or_comment(line) && indent(line) == 0 {
                section = line.split(':').next().unwrap_or("").trim();
                top_level_keys.push(section);
                sections.push((section, Vec::new()));
                continue;
            }
            if let Some((_, lines)) = sections.last_mut() {
                lines.push(line);
            }
            if section == "jobs" {
                if let Some(id) = key_at(line, 2) {
                    jobs.push(Job {
                        id,
                        lines: Vec::new(),
                    });
                } else if let Some(job) = jobs.last_mut() {
                    job.lines.push(line);
                }
            }
        }
        Self {
            top_level_keys,
            sections,
            jobs,
        }
    }

    pub fn job(&self, id: &str) -> Option<&Job<'a>> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Lines of the top-level section `key` (empty if absent).
    pub fn section(&self, key: &str) -> &[&'a str] {
        self.sections
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(&[], |(_, lines)| lines.as_slice())
    }

    /// The `on:` block.
    pub fn on_block(&self) -> &[&'a str] {
        self.section("on")
    }
}

/// Collapses runs of whitespace to one space.
pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl<'a> Job<'a> {
    /// Every `run:` script in the job, whitespace-normalized. Block scalars
    /// (`|`, `>`, `>-`, ...) are joined; shell comment lines are dropped so a
    /// commented-out command does not count.
    pub fn run_scripts(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            let line = self.lines[i];
            let t = line.trim_start();
            let t = t.strip_prefix("- ").unwrap_or(t);
            let Some(value) = t.strip_prefix("run:") else {
                i += 1;
                continue;
            };
            let key_indent = indent(line);
            let value = value.trim();
            if value.starts_with('|') || value.starts_with('>') {
                let mut body = Vec::new();
                i += 1;
                while i < self.lines.len() {
                    let l = self.lines[i];
                    if !l.trim().is_empty() && indent(l) <= key_indent {
                        break;
                    }
                    if !l.trim_start().starts_with('#') {
                        body.push(l.trim());
                    }
                    i += 1;
                }
                out.push(normalize(&body.join("\n")));
            } else {
                out.push(normalize(value));
                i += 1;
            }
        }
        out
    }

    /// Lines that are not comments, trimmed.
    pub fn content_lines(&self) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .filter(|l| !is_blank_or_comment(l))
            .map(|l| l.trim())
    }

    /// Job ids in this job's `needs:` (flow list, block list or scalar).
    pub fn needs(&self) -> Vec<String> {
        let Some(pos) = self
            .lines
            .iter()
            .position(|l| indent(l) == 4 && l.trim_start().starts_with("needs:"))
        else {
            return Vec::new();
        };
        let first = self.lines[pos].trim_start()["needs:".len()..].trim();
        let items: Vec<String> = if let Some(flow) = first.strip_prefix('[') {
            let mut text = flow.to_owned();
            let mut j = pos + 1;
            while !text.contains(']') && j < self.lines.len() {
                text.push(' ');
                text.push_str(self.lines[j].trim());
                j += 1;
            }
            let text = text.split(']').next().unwrap_or("");
            text.split(',').map(|s| s.trim().to_owned()).collect()
        } else if first.is_empty() {
            self.lines[pos + 1..]
                .iter()
                .take_while(|l| is_blank_or_comment(l) || indent(l) > 4)
                .filter_map(|l| l.trim().strip_prefix("- "))
                .map(|s| s.trim().to_owned())
                .collect()
        } else {
            vec![first.to_owned()]
        };
        items
            .into_iter()
            .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// The job's steps, in order: each `- ` item at six-space indent under
    /// `steps:`. A step's `id` is its `name:` (or `uses:`), else "".
    pub fn steps(&self) -> Vec<Job<'a>> {
        let Some(start) = self
            .lines
            .iter()
            .position(|l| indent(l) == 4 && l.trim() == "steps:")
        else {
            return Vec::new();
        };
        let mut steps: Vec<Job<'a>> = Vec::new();
        for &line in &self.lines[start + 1..] {
            if !is_blank_or_comment(line) && indent(line) <= 4 {
                break;
            }
            if indent(line) == 6 && line.trim_start().starts_with("- ") {
                steps.push(Job {
                    id: "",
                    lines: Vec::new(),
                });
            }
            if let Some(step) = steps.last_mut() {
                step.lines.push(line);
            }
        }
        for step in &mut steps {
            step.id = step
                .lines
                .iter()
                .map(|l| l.trim_start().trim_start_matches("- "))
                .find_map(|l| l.strip_prefix("name:").or_else(|| l.strip_prefix("uses:")))
                .map_or("", str::trim);
        }
        steps
    }

    /// The value of `key:` at exactly `at` spaces of indent (or as the first
    /// key of a `- ` item at `at - 2`), unquoted and trimmed.
    pub fn value_at(&self, key: &str, at: usize) -> Option<String> {
        self.lines.iter().find_map(|l| {
            let t = if indent(l) == at {
                l.trim_start()
            } else if indent(l) + 2 == at {
                l.trim_start().strip_prefix("- ")?
            } else {
                return None;
            };
            let v = t.strip_prefix(key)?.strip_prefix(':')?.trim();
            Some(v.trim_matches(|c| c == '"' || c == '\'').to_owned())
        })
    }
}

/// The first non-comment line whose key is `key`, at any indent.
pub fn key_at_any<'a>(lines: &[&'a str], key: &str) -> Option<&'a str> {
    lines
        .iter()
        .copied()
        .find(|l| !is_blank_or_comment(l) && l.trim_start().split(':').next() == Some(key))
}

/// Problems if the workflow does not run on both `push` and `pull_request`.
pub fn trigger_problems(wf: &Workflow<'_>) -> Vec<String> {
    ["push", "pull_request"]
        .into_iter()
        .filter(|t| key_at_any(wf.on_block(), t).is_none())
        .map(|t| format!("workflow does not run on `{t}`"))
        .collect()
}

/// Problems if `job` is not in the aggregate gate's `needs`.
pub fn gate_needs_problems(wf: &Workflow<'_>, job: &str) -> Vec<String> {
    match wf.job(GATE_JOB) {
        None => vec![format!("no `{GATE_JOB}` job")],
        Some(gate) if !gate.needs().iter().any(|n| n == job) => vec![format!(
            "`{GATE_JOB}.needs` does not include `{job}` (needs: {:?})",
            gate.needs()
        )],
        Some(_) => Vec::new(),
    }
}

/// Problems if any step of `job` can be skipped or softened: `if:` (other
/// than on the steps named in `allowed_if`, which must be `if: always()`),
/// `continue-on-error`, or a script that swallows failures.
pub fn skippable_problems(job: &Job<'_>, allowed_always: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    let id = job.id;
    for line in job.content_lines() {
        let l = line.strip_prefix("- ").unwrap_or(line);
        if l.starts_with("continue-on-error") {
            problems.push(format!("`{id}` uses `{l}`"));
        }
    }
    for step in job.steps() {
        for line in step.content_lines() {
            let l = line.strip_prefix("- ").unwrap_or(line);
            if l.starts_with("if:") {
                let ok = allowed_always.contains(&step.id) && normalize(l) == "if: always()";
                if !ok {
                    problems.push(format!(
                        "`{id}` step `{}` has a conditional (`{l}`)",
                        step.id
                    ));
                }
            }
        }
    }
    for s in job.run_scripts() {
        if s.contains("|| true") || s.contains("|| :") || s.contains("exit 0") {
            problems.push(format!("`{id}` step swallows failures: {s}"));
        }
    }
    problems
}

/// Applies `edit` to the real ci.yml (asserting it changed something) and
/// returns what `check` finds in the result.
pub fn mutated(check: impl Fn(&str) -> Vec<String>, edit: impl Fn(&str) -> String) -> Vec<String> {
    let src = ci_yml();
    let changed = edit(&src);
    assert_ne!(changed, src, "mutation did not apply");
    check(&changed)
}

pub fn assert_flags(problems: &[String], needle: &str) {
    assert!(
        problems.iter().any(|p| p.contains(needle)),
        "expected a problem mentioning {needle:?}, got {problems:#?}"
    );
}
