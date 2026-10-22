//! US-RR-21-3 (acceptance criterion of US-RR-21): every unit string
//! appearing in the pinned logger and ECU definition corpora resolves to a
//! registry unit (or the documented no-unit / unstated-pressure outcome of
//! [`units::legacy_unit`]) or produces a Warning diagnostic (`UNT0001`)
//! whose message quotes the unknown string, located at its first
//! occurrence (file and attribute-value span). Built in US-RR-21-5.
//!
//! Where legacy reads a unit string, and so where this test looks:
//!
//! * ECU definitions: the `units` attribute of `<scaling>`
//!   (`legacy/src/main/java/com/romraider/xml/TableScaleUnmarshaller.java:450`,
//!   attribute name compared ignoring case; also the scaling's fallback
//!   name, :439).
//! * Logger definitions: the `units` attribute of `<conversion>` and of
//!   `<switch>`
//!   (`legacy/src/main/java/com/romraider/logger/ecu/definition/xml/LoggerDefinitionHandler.java:248`
//!   and `:280`).
//!
//! There is no logger-definition reader yet (US-RR-30), so the test takes
//! a superset: the `units` attribute (name in any ASCII case, as legacy's
//! scaling reader) of every element of every pinned XML file, whatever the
//! element (`<scaling>`, `<conversion>`, `<switch>`, `<table>`, and a
//! misspelt `<scalin>`). No other attribute or element of either corpus
//! carries a unit (checked by hand: `units` is the only attribute name
//! containing `unit`, in any case, in both).
//!
//! The strings found are committed, metadata only, as
//! `fixtures/corpus-unit-strings.txt` (format in its header). rr-core's
//! no-corpus test `units::legacy::tests::every_corpus_string_resolves_or_warns`
//! reads every string of that list, `v370` ones included, in every CI run
//! and pins the strings that warn. This test fails when the list is stale:
//! a string of the corpus missing from it, or listed but no longer found,
//! or listed under the wrong sources.
//!
//! `logger_v370.zip` is never downloaded (D-028) and CI has no copy: there
//! the `v370` sources are not compared and this test prints
//! `[units] logger_v370.zip: SKIPPED ...`, which CI's `corpus` job turns into
//! a workflow warning (step `Corpus unit strings (logger_v370.zip)`); with a
//! copy it prints `[units] logger_v370.zip: N member(s) scanned`. The
//! logger's strings are then covered in CI only through the committed list.
//!
//! Regenerate the list (with the zip present) with
//!
//! ```text
//! RR_CORPUS_DIR=~/.cache/romraider-rs/corpus RR_UPDATE_UNIT_STRINGS=1 \
//!     cargo nextest run -p rr-defs --test corpus_unit_strings
//! ```
//!
//! The XML is read with rr-defs' hardened reader ([`rr_defs::xml`]).
//!
//! Corpus-tier (`*::requires_RR_CORPUS_DIR`): skipped without
//! `RR_CORPUS_DIR`, run by CI's `corpus` job.

use std::collections::{BTreeMap, BTreeSet};

use rr_core::diag::{Provenance, Severity, codes};
use rr_core::units;
use rr_defs::xml::Element;
use rr_testkit::corpus::Corpus;
use rr_testkit::corpus::defs::{self, DefsError, DefsPin};
use rr_testkit::fixtures::repo_root;

/// The committed list, relative to the repository root.
const LIST: &str = "fixtures/corpus-unit-strings.txt";
/// Set to `1` to rewrite [`LIST`] from the corpus.
const UPDATE_ENV: &str = "RR_UPDATE_UNIT_STRINGS";
/// The source tag of the pinned definition tree.
const DEFS: &str = "defs";
/// The source tag of the logger package.
const V370: &str = "v370";

/// String -> the sources it occurs in.
type Found = BTreeMap<String, BTreeSet<&'static str>>;
/// String -> where it first occurs: file and attribute-value span.
type FirstSeen = BTreeMap<String, Provenance>;

/// Adds every `units` attribute (name in any ASCII case) of `element` and
/// its descendants, found in `source`.
fn collect(
    element: &Element,
    source: &str,
    tag: &'static str,
    found: &mut Found,
    first: &mut FirstSeen,
) {
    for attr in element
        .attributes()
        .iter()
        .filter(|a| a.name().eq_ignore_ascii_case("units"))
    {
        let text = attr.value();
        found.entry(text.to_owned()).or_default().insert(tag);
        first
            .entry(text.to_owned())
            .or_insert_with(|| Provenance::new(source, attr.value_span()));
    }
    for child in element.elements() {
        collect(child, source, tag, found, first);
    }
}

/// Reads `bytes` as XML and adds its unit strings, or says why not.
fn scan(
    source: &str,
    bytes: &[u8],
    tag: &'static str,
    found: &mut Found,
    first: &mut FirstSeen,
) -> Result<(), String> {
    let parsed = rr_defs::xml::parse(source, bytes);
    let Some(Some(doc)) = parsed.value() else {
        return Err(format!(
            "{source}: not well-formed: {:?}",
            parsed.diagnostics()
        ));
    };
    collect(doc.root(), source, tag, found, first);
    Ok(())
}

/// One line of [`LIST`].
fn line(text: &str, sources: &BTreeSet<&str>) -> String {
    let sources: Vec<&str> = sources.iter().copied().collect();
    format!("{} |{text}|", sources.join(","))
}

