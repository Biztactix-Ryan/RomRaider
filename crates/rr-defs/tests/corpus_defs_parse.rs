//! Every pinned definition file in the corpus parses without Error
//! diagnostics (US-RR-27, acceptance criterion 4; task US-RR-27-4; D-028).
//!
//! The file list is the manifest's, not a directory listing
//! ([`rr_testkit::corpus::defs::DefsPin`]): every `[[defs.files]]` entry
//! must be present in the fetched tree with its pinned size and SHA-256, so
//! a missing or changed file fails the test instead of shrinking it. Each
//! `.xml` entry (any case) is read with [`rr_defs::xml::parse`] and must
//! give a document with no Error diagnostic. The two `.dtd` entries are
//! checked against their pins but not parsed: a DTD is not an XML document,
//! and the reader never processes one (it reads past a DOCTYPE).
//!
//! Warnings and infos are allowed (the criterion is "without Error"; a
//! warning means the file was read with a well-defined fallback), and are
//! printed as a count per code, so a change in them shows in the output. At
//! the pin there are none at all.
//!
//! A failure lists every failing file with its first few diagnostics, not
//! only the first.
//!
//! The forum logger package `logger_v370.zip` (`[defs.logger_v370]`) is
//! never downloaded (D-028), so CI has no copy: its test reads the six
//! logger definitions inside it when a copy is present (`RR_LOGGER_ZIP`,
//! else `<corpus>/defs/logger_v370.zip`, as `cargo xtask fetch-corpus`
//! checks it), and otherwise prints that it skipped them. The public
//! tree's own logger definitions (v290) are in `[[defs.files]]` and always
//! read.
//!
//! Both tests are corpus-tier (`*::requires_RR_CORPUS_DIR`): skipped
//! without `RR_CORPUS_DIR`, run by CI's `corpus` job.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rr_core::diag::Diagnostic;
use rr_testkit::corpus::Corpus;
use rr_testkit::corpus::defs::{self, DefsError, DefsPin};

/// How many diagnostics of a failing file to show.
const SHOWN: usize = 5;

/// What reading a set of files found.
#[derive(Default)]
struct Tally {
    read: usize,
    /// `(file, problem lines)` for every file that failed.
    failures: Vec<(String, Vec<String>)>,
    /// Non-error diagnostics, by `severity[code]`.
    others: BTreeMap<String, usize>,
}

impl Tally {
    /// Parses one file and records the outcome.
    fn parse(&mut self, source: &str, bytes: &[u8]) {
        self.read += 1;
        let loaded = rr_defs::xml::parse(source, bytes);
        let errors: Vec<&Diagnostic> = loaded
            .diagnostics()
            .iter()
            .filter(|d| d.is_error())
            .collect();
        for d in loaded.diagnostics().iter().filter(|d| !d.is_error()) {
            *self
                .others
                .entry(format!("{}[{}]", d.severity(), d.code()))
                .or_insert(0) += 1;
        }
        let mut problems: Vec<String> =
            errors.iter().take(SHOWN).map(ToString::to_string).collect();
        if errors.len() > SHOWN {
            problems.push(format!("... and {} more error(s)", errors.len() - SHOWN));
        }
        if errors.is_empty() && loaded.value().is_none_or(Option::is_none) {
            problems.push("no document and no error diagnostic".to_owned());
        }
        if !problems.is_empty() {
            self.failures.push((source.to_owned(), problems));
        }
    }

    /// A file that could not be read at all.
    fn missing(&mut self, source: String, error: &DefsError) {
        self.failures.push((source, vec![error.to_string()]));
    }

    /// Prints the summary, then fails listing every failure.
    fn finish(self, what: &str) {
        let others = if self.others.is_empty() {
            "none".to_owned()
        } else {
            self.others
                .iter()
                .map(|(code, n)| format!("{code} x{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        eprintln!(
            "{what}: {} file(s) parsed, {} failed; warnings/infos: {others}",
            self.read,
            self.failures.len()
        );
        let mut report = format!(
            "{} of {what} failed (missing, not the pinned bytes, or Error diagnostics):\n",
            self.failures.len()
        );
        for (file, problems) in &self.failures {
            let _ = writeln!(report, "  {file}");
            for p in problems {
                let _ = writeln!(report, "    {p}");
            }
        }
        assert!(self.failures.is_empty(), "{report}");
    }
}

rr_testkit::corpus_test! {
    /// Every `[[defs.files]]` XML file is present, is the pinned bytes, and
    /// parses to a document with no Error diagnostic.
    fn every_pinned_definition_parses_without_errors() -> Result<(), DefsError> {
        let corpus = Corpus::open()?;
        let pin = DefsPin::load(corpus.manifest().path())?;
        assert!(!pin.files.is_empty(), "the manifest pins no definition files");
        let mut tally = Tally::default();
        let mut dtds = 0;
        for file in &pin.files {
            let source = file.corpus_path();
            match defs::read_def(corpus.dir(), file) {
                Ok(bytes) if file.is_xml() => tally.parse(&source, &bytes),
                Ok(_) => {
                    assert!(
                        file.path.to_ascii_lowercase().ends_with(".dtd"),
                        "{source}: pinned file that is neither XML nor a DTD; decide how to check it"
                    );
                    dtds += 1;
                }
                Err(e) => tally.missing(source, &e),
            }
        }
        let xml = pin.files.iter().filter(|f| f.is_xml()).count();
        eprintln!(
            "[defs] at {}: {} pinned file(s): {xml} XML, {dtds} DTD (pin-checked, not parsed)",
            pin.commit,
            pin.files.len()
        );
        tally.finish("the pinned definition files");
        assert_eq!(dtds + xml, pin.files.len());
        Ok(())
    }
}

rr_testkit::corpus_test! {
    /// Every logger definition in the pinned `logger_v370.zip` parses with no
    /// Error diagnostic, when a copy is present (never downloaded, D-028).
    fn logger_v370_definitions_parse_without_errors() -> Result<(), DefsError> {
        let corpus = Corpus::open()?;
        let pin = DefsPin::load(corpus.manifest().path())?;
        let Some((path, members)) = defs::read_logger_v370(corpus.dir(), &pin.logger_v370)? else {
            eprintln!(
                "SKIPPED: logger v370 not present (no {}; set {} to a private copy); \
                 it is never downloaded (D-028)",
                defs::logger_v370_path(corpus.dir(), &pin.logger_v370).display(),
                defs::LOGGER_ZIP_ENV
            );
            return Ok(());
        };
        let mut tally = Tally::default();
        for member in &members {
            let source = format!("{}!{}", pin.logger_v370.file, member.name);
            assert!(
                member.name.to_ascii_lowercase().ends_with(".xml"),
                "{source}: not an XML file; decide how to check it"
            );
            tally.parse(&source, &member.bytes);
        }
        assert!(tally.read > 0, "{}: no members", path.display());
        tally.finish(&format!("the logger definitions in {}", path.display()));
        Ok(())
    }
}
