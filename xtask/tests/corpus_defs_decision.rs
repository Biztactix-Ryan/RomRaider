//! US-RR-16-1 (D-028): the ECU-definition source, its licence position and
//! the pinned versions are recorded in both `.project/DECISIONS.md` (D-028)
//! and `fixtures/corpus-manifest.toml` (`[defs]`, `[defs.logger_v370]`), and
//! the two agree. The values are read from the D-028 `**Decision:**` text,
//! not restated here, so moving a pin means changing both files together.
//! The tamper tests prove each cross-check fails when one side drifts.

use std::fs;
use std::path::{Path, PathBuf};

use xtask::corpus_manifest::{COMMITTED_PATH, Manifest};

const DECISIONS_PATH: &str = ".project/DECISIONS.md";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn committed() -> anyhow::Result<(String, Manifest)> {
    let root = workspace_root();
    let decisions = fs::read_to_string(root.join(DECISIONS_PATH))?;
    let manifest = Manifest::from_toml(&fs::read_to_string(root.join(COMMITTED_PATH))?)?;
    Ok((decisions, manifest))
}

/// The body of the `## D-028` entry, up to the next `## ` heading.
fn d028(decisions: &str) -> Option<&str> {
    let start = decisions.find("\n## D-028:")? + 1;
    let rest = &decisions[start..];
    let end = rest[1..].find("\n## ").map_or(rest.len(), |i| i + 1);
    Some(&rest[..end])
}

/// The text between `start` and the next `end` after it.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let len = text[from..].find(end)?;
    Some(&text[from..from + len])
}

/// Licence positions D-028 and the manifest's `licence` must both state:
/// (D-028 phrase, manifest phrase).
const LICENCE_POSITION: [(&str, &str); 3] = [
    ("No licence is stated", "none stated"),
    (
        "all rights stay with the authors",
        "all rights stay with the authors",
    ),
    (
        "must not vendor, publish or redistribute",
        "never vendored, published or redistributed",
    ),
];

/// Every way D-028 and the manifest disagree on the definition source, the
/// pinned commit, the licence position or the logger v370 pin, or either
/// side missing. Empty when they agree.
fn provenance_problems(decisions: &str, manifest: &Manifest) -> Vec<String> {
    let mut p = Vec::new();
    let Some(entry) = d028(decisions) else {
        return vec!["DECISIONS.md has no `## D-028:` entry".to_owned()];
    };
    let Some(defs) = manifest.defs.as_ref() else {
        return vec!["the manifest has no [defs] table".to_owned()];
    };
    let Some(decision) = between(entry, "- **Decision:**", "- **Consequences:**") else {
        return vec!["D-028 has no `**Decision:**` before `**Consequences:**`".to_owned()];
    };

    // Source: "The ECU-definition corpus is the `<prefix>` subtree of <url>,".
    match between(decision, "subtree of ", ",") {
        Some(url) if url == defs.upstream => {}
        other => p.push(format!(
            "source: D-028 says {other:?}, manifest upstream is {:?}",
            defs.upstream
        )),
    }
    match between(decision, "corpus is the `", "` subtree") {
        Some(prefix) if prefix == defs.path_prefix => {}
        other => p.push(format!(
            "subtree: D-028 says {other:?}, manifest path_prefix is {:?}",
            defs.path_prefix
        )),
    }
    // Pin: "pinned at commit `<sha>` (branch `<branch>`, ...".
    match between(decision, "pinned at commit `", "`") {
        Some(c) if c == defs.commit => {}
        other => p.push(format!(
            "commit: D-028 says {other:?}, manifest commit is {:?}",
            defs.commit
        )),
    }
    match between(decision, "(branch `", "`") {
        Some(b) if b == defs.branch => {}
        other => p.push(format!(
            "branch: D-028 says {other:?}, manifest branch is {:?}",
            defs.branch
        )),
    }

    // Licence position, in D-028 and in the manifest's `licence`.
    if !entry.contains("**Licence:**") {
        p.push("D-028 has no `**Licence:**` finding".to_owned());
    }
    for (in_decision, in_manifest) in LICENCE_POSITION {
        if !entry.contains(in_decision) {
            p.push(format!("licence: D-028 does not say {in_decision:?}"));
        }
        if !defs.licence.contains(in_manifest) {
            p.push(format!(
                "licence: manifest licence {:?} does not say {in_manifest:?}",
                defs.licence
            ));
        }
    }
    if !defs.licence.contains("D-028") {
        p.push("licence: manifest licence does not cite D-028".to_owned());
    }

    // Logger v370: "`<file>` (<url>), pinned by size <n> bytes and SHA-256 `<sha>`".
    let logger = &defs.logger_v370;
    match between(decision, "pinned by size ", " bytes") {
        Some(n) if n.replace(',', "").parse::<u64>().ok() == Some(logger.size) => {}
        other => p.push(format!(
            "logger size: D-028 says {other:?}, manifest size is {}",
            logger.size
        )),
    }
    match between(decision, "bytes and SHA-256 `", "`") {
        Some(h) if h == logger.sha256 => {}
        other => p.push(format!(
            "logger SHA-256: D-028 says {other:?}, manifest sha256 is {:?}",
            logger.sha256
        )),
    }
    match between(decision, &format!("`{}` (", logger.file), ")") {
        Some(u) if u == logger.url => {}
        other => p.push(format!(
            "logger url: D-028 says {other:?} for `{}`, manifest url is {:?}",
            logger.file, logger.url
        )),
    }
    p
}