/// The committed list: its header comment and its entries.
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn committed() -> (String, Found) {
    let path = repo_root().join(LIST);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut header = String::new();
    let mut found = Found::new();
    for l in text.lines() {
        if l.starts_with('#') {
            header.push_str(l);
            header.push('\n');
            continue;
        }
        let parsed = l.split_once(' ').and_then(|(sources, quoted)| {
            let s = quoted.strip_prefix('|')?.strip_suffix('|')?;
            let tags = sources
                .split(',')
                .map(|t| [DEFS, V370].into_iter().find(|k| *k == t))
                .collect::<Option<BTreeSet<_>>>()?;
            Some((s.to_owned(), tags))
        });
        let Some((s, tags)) = parsed else {
            panic!("{LIST}: malformed line {l:?}");
        };
        assert!(
            found.insert(s, tags).is_none(),
            "{LIST}: {l:?} listed twice"
        );
    }
    (header, found)
}

rr_testkit::corpus_test! {
    /// US-RR-21-3: every unit string of the pinned ECU and logger
    /// definition corpora reads as a unit, or as an `UNT0001` Warning whose
    /// message quotes it, at its first occurrence; and the committed list
    /// is exactly those strings.
    fn corpus_unit_strings_are_listed_and_resolve_or_warn() -> Result<(), DefsError> {
        let corpus = Corpus::open()?;
        let pin = DefsPin::load(corpus.manifest().path())?;
        let mut found = Found::new();
        let mut first = FirstSeen::new();
        let mut failures = Vec::new();
        for file in pin.files.iter().filter(|f| f.is_xml()) {
            let bytes = defs::read_def(corpus.dir(), file)?;
            if let Err(e) = scan(&file.corpus_path(), &bytes, DEFS, &mut found, &mut first) {
                failures.push(e);
            }
        }
        let zip = defs::read_logger_v370(corpus.dir(), &pin.logger_v370)?;
        if let Some((_, members)) = &zip {
            let xml: Vec<_> = members
                .iter()
                .filter(|m| m.name.to_ascii_lowercase().ends_with(".xml"))
                .collect();
            assert!(!xml.is_empty(), "{}: no XML member", pin.logger_v370.file);
            for member in &xml {
                let source = format!("{}!{}", pin.logger_v370.file, member.name);
                if let Err(e) = scan(&source, &member.bytes, V370, &mut found, &mut first) {
                    failures.push(e);
                }
            }
            eprintln!(
                "[units] logger_v370.zip: {} member(s) scanned",
                xml.len()
            );
        } else {
            eprintln!(
                "[units] logger_v370.zip: SKIPPED: no copy at {} (set {}; never downloaded, \
                 D-028); its strings are checked only through the committed {LIST}",
                defs::logger_v370_path(corpus.dir(), &pin.logger_v370).display(),
                defs::LOGGER_ZIP_ENV
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));

        // Every string resolves, or is a Warning quoting it, at its first
        // occurrence.
        let mut resolved = 0;
        let mut warned = 0;
        for (text, at) in &first {
            match units::legacy_unit(text) {
                Ok(_) => resolved += 1,
                Err(e) => {
                    assert_eq!(&e.text, text);
                    let d = e.to_diagnostic(at.clone());
                    assert_eq!(d.severity(), Severity::Warning, "{text:?}");
                    assert_eq!(d.code(), codes::UNKNOWN_UNIT_STRING, "{text:?}");
                    assert_eq!(d.provenance(), at, "{text:?}");
                    // Quoted (`{:?}`), so a string with a trailing space or
                    // a quote in it is unambiguous.
                    assert!(
                        d.message().contains(&format!("{text:?}")),
                        "{text:?}: {}",
                        d.message()
                    );
                    warned += 1;
                }
            }
        }
        assert_eq!(first.len(), found.len());
        eprintln!(
            "[units] at {}: {} distinct unit string(s): {resolved} resolve, {warned} warn (UNT0001)",
            pin.commit,
            found.len()
        );

        let (header, listed) = committed();
        if std::env::var(UPDATE_ENV).is_ok_and(|v| v == "1") {
            assert!(zip.is_some(), "{UPDATE_ENV}=1 needs a copy of logger_v370.zip");
            let mut text = header;
            for (s, tags) in &found {
                text.push_str(&line(s, tags));
                text.push('\n');
            }
            let path = repo_root().join(LIST);
            std::fs::write(&path, text).map_err(|source| DefsError::Io { path, source })?;
            eprintln!("[units] wrote {LIST}");
            return Ok(());
        }
        // Without the zip, compare the `defs` sources only.
        let view = |map: &Found| -> Found {
            map.iter()
                .filter_map(|(s, tags)| {
                    let tags: BTreeSet<_> =
                        tags.iter().copied().filter(|t| zip.is_some() || *t == DEFS).collect();
                    (!tags.is_empty()).then(|| (s.clone(), tags))
                })
                .collect()
        };
        let (have, want) = (view(&found), view(&listed));
        let stale: Vec<String> = have
            .iter()
            .filter(|(s, tags)| want.get(*s) != Some(tags))
            .map(|(s, tags)| format!("corpus has  {}", line(s, tags)))
            .chain(
                want.iter()
                    .filter(|(s, tags)| have.get(*s) != Some(tags))
                    .map(|(s, tags)| format!("list has    {}", line(s, tags))),
            )
            .collect();
        assert!(
            stale.is_empty(),
            "{LIST} is stale; regenerate it with {UPDATE_ENV}=1 (module docs):\n{}",
            stale.join("\n")
        );
        Ok(())
    }
}
