//! Every definition file in the pinned definition corpus loads (US-RR-29,
//! acceptance criterion 1; task US-RR-29-1; D-028): any Error diagnostic is
//! either fixed or recorded as a known-bad fixture with a reason.
//!
//! # What is checked
//!
//! [`every_pinned_definition_file_loads`] walks the manifest's
//! `[[defs.files]]` (not a directory listing: a missing or changed file
//! fails, through [`defs::read_def`]'s size and SHA-256 check) and accounts
//! for every entry:
//!
//! * a `.dtd` is not a document: pin-checked only;
//! * an XML file is parsed ([`rr_defs::xml::parse`]); by its root element
//!   it is then an ECU definition (`<roms>` or `<rom>`, any case), read with
//!   [`romraider::read_document`], or a file this reader does not read,
//!   whose root must be listed under `[[out_of_scope]]` in the registry
//!   with the reader that will read it and why (any other root fails);
//! * the ECU definitions form one [`DefinitionSet`] in manifest order, and
//!   every `<rom>` of it is resolved along its `base` chain into typed
//!   tables (preferred scaling `Metric`, as legacy's default).
//!
//! The members of the `logger_v370.zip` pin are classified the same way
//! when a copy is present (never downloaded, D-028); otherwise the test
//! prints that it skipped them.
//!
//! Every Error diagnostic of every stage (XML, reading, the set, resolving)
//! must be one listed in the known-bad registry, `fixtures/defs-known-bad.toml`,
//! and every Error listed there must still be reported: a new Error fails,
//! and so does a known-bad entry that no longer errors (its file was fixed
//! or the reader changed: remove or update the entry). Errors are compared
//! as source, code, `line:col` and layer, as a multiset.
//!
//! It prints a summary: files by kind, `<rom>`s and how they are
//! identified, `<table>`s by kind as read and as resolved, axes, chain
//! lengths, and warnings by code over all stages. (It replaces the stage
//! smokes of US-RR-29-5, -6 and -8, whose counts it prints; the scaling
//! smoke, `corpus_romraider_scalings.rs`, stays: it analyses the
//! conversions, which this test does not.)
//!
//! Corpus-tier (`*::requires_RR_CORPUS_DIR`): skipped without
//! `RR_CORPUS_DIR`, run by CI's `corpus` job.
//!
//! # The registry
//!
//! [`the_known_bad_registry_is_valid`] runs on every `cargo test`, with no
//! corpus: it checks the committed registry against the committed manifest
//! ([`validate`]): unknown keys are rejected (`deny_unknown_fields`), every
//! known-bad `file` must be a pinned XML file whose `sha256` is the pinned
//! one (so a corpus bump makes each entry be reviewed again), every code a
//! registered Error code, every `reason` and `legacy` non-empty, and no
//! entry, Error or root listed twice.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod loads {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use rr_core::diag::codes::REGISTRY;
    use rr_core::diag::{Diagnostic, Severity, Span};
    use rr_defs::romraider::{self, DefinitionFile, DefinitionSet, Identification, TableDoc};
    use rr_defs::xml::Document;
    use rr_testkit::corpus::defs::{self, DefFile, DefsError, DefsPin};
    use rr_testkit::corpus::{Corpus, MANIFEST_PATH};
    use rr_testkit::fixtures::{FixtureError, parse_kat, repo_root};
    use serde::Deserialize;

    /// The registry, relative to the repository root.
    const REGISTRY_PATH: &str = "fixtures/defs-known-bad.toml";

    /// The registry file (every top-level key but `schema`).
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Registry {
        /// Pinned files with Errors of their own.
        #[serde(default)]
        known_bad: Vec<KnownBad>,
        /// Root elements of pinned XML this reader does not read.
        #[serde(default)]
        out_of_scope: Vec<OutOfScope>,
    }

    /// One known-bad pinned file.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct KnownBad {
        /// The path as pinned in `[[defs.files]]`.
        file: String,
        /// The manifest's SHA-256 of it.
        sha256: String,
        /// What is wrong with the file, and why it is not fixed here.
        reason: String,
        /// What legacy RomRaider did with it, with references into legacy/.
        legacy: String,
        /// Every Error reported in it, exactly.
        error: Vec<ExpectedError>,
    }

    /// One expected Error.
    #[derive(Debug, Clone, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedError {
        /// `DEF0024`.
        code: String,
        /// 1-based.
        line: u32,
        /// 1-based.
        column: u32,
        /// The layer the diagnostic names, if it names one.
        #[serde(default)]
        layer: Option<String>,
    }

    /// A root element this reader does not read.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OutOfScope {
        /// The root element's name (compared in any case).
        root: String,
        /// The reader (story) that will read it.
        reader: String,
        /// Why it is not an ECU definition.
        reason: String,
    }

    impl Registry {
        fn out_of_scope(&self, root: &str) -> Option<&OutOfScope> {
            self.out_of_scope
                .iter()
                .find(|o| o.root.eq_ignore_ascii_case(root))
        }
    }

    /// One Error, as compared: source, code, line, column, layer.
    type Key = (String, String, u32, u32, Option<String>);

    fn parse_registry(path: impl Into<PathBuf>, text: &str) -> Result<Registry, FixtureError> {
        parse_kat::<Registry>(path, text).map(|f| f.data)
    }

    fn load_registry() -> Registry {
        let path = repo_root().join(REGISTRY_PATH);
        let text = std::fs::read_to_string(&path).expect("read the known-bad registry");
        parse_registry(path, &text).expect("the known-bad registry parses")
    }

    fn committed_pin() -> DefsPin {
        DefsPin::load(&repo_root().join(MANIFEST_PATH)).expect("load [defs] of the manifest")
    }

    fn is_ecu_root(root: &str) -> bool {
        root.eq_ignore_ascii_case("roms") || root.eq_ignore_ascii_case("rom")
    }

    /// Every problem with `registry` against the pinned definitions `pin`.
    fn validate(registry: &Registry, pin: &DefsPin) -> Vec<String> {
        let mut problems = Vec::new();
        let mut files = BTreeSet::new();
        for entry in &registry.known_bad {
            let at = &entry.file;
            if !files.insert(at.as_str()) {
                problems.push(format!("{at}: listed twice"));
            }
            match pin.files.iter().find(|f| f.path == *at) {
                None => problems.push(format!("{at}: not a [[defs.files]] entry of the manifest")),
                Some(f) if !f.is_xml() => problems.push(format!("{at}: not an XML file")),
                Some(f) if f.sha256 != entry.sha256 => problems.push(format!(
                    "{at}: sha256 {} is not the pinned {}: the file changed, review the entry \
                     again against the new pin",
                    entry.sha256, f.sha256
                )),
                Some(_) => {}
            }
            if entry.reason.trim().is_empty() {
                problems.push(format!("{at}: empty reason"));
            }
            if entry.legacy.trim().is_empty() {
                problems.push(format!(
                    "{at}: empty legacy (what legacy RomRaider did with it)"
                ));
            }
            if entry.error.is_empty() {
                problems.push(format!("{at}: lists no Error"));
            }
            let mut seen = BTreeSet::new();
            for e in &entry.error {
                let here = format!("{at}:{}:{} {}", e.line, e.column, e.code);
                match REGISTRY.iter().find(|i| i.code.as_str() == e.code) {
                    None => problems.push(format!("{here}: not a registered diagnostic code")),
                    Some(i) if i.severity != Severity::Error => problems.push(format!(
                        "{here}: a {} code; only Errors are listed",
                        i.severity
                    )),
                    Some(_) => {}
                }
                if e.line == 0 || e.column == 0 {
                    problems.push(format!("{here}: line and column are 1-based"));
                }
                if !seen.insert((&e.code, e.line, e.column, &e.layer)) {
                    problems.push(format!("{here}: listed twice"));
                }
            }
        }
        let mut roots = BTreeSet::new();
        for o in &registry.out_of_scope {
            let root = o.root.to_ascii_lowercase();
            if root.trim().is_empty() || is_ecu_root(&root) {
                problems.push(format!(
                    "out_of_scope root {:?}: not a root this reader skips",
                    o.root
                ));
            }
            if !roots.insert(root) {
                problems.push(format!("out_of_scope root {:?}: listed twice", o.root));
            }
            if o.reader.trim().is_empty() || o.reason.trim().is_empty() {
                problems.push(format!(
                    "out_of_scope root {:?}: empty reader or reason",
                    o.root
                ));
            }
        }
        problems
    }

    /// The expected Errors of `registry`, keyed with the corpus path of
    /// their file.
    fn expected_errors(registry: &Registry) -> Vec<Key> {
        registry
            .known_bad
            .iter()
            .flat_map(|entry| {
                let source = DefFile {
                    path: entry.file.clone(),
                    size: 0,
                    sha256: String::new(),
                }
                .corpus_path();
                entry.error.iter().map(move |e| {
                    (
                        source.clone(),
                        e.code.clone(),
                        e.line,
                        e.column,
                        e.layer.clone(),
                    )
                })
            })
            .collect()
    }

    fn key(d: &Diagnostic) -> Key {
        let p = d.provenance();
        let (line, col) = match p.span() {
            Span::LineCol { line, col } => (line, col),
            Span::ByteOffset(_) => (0, 0),
        };
        (
            p.source().to_owned(),
            d.code().to_string(),
            line,
            col,
            p.layer().map(str::to_owned),
        )
    }

    /// The Errors `found` that are not `expected` (new), and the `expected`
    /// that were not found (gone), each as a multiset.
    fn compare(expected: &[Key], found: &[(Key, String)]) -> (Vec<String>, Vec<Key>) {
        let mut left: BTreeMap<&Key, usize> = BTreeMap::new();
        for k in expected {
            *left.entry(k).or_insert(0) += 1;
        }
        let mut new = Vec::new();
        for (k, shown) in found {
            match left.get_mut(k) {
                Some(n) if *n > 0 => *n -= 1,
                _ => new.push(shown.clone()),
            }
        }
        let gone = left
            .into_iter()
            .flat_map(|(k, n)| std::iter::repeat_n(k.clone(), n))
            .collect();
        (new, gone)
    }

    // ----------------------------------------------------------------------
    // The registry, every run.

    #[test]
    fn the_known_bad_registry_is_valid() {
        let registry = load_registry();
        let problems = validate(&registry, &committed_pin());
        assert!(
            problems.is_empty(),
            "{REGISTRY_PATH}:\n  {}",
            problems.join("\n  ")
        );
        assert!(
            registry.out_of_scope("logger").is_some() && registry.out_of_scope("dyno").is_some(),
            "the pinned logger and dyno definitions must be accounted for"
        );
    }

    /// A registry text with one known-bad entry, `{file}`, `{sha}` and
    /// `{code}` substituted, and an extra line `{extra}` in the entry.
    fn sample(file: &str, sha: &str, code: &str, extra: &str) -> String {
        format!(
            "schema = 1\n\
             [[known_bad]]\nfile = \"{file}\"\nsha256 = \"{sha}\"\nreason = \"r\"\nlegacy = \"l\"\n{extra}\n\
             [[known_bad.error]]\ncode = \"{code}\"\nline = 1\ncolumn = 1\n\
             [[out_of_scope]]\nroot = \"logger\"\nreader = \"x\"\nreason = \"y\"\n"
        )
    }

    #[test]
    fn the_validator_rejects_what_it_should() {
        let pin = committed_pin();
        let pinned = pin
            .files
            .iter()
            .find(|f| f.is_xml())
            .expect("a pinned XML file");
        let dtd = pin
            .files
            .iter()
            .find(|f| !f.is_xml())
            .expect("a pinned DTD");
        let (file, sha) = (pinned.path.as_str(), pinned.sha256.as_str());
        let ok = sample(file, sha, "DEF0024", "");
        let ok = parse_registry("ok.toml", &ok).expect("the sample parses");
        assert_eq!(validate(&ok, &pin), Vec::<String>::new());

        let bad_sha = "0".repeat(64);
        let cases: [(&str, String, &str); 7] = [
            (
                "unpinned file",
                sample("ecu/none.xml", sha, "DEF0024", ""),
                "not a [[defs.files]]",
            ),
            (
                "a DTD",
                sample(&dtd.path, &dtd.sha256, "DEF0024", ""),
                "not an XML file",
            ),
            (
                "changed file",
                sample(file, &bad_sha, "DEF0024", ""),
                "is not the pinned",
            ),
            (
                "warning code",
                sample(file, sha, "DEF0026", ""),
                "only Errors are listed",
            ),
            (
                "unknown code",
                sample(file, sha, "DEF9999", ""),
                "not a registered",
            ),
            (
                "empty reason",
                sample(file, sha, "DEF0024", "").replace("reason = \"r\"", "reason = \" \""),
                "empty reason",
            ),
            (
                "ECU root out of scope",
                sample(file, sha, "DEF0024", "").replace("root = \"logger\"", "root = \"ROMS\""),
                "not a root this reader skips",
            ),
        ];
        for (what, text, want) in cases {
            let registry = parse_registry("bad.toml", &text).expect("the sample parses");
            let problems = validate(&registry, &pin);
            assert!(
                problems.iter().any(|p| p.contains(want)),
                "{what}: want a problem with {want:?}, got {problems:?}"
            );
        }

        // Duplicates: the same file twice, one Error twice, one root twice.
        let entry = format!(
            "[[known_bad]]\nfile = \"{file}\"\nsha256 = \"{sha}\"\nreason = \"r\"\nlegacy = \"l\"\n\
             [[known_bad.error]]\ncode = \"DEF0024\"\nline = 1\ncolumn = 1\n"
        );
        let error = "[[known_bad.error]]\ncode = \"DEF0024\"\nline = 1\ncolumn = 1\n";
        let root = "[[out_of_scope]]\nroot = \"dyno\"\nreader = \"x\"\nreason = \"y\"\n";
        let twice = format!("schema = 1\n{entry}{entry}{error}{root}{root}");
        let registry = parse_registry("twice.toml", &twice).expect("the sample parses");
        let problems = validate(&registry, &pin);
        for want in [
            format!("{file}: listed twice"),
            format!("{file}:1:1 DEF0024: listed twice"),
            "\"dyno\": listed twice".to_owned(),
        ] {
            assert!(
                problems.iter().any(|p| p.ends_with(&want)),
                "want {want:?}: {problems:?}"
            );
        }

        // Unknown keys, anywhere, and a missing schema, do not parse.
        for (what, text) in [
            (
                "top level",
                format!(
                    "typo = 1\n{ok_text}",
                    ok_text = sample(file, sha, "DEF0024", "")
                ),
            ),
            ("entry", sample(file, sha, "DEF0024", "fixed = true")),
            (
                "error",
                sample(file, sha, "DEF0024", "").replace("line = 1", "line = 1\ncol = 1"),
            ),
            (
                "missing schema",
                sample(file, sha, "DEF0024", "").replace("schema = 1\n", ""),
            ),
        ] {
            assert!(parse_registry("bad.toml", &text).is_err(), "{what}: parsed");
        }
    }

    #[test]
    fn a_known_bad_error_no_longer_reported_fails_and_a_new_one_fails() {
        let k = |line| {
            (
                "defs/romraider/ecu/a.xml".to_owned(),
                "DEF0024".to_owned(),
                line,
                5,
                None,
            )
        };
        let expected = [k(1), k(2)];
        // Exactly the expected: nothing new, nothing gone.
        let found = [(k(2), "2".to_owned()), (k(1), "1".to_owned())];
        assert_eq!(compare(&expected, &found), (vec![], vec![]));
        // One fixed: it is gone.
        assert_eq!(compare(&expected, &found[..1]), (vec![], vec![k(1)]));
        // One more, and one reported twice: both new.
        let more = [
            found[0].clone(),
            found[1].clone(),
            (k(3), "3".to_owned()),
            (k(1), "1 again".to_owned()),
        ];
        assert_eq!(
            compare(&expected, &more),
            (vec!["3".to_owned(), "1 again".to_owned()], vec![])
        );
        // The registry's own Errors are keyed with the corpus path.
        let registry = load_registry();
        assert!(
            expected_errors(&registry)
                .iter()
                .all(|(s, ..)| s.starts_with("defs/romraider/"))
        );
    }

    // ----------------------------------------------------------------------
    // The corpus.

    /// What reading the ECU definitions found, before resolving.
    #[derive(Debug, Default)]
    struct ReadCounts {
        roms: usize,
        bases: usize,
        by_bytes: usize,
        forced: usize,
        tables: usize,
        kinds: BTreeMap<&'static str, usize>,
        omitted: usize,
        axes: usize,
        static_axes: usize,
        states: usize,
        bits: usize,
    }

    impl ReadCounts {
        fn add(&mut self, file: &DefinitionFile) {
            for rom in file.roms() {
                self.roms += 1;
                self.bases += usize::from(rom.base().is_some());
                match rom.identification() {
                    Some(Identification::Bytes(_)) => self.by_bytes += 1,
                    Some(Identification::Force) => self.forced += 1,
                    None => {}
                }
                for (_, state) in rom.doc().tables.entries() {
                    match state.get() {
                        Some(table) => self.table(table),
                        None => self.omitted += 1,
                    }
                }
            }
        }

        fn table(&mut self, table: &TableDoc) {
            self.tables += 1;
            let kind = table.kind.get().map_or("(untyped change)", |k| k.name());
            *self.kinds.entry(kind).or_insert(0) += 1;
            for axis in [&table.x_axis, &table.y_axis] {
                if let Some(axis) = axis.get() {
                    self.axes += 1;
                    self.static_axes += usize::from(axis.cells.static_values.get().is_some());
                }
            }
            self.states += table.states.len();
            self.bits += table.bits.items().len();
        }
    }

    /// Every diagnostic, all stages.
    #[derive(Debug, Default)]
    struct Found {
        /// Every Error, keyed, with how it is shown.
        errors: Vec<(Key, String)>,
        /// Warnings and infos by `severity[code]`.
        others: BTreeMap<String, usize>,
    }

    impl Found {
        fn add(&mut self, diagnostics: &[Diagnostic]) {
            for d in diagnostics {
                if d.is_error() {
                    self.errors.push((key(d), d.to_string()));
                } else {
                    *self
                        .others
                        .entry(format!("{}[{}]", d.severity(), d.code()))
                        .or_insert(0) += 1;
                }
            }
        }
    }

    /// One parsed XML file: its root and, for an ECU definition, what the
    /// reader made of it.
    struct Parsed {
        source: String,
        diagnostics: Vec<Diagnostic>,
        /// `None` when the XML is not well-formed.
        root: Option<String>,
        ecu: Option<Option<DefinitionFile>>,
    }

    /// Parses `bytes` and, for an ECU definition, reads it.
    fn parse(source: String, bytes: &[u8]) -> Parsed {
        let loaded = rr_defs::xml::parse(&source, bytes);
        let mut diagnostics = loaded.diagnostics().to_vec();
        let doc: Option<&Document> = loaded.value().and_then(Option::as_ref);
        let root = doc.map(|d| d.root().name().to_owned());
        let ecu = doc
            .filter(|_| root.as_deref().is_some_and(is_ecu_root))
            .map(|doc| {
                let (value, read) = romraider::read_document(&source, doc).accept_errors();
                diagnostics.extend(read);
                value
            });
        Parsed {
            source,
            diagnostics,
            root,
            ecu,
        }
    }

    /// [`parse`] over `inputs` on every available thread, in input order.
    fn parse_all(inputs: Vec<(String, Vec<u8>)>) -> Vec<Parsed> {
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let chunk = inputs.len().div_ceil(threads).max(1);
        let mut inputs = inputs;
        let mut chunks = Vec::new();
        while !inputs.is_empty() {
            let rest = inputs.split_off(chunk.min(inputs.len()));
            chunks.push(std::mem::replace(&mut inputs, rest));
        }
        std::thread::scope(|s| {
            let handles: Vec<_> = chunks
                .into_iter()
                .map(|c| {
                    s.spawn(move || {
                        c.into_iter()
                            .map(|(src, b)| parse(src, &b))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("a parsing thread panicked"))
                .collect()
        })
    }

    /// How the XML files were accounted for.
    #[derive(Debug, Default)]
    struct Accounts {
        ecu: Vec<DefinitionFile>,
        out_of_scope: BTreeMap<String, usize>,
        failures: Vec<String>,
    }

    impl Accounts {
        fn add(&mut self, parsed: Parsed, registry: &Registry, found: &mut Found) {
            found.add(&parsed.diagnostics);
            let source = parsed.source;
            match (parsed.root, parsed.ecu) {
                (None, _) => self.failures.push(format!("{source}: not well-formed")),
                (Some(_), Some(Some(file))) => self.ecu.push(file),
                (Some(_), Some(None)) => self.failures.push(format!("{source}: not read")),
                (Some(root), None) => match registry.out_of_scope(&root) {
                    Some(o) => *self.out_of_scope.entry(o.root.clone()).or_insert(0) += 1,
                    None => self.failures.push(format!(
                        "{source}: root <{root}> is neither an ECU definition nor listed \
                         under [[out_of_scope]] in {REGISTRY_PATH}"
                    )),
                },
            }
        }
    }

    rr_testkit::corpus_test! {
        /// Every pinned definition file loads, accounted for, with no Error
        /// but the known-bad registry's, exactly.
        fn every_pinned_definition_file_loads() -> Result<(), DefsError> {
            let corpus = Corpus::open()?;
            let pin = DefsPin::load(corpus.manifest().path())?;
            let registry = load_registry();
            let problems = validate(&registry, &pin);
            assert!(problems.is_empty(), "{REGISTRY_PATH}:\n  {}", problems.join("\n  "));

            let mut dtds = 0;
            let mut inputs = Vec::new();
            for file in &pin.files {
                let bytes = defs::read_def(corpus.dir(), file)?;
                if file.is_xml() {
                    inputs.push((file.corpus_path(), bytes));
                } else {
                    assert!(
                        file.path.to_ascii_lowercase().ends_with(".dtd"),
                        "{}: pinned file that is neither XML nor a DTD; decide how to check it",
                        file.path
                    );
                    dtds += 1;
                }
            }
            let xml = inputs.len();
            let mut found = Found::default();
            let mut accounts = Accounts::default();
            for parsed in parse_all(inputs) {
                accounts.add(parsed, &registry, &mut found);
            }

            // The logger package, when a copy is present.
            let logger = match defs::read_logger_v370(corpus.dir(), &pin.logger_v370)? {
                Some((_, members)) => {
                    let n = members.len();
                    let inputs = members
                        .into_iter()
                        .map(|m| (format!("{}!{}", pin.logger_v370.file, m.name), m.bytes))
                        .collect();
                    for parsed in parse_all(inputs) {
                        accounts.add(parsed, &registry, &mut found);
                    }
                    format!("{n} member(s) of {} classified", pin.logger_v370.file)
                }
                None => format!(
                    "SKIPPED {} (not present: set {}; never downloaded, D-028)",
                    pin.logger_v370.file,
                    defs::LOGGER_ZIP_ENV
                ),
            };

            let mut read = ReadCounts::default();
            accounts.ecu.iter().for_each(|f| read.add(f));
            let ecu_files = accounts.ecu.len();
            let (set, diagnostics) = DefinitionSet::new(std::mem::take(&mut accounts.ecu)).accept_errors();
            found.add(&diagnostics);
            let mut resolver = set.resolver().with_preferred_scaling("Metric");
            let (mut resolved, mut unresolved, mut tables, mut unplaced, mut axes) = (0, 0, 0, 0, 0);
            let mut depths: BTreeMap<usize, usize> = BTreeMap::new();
            let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
            let refs: Vec<_> = set.roms().map(|(r, _)| r).collect();
            for r in &refs {
                let (rom, diagnostics) = resolver.resolve(*r).accept_errors();
                found.add(&diagnostics);
                let Some(rom) = rom else {
                    unresolved += 1;
                    continue;
                };
                resolved += 1;
                *depths.entry(rom.lineage().names().len()).or_insert(0) += 1;
                tables += rom.tables().len();
                unplaced += rom.unplaced().len();
                for t in rom.tables() {
                    *kinds.entry(t.kind.name()).or_insert(0) += 1;
                    axes += t.axes.len();
                }
            }

            let expected = expected_errors(&registry);
            let (new, gone) = compare(&expected, &found.errors);
            let mut errors_by_code: BTreeMap<&str, usize> = BTreeMap::new();
            for ((_, code, ..), _) in &found.errors {
                *errors_by_code.entry(code).or_insert(0) += 1;
            }
            eprintln!(
                "[defs] at {}: {} pinned file(s): {xml} XML, {dtds} DTD (pin-checked); \
                 {ecu_files} ECU definition(s) read, out of scope by root {:?} ({REGISTRY_PATH}); \
                 logger_v370: {logger}",
                pin.commit,
                pin.files.len(),
                accounts.out_of_scope,
            );
            eprintln!(
                "[defs] read: {} <rom>(s), {} with a base, {} identified by bytes, {} forced; \
                 {} <table>(s) by kind {:?}, {} omitted; {} axes ({} static), {} switch states, \
                 {} bits",
                read.roms, read.bases, read.by_bytes, read.forced, read.tables, read.kinds,
                read.omitted, read.axes, read.static_axes, read.states, read.bits,
            );
            eprintln!(
                "[defs] resolved: {} <rom>(s) in the set ({} repeats within a file not \
                 registered); {resolved} resolved, {unresolved} not; chain lengths {depths:?}; \
                 {tables} table(s) typed by kind {kinds:?} with {axes} axes; {unplaced} inherited \
                 table(s) not on their ROM",
                refs.len(),
                read.roms - refs.len(),
            );
            eprintln!(
                "[defs] diagnostics, all stages: warnings/infos {:?}; errors by code \
                 {errors_by_code:?}; {} known-bad Error(s) in {} file(s) matched",
                found.others,
                expected.len() - gone.len(),
                registry.known_bad.len(),
            );

            assert!(resolved > 0 && tables > 0, "nothing resolved in the pinned corpus");
            assert!(
                accounts.failures.is_empty(),
                "files not accounted for:\n  {}",
                accounts.failures.join("\n  ")
            );
            assert_eq!(unresolved, 0, "every <rom> of the set resolves (with its Errors accepted)");
            assert!(
                new.is_empty() && gone.is_empty(),
                "the Errors differ from {REGISTRY_PATH}: {} found, by code {errors_by_code:?}\n\
                 not known-bad (fix it, or record the file with its reason; the first 200):\n  {}\n\
                 known-bad but no longer reported (remove or update the entry):\n  {}",
                found.errors.len(),
                new.iter().take(200).cloned().collect::<Vec<_>>().join("\n  "),
                gone.iter()
                    .map(|(s, c, l, col, layer)| format!("{s}:{l}:{col} {c} {layer:?}"))
                    .collect::<Vec<_>>()
                    .join("\n  "),
            );
            Ok(())
        }
    }
}
