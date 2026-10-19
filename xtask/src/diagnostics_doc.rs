//! `docs/diagnostics.md`: generated from the diagnostic code registry
//! (US-RR-17-6).
//!
//! The registry is `rr_core::diag::codes`; its `render_markdown` is the
//! single source of the page. `cargo xtask diagnostics-doc` (re)writes the
//! committed file; `cargo xtask diagnostics-doc --check` and the integration
//! test `xtask/tests/diagnostics_doc.rs` (run by CI's nextest `test` job)
//! fail when the registry breaks a rule (duplicate code, malformed code,
//! missing doc, ...) or the committed file differs from what the registry
//! renders.

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use rr_core::diag::codes;

/// The generated page, relative to the workspace root.
pub const DOC_PATH: &str = "docs/diagnostics.md";

/// Registry rule violations, as messages. Empty when the registry is sound.
#[must_use]
pub fn registry_problems() -> Vec<String> {
    codes::problems(codes::REGISTRY, codes::DEPRECATED, codes::DOMAINS)
        .iter()
        .map(|p| format!("registry: {p}"))
        .collect()
}

/// Compares a committed page (`None` when the file is missing) with the
/// `expected` rendering. Line endings are normalised, so a Windows checkout
/// with `\r\n` does not count as drift. Empty when they match.
#[must_use]
pub fn drift(committed: Option<&str>, expected: &str) -> Vec<String> {
    let fix = "run `cargo xtask diagnostics-doc` and commit the result";
    let Some(committed) = committed else {
        return vec![format!("{DOC_PATH} is missing; {fix}")];
    };
    let committed = committed.replace("\r\n", "\n");
    if committed == expected {
        return Vec::new();
    }
    let mut got = committed.lines();
    let mut want = expected.lines();
    let mut line = 1usize;
    loop {
        match (got.next(), want.next()) {
            (Some(g), Some(w)) if g == w => line += 1,
            (g, w) => {
                return vec![format!(
                    "{DOC_PATH} is out of date with the registry \
                     (first difference at line {line}: committed {g:?}, \
                     generated {w:?}); {fix}"
                )];
            }
        }
    }
}

fn read_committed(root: &Path) -> Result<Option<String>> {
    let path = root.join(DOC_PATH);
    match fs::read_to_string(&path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Every problem with the registry and the committed page under `root`.
pub fn check(root: &Path) -> Result<Vec<String>> {
    let mut out = registry_problems();
    let committed = read_committed(root)?;
    out.extend(drift(committed.as_deref(), &codes::render_markdown()));
    Ok(out)
}

/// Writes the page under `root`. Returns `true` if the file changed. Refuses
/// (and writes nothing) while the registry breaks a rule.
pub fn write(root: &Path) -> Result<bool> {
    let problems = registry_problems();
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    let expected = codes::render_markdown();
    let committed = read_committed(root)?;
    if drift(committed.as_deref(), &expected).is_empty() {
        return Ok(false);
    }
    let path = root.join(DOC_PATH);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(&path, expected).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}