#[test]
fn d028_and_the_manifest_agree_on_source_licence_and_pins() -> anyhow::Result<()> {
    let (decisions, manifest) = committed()?;
    let problems = provenance_problems(&decisions, &manifest);
    assert!(problems.is_empty(), "{problems:#?}");
    // Both sides hold the values (not just agree on empty ones).
    let defs = manifest.defs.as_ref().expect("checked above");
    assert!(!defs.upstream.is_empty() && !defs.commit.is_empty());
    assert_eq!(defs.commit.len(), 40, "{}", defs.commit);
    assert_eq!(defs.logger_v370.sha256.len(), 64);
    assert!(defs.logger_v370.size > 0);
    Ok(())
}

/// Replaces the single occurrence of `from` in `text` (so a tamper that
/// silently matches nothing fails the test rather than passing it).
fn replace_once(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from:?} must occur once");
    text.replacen(from, to, 1)
}

/// Asserts the tampered pair is caught with a problem naming `topic`.
fn assert_caught(decisions: &str, manifest: &Manifest, topic: &str) {
    let problems = provenance_problems(decisions, manifest);
    assert!(
        problems.iter().any(|p| p.contains(topic)),
        "tamper on {topic} not caught: {problems:#?}"
    );
}

#[test]
fn a_tampered_decisions_entry_is_caught() -> anyhow::Result<()> {
    let (decisions, manifest) = committed()?;
    let defs = manifest.defs.as_ref().expect("[defs]");
    let lg = &defs.logger_v370;
    let entry = d028(&decisions).expect("D-028");
    // As written in D-028, with thousands separators (986,211).
    let size_text = between(entry, "pinned by size ", " bytes").expect("logger size");
    for (from, to, topic) in [
        (
            format!("subtree of {},", defs.upstream),
            "subtree of https://github.com/Merp/SubaruDefs,".to_owned(),
            "source",
        ),
        (
            format!("commit `{}`", defs.commit),
            format!("commit `{}`", "0".repeat(40)),
            "commit",
        ),
        (
            format!("size {size_text} bytes"),
            format!("size {} bytes", lg.size + 1),
            "logger size",
        ),
        (
            format!("SHA-256 `{}`", lg.sha256),
            format!("SHA-256 `{}`", "f".repeat(64)),
            "logger SHA-256",
        ),
        (
            "No licence is stated".to_owned(),
            "The licence is MIT".to_owned(),
            "licence",
        ),
    ] {
        let tampered_entry = replace_once(entry, &from, &to);
        let tampered = decisions.replacen(entry, &tampered_entry, 1);
        assert_caught(&tampered, &manifest, topic);
    }
    // D-028 missing altogether.
    let gone = decisions.replacen("## D-028:", "## D-0XX:", 1);
    assert_caught(&gone, &manifest, "D-028");
    Ok(())
}

#[test]
fn a_tampered_manifest_is_caught() -> anyhow::Result<()> {
    let (decisions, manifest) = committed()?;
    type Tamper = fn(&mut xtask::corpus_manifest::DefsCorpus);
    let tampers: [(Tamper, &str); 7] = [
        (
            |d| d.upstream = "https://github.com/Merp/SubaruDefs".into(),
            "source",
        ),
        (|d| d.path_prefix = "ECUFlash/".into(), "subtree"),
        (|d| d.commit = "0".repeat(40), "commit"),
        (|d| d.licence = "GPL-2.0-or-later (D-028)".into(), "licence"),
        (|d| d.logger_v370.size += 1, "logger size"),
        (|d| d.logger_v370.sha256 = "f".repeat(64), "logger SHA-256"),
        (|d| d.logger_v370.url.push('0'), "logger url"),
    ];
    for (tamper, topic) in tampers {
        let mut m = manifest.clone();
        tamper(m.defs.as_mut().expect("[defs]"));
        assert_caught(&decisions, &m, topic);
    }
    // [defs] missing altogether.
    let mut m = manifest;
    m.defs = None;
    assert_caught(&decisions, &m, "[defs]");
    Ok(())
}
