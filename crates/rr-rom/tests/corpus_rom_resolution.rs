//! Every non-suspect stock ROM of the external corpus that has a definition
//! resolves to exactly one definition, and the results are committed as a
//! coverage fixture (US-RR-34, acceptance criterion 1; task US-RR-34-1;
//! EC-03, D-025, D-028).
//!
//! # Which definition set
//!
//! Each pinned ECU-definition file is a definition set on its own: the
//! file is read, made a [`DefinitionSet`], indexed ([`RomIdIndex`]) and
//! every non-suspect image is looked up in it. That is the unit legacy
//! RomRaider worked in: the user lists definition files and legacy reads,
//! matches and resolves `base` within one file at a time, taking the first
//! file that matches (`editor/ecu/OpenImageWorker.java:219-250`,
//! `xml/DOMRomUnmarshaller.java:97-152`; the default list is one
//! `ecu_defs.xml`, `xml/DOMSettingsUnmarshaller.java:110`). The pinned
//! tree is not one set a user would load: it holds alternative copies of
//! the same ROMs (`ecu/standard/ecu_defs.xml`, `ecu/metric/ecu_defs.xml`
//! with metric units, `ecu/standard/ecu_defs_merpmod.xml` and a MerpMod
//! testing file, plus per-ROM files that repeat a ROM of `ecu_defs.xml`).
//! Loaded together, an image two copies define is ambiguous and refused,
//! which is the index doing its job (EC-03, US-RR-34-2), not a corpus
//! property worth pinning. So "resolves to exactly one definition" is
//! checked in every file: in no file may two `<rom>`s match one image, and
//! the fixture lists, per image, every file that defines it with the one
//! `xmlid` it resolves to, the standard and the metric `ecu_defs.xml` among
//! them.
//!
//! "Has a definition" is `fixtures/corpus-coverage.toml`'s (US-RR-16-6): an
//! image is `matched` there exactly when it is `unique` here, through the
//! same files and `xmlid`s, a definition whose `base` legacy could not find
//! in its file (`base_error` there) being one resolved with its base from
//! another file here (`base_from`). [`the_resolution_fixture_is_valid`]
//! checks the two committed files agree, with no corpus. That report
//! matches with a small standalone reader in xtask following legacy; this
//! test matches with rr-defs' reader and index, so each checks the other.
//! Where rr-defs deliberately differs from legacy the difference is listed,
//! with why, in [`KNOWN_DIFFERENCES`] (one at the pin: a file whose root
//! element is its one `<rom>`, which legacy never matched).
//!
//! # What is checked, with the corpus
//!
//! [`every_defined_stock_rom_resolves_uniquely`] reads every pinned file
//! (size and SHA-256 checked) and every manifest image; a suspect image
//! (`[roms.files.suspect]`, US-RR-11-7) is counted and skipped. For every
//! (file, non-suspect image):
//!
//! * the lookup is never [`RomMatch::Ambiguous`];
//! * a [`RomMatch::Unique`] definition resolves ([`DefinitionResolver`])
//!   with no Error. One whose only Errors are an unknown `base` (a per-ROM
//!   file naming `32BITBASE` without carrying a copy) is resolved again
//!   with [`BASE_PROVIDER`] loaded after its file, as a user would load it
//!   (MF-17), and must then resolve with no Error; the fixture records
//!   `base_from`;
//! * the image is opened through the definition ([`OpenRom::open`]) and
//!   its size check recorded: `ok`, `undeclared` (no `<filesize>`: nothing
//!   to check) or `mismatch` with both sizes (the image opens read-only,
//!   US-RR-34-8). A mismatch is recorded, not a failure: at the pin every
//!   160 KiB 16-bit image is defined with a `192kb` `<filesize>`, which
//!   legacy bridged with its RAM offset (`xml/DOMRomUnmarshaller.java:72-75`,
//!   `Settings.SIXTEENBIT_SMALL_SIZE`).
//!
//! The results, rendered as `fixtures/corpus-rom-resolution.toml`, must be
//! byte-identical to the committed file, so a corpus, definition or reader
//! change that moves any image fails here until the fixture is regenerated
//! and the change reviewed:
//!
//! ```sh
//! RR_CORPUS_DIR=~/.cache/romraider-rs/corpus RR_UPDATE_ROM_RESOLUTION=1 \
//!     cargo nextest run -p rr-rom --test corpus_rom_resolution
//! ```
//!
//! Corpus-tier (`*::requires_RR_CORPUS_DIR`): skipped without
//! `RR_CORPUS_DIR`, run by CI's `corpus` job.
//!
//! # The committed fixture, every run
//!
//! [`the_resolution_fixture_is_valid`] needs no corpus: the fixture parses
//! with no unknown key, renders back to itself (it is generated, never
//! edited), is pinned to the manifest's commits, lists every non-suspect
//! manifest image once in manifest order and no suspect one, names only
//! pinned XML files, has a summary that adds up and no ambiguous image, and
//! agrees with `fixtures/corpus-coverage.toml` image by image.
//!
//! Metadata only (D-028): image paths, definition file paths, `xmlid`s and
//! sizes; no ROM bytes and no definition content.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod resolution {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::sync::Arc;

    use rr_core::diag::{Diagnostic, codes};
    use rr_defs::resolve::{Definition, DefinitionResolver, RomIdIndex, RomMatch};
    use rr_defs::romraider::{self, DefinitionFile, DefinitionSet, RomRef};
    use rr_rom::image::Image as RomImage;
    use rr_rom::open::{EditingDisabled, OpenRom};
    use rr_testkit::corpus::defs::{self, DefFile, DefsError, DefsPin};
    use rr_testkit::corpus::{Corpus, Entry, Image, MANIFEST_PATH, Manifest};
    use rr_testkit::fixtures::{FixtureError, parse_kat, repo_root};
    use serde::Deserialize;

    /// The fixture, relative to the repository root.
    const FIXTURE_PATH: &str = "fixtures/corpus-rom-resolution.toml";

    /// The ROM-ID coverage report it must agree with.
    const COVERAGE_PATH: &str = "fixtures/corpus-coverage.toml";

    /// Set (to anything) to write a fresh fixture instead of failing on a
    /// stale one.
    const UPDATE_ENV: &str = "RR_UPDATE_ROM_RESOLUTION";

    /// The file a definition whose `base` is not in its own file is resolved
    /// with (MF-17): RomRaider's main definition file, the one the legacy
    /// release notes point users at.
    const BASE_PROVIDER: &str = "ecu/standard/ecu_defs.xml";

    /// (image, file) definitions this fixture has and the coverage report
    /// does not, each with why. Every other image must agree exactly, and
    /// every entry here must still be a difference.
    const KNOWN_DIFFERENCES: &[(&str, &str, &str)] = &[(
        "USDM/Impreza/AE5L500V-2013-USDM-Subaru-Impreza-STI-MT.hex",
        "ecu/AE5L500V_RR.xml",
        "the file's root element is its one <rom>, not <roms>. Legacy looked for <rom> \
         children of the root only (xml/DOMRomUnmarshaller.java findRomNodeMatch), so it \
         never matched this file, and the coverage report follows legacy; rr-defs reads it \
         as that one ROM (DEF0003 SINGLE_ROM_ROOT). Its base 32BITBASE is not in the file, \
         so it resolves only with ecu/standard/ecu_defs.xml loaded (MF-17).",
    )];

    const HEADER: &str = "\
# Definition resolution of the stock-ROM corpus (US-RR-34-1, EC-03, D-025, D-028).
# GENERATED: do not edit.
#   regenerate: RR_CORPUS_DIR=<dir> RR_UPDATE_ROM_RESOLUTION=1 \\
#                 cargo nextest run -p rr-rom --test corpus_rom_resolution
#   check:      the corpus-tier test of crates/rr-rom/tests/corpus_rom_resolution.rs
#               (CI `corpus` job); its no-corpus test checks this file's shape and
#               that it agrees with fixtures/corpus-coverage.toml.
# Definition set: each pinned ECU-definition file on its own, as legacy RomRaider
# read, matched and inherited within one file at a time. The pinned tree holds
# alternative copies of the same ROMs (ecu/standard and ecu/metric ecu_defs.xml,
# the MerpMod variants, per-ROM files), which loaded together are ambiguous by
# design (EC-03), so they are never one set here.
# For every non-suspect [roms] image of fixtures/corpus-manifest.toml, in
# manifest order: every file whose ROM-ID index (rr_defs::resolve::RomIdIndex)
# matches the image, each with the one <rom> xmlid it resolves to (with no
# Error; base_from: its base is not in its own file and was found in that
# one, MF-17) and the image's size check against it (rr_rom::open::OpenRom:
# ok, undeclared, or mismatch with the declared and image sizes: the image
# opens read-only); status \"unique\" (no file matches it twice) or
# \"no-definition\". Agrees with fixtures/corpus-coverage.toml image by image,
# but for the differences listed in the test's KNOWN_DIFFERENCES.
# Metadata only (D-028): paths, xmlids and sizes; no ROM bytes and no
# definition content. Format: crates/rr-rom/tests/corpus_rom_resolution.rs.
";

    // ----------------------------------------------------------------------
    // The fixture model.

    /// The fixture (every key but `schema`).
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Resolution {
        /// The manifest's `[roms] commit`.
        roms_commit: String,
        /// The manifest's `[defs] commit`.
        defs_commit: String,
        summary: Summary,
        #[serde(default)]
        roms: Vec<Rom>,
    }

    /// Totals.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Summary {
        /// Manifest images.
        images: usize,
        /// Suspect images, not looked up.
        suspect: usize,
        /// Non-suspect images: the `[[roms]]` entries.
        non_suspect: usize,
        /// Non-suspect images with a definition, each unique in every file.
        unique: usize,
        /// Non-suspect images no file defines.
        no_definition: usize,
        /// Non-suspect images some file matches more than once: always 0
        /// (the test fails first).
        ambiguous: usize,
        /// Pinned files read as ECU definitions, each one set.
        definition_files: usize,
        /// (image, file) matches: the `definitions` entries.
        matches: usize,
        /// Matches whose size check passed.
        size_ok: usize,
        /// Matches whose definition declares no size.
        size_undeclared: usize,
        /// Matches whose image is not the declared size.
        size_mismatch: usize,
    }

    /// One non-suspect image.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Rom {
        /// The manifest path.
        path: String,
        status: Status,
        /// Every file defining the image, by path.
        #[serde(default)]
        definitions: Vec<Match>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum Status {
        /// At least one file defines the image, none twice.
        Unique,
        /// No file defines it.
        NoDefinition,
    }

    impl Status {
        const fn name(self) -> &'static str {
            match self {
                Self::Unique => "unique",
                Self::NoDefinition => "no-definition",
            }
        }
    }

    /// One file's definition of an image.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Match {
        /// The pinned path (`[[defs.files]] path`).
        file: String,
        /// The matching `<rom>`'s own `xmlid`.
        xmlid: String,
        /// The file its `base` was found in, when not its own
        /// ([`BASE_PROVIDER`]).
        #[serde(default)]
        base_from: Option<String>,
        size: SizeCheck,
        /// The definition's `<filesize>`, for a mismatch.
        #[serde(default)]
        declared: Option<u64>,
        /// The image's size, for a mismatch.
        #[serde(default)]
        image: Option<u64>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum SizeCheck {
        Ok,
        Undeclared,
        Mismatch,
    }

    impl SizeCheck {
        const fn name(self) -> &'static str {
            match self {
                Self::Ok => "ok",
                Self::Undeclared => "undeclared",
                Self::Mismatch => "mismatch",
            }
        }
    }

    /// Boxed: `FixtureError` holds paths and is over clippy's
    /// `result_large_err` threshold on Windows, where a `PathBuf` is larger.
    fn parse_fixture(
        path: impl Into<PathBuf>,
        text: &str,
    ) -> Result<Resolution, Box<FixtureError>> {
        parse_kat::<Resolution>(path, text)
            .map(|f| f.data)
            .map_err(Box::new)
    }

    /// A TOML basic string.
    fn quoted(s: &str) -> String {
        toml::Value::String(s.to_owned()).to_string()
    }

    /// The fixture text of `r`: deterministic, `\n` endings.
    fn render(r: &Resolution) -> String {
        let mut out = String::from(HEADER);
        let s = &r.summary;
        // Writing to a `String` cannot fail.
        let _ = write!(
            out,
            "\nschema = 1\nroms_commit = {}\ndefs_commit = {}\n\n[summary]\n\
             images = {}\nsuspect = {}\nnon_suspect = {}\nunique = {}\nno_definition = {}\n\
             ambiguous = {}\ndefinition_files = {}\nmatches = {}\nsize_ok = {}\n\
             size_undeclared = {}\nsize_mismatch = {}\n",
            quoted(&r.roms_commit),
            quoted(&r.defs_commit),
            s.images,
            s.suspect,
            s.non_suspect,
            s.unique,
            s.no_definition,
            s.ambiguous,
            s.definition_files,
            s.matches,
            s.size_ok,
            s.size_undeclared,
            s.size_mismatch,
        );
        for rom in &r.roms {
            let _ = write!(
                out,
                "\n[[roms]]\npath = {}\nstatus = {}\n",
                quoted(&rom.path),
                quoted(rom.status.name())
            );
            if rom.definitions.is_empty() {
                continue;
            }
            out.push_str("definitions = [\n");
            for m in &rom.definitions {
                let _ = write!(
                    out,
                    "  {{ file = {}, xmlid = {}",
                    quoted(&m.file),
                    quoted(&m.xmlid)
                );
                if let Some(b) = &m.base_from {
                    let _ = write!(out, ", base_from = {}", quoted(b));
                }
                let _ = write!(out, ", size = {}", quoted(m.size.name()));
                if let Some(d) = m.declared {
                    let _ = write!(out, ", declared = {d}");
                }
                if let Some(i) = m.image {
                    let _ = write!(out, ", image = {i}");
                }
                out.push_str(" },\n");
            }
            out.push_str("]\n");
        }
        out
    }

    /// The summary `roms` adds up to, with the counts the entries do not
    /// hold (`images`, `suspect`, `definition_files`) taken from `given`.
    fn summarise(roms: &[Rom], given: &Summary) -> Summary {
        let mut s = Summary {
            images: given.images,
            suspect: given.suspect,
            definition_files: given.definition_files,
            non_suspect: roms.len(),
            ..Summary::default()
        };
        for rom in roms {
            match rom.status {
                Status::Unique => s.unique += 1,
                Status::NoDefinition => s.no_definition += 1,
            }
            for m in &rom.definitions {
                s.matches += 1;
                match m.size {
                    SizeCheck::Ok => s.size_ok += 1,
                    SizeCheck::Undeclared => s.size_undeclared += 1,
                    SizeCheck::Mismatch => s.size_mismatch += 1,
                }
            }
        }
        s
    }

    // ----------------------------------------------------------------------
    // The coverage report, as far as this needs it.

    #[derive(Debug, Deserialize)]
    struct Coverage {
        roms: Vec<CoverageRom>,
    }

    #[derive(Debug, Deserialize)]
    struct CoverageRom {
        path: String,
        status: String,
        #[serde(default)]
        definitions: Vec<CoverageDef>,
    }

    #[derive(Debug, Deserialize)]
    struct CoverageDef {
        file: String,
        xmlid: String,
        /// Why legacy could not resolve the definition's `base`.
        #[serde(default)]
        base_error: Option<String>,
    }

    fn load_coverage() -> Coverage {
        let text = std::fs::read_to_string(repo_root().join(COVERAGE_PATH))
            .expect("read the coverage report");
        toml::from_str(&text).expect("the coverage report parses")
    }

    // ----------------------------------------------------------------------
    // Validation, every run.

    /// Every problem with `r` against the manifest `manifest`, its pinned
    /// definitions `pin` and the coverage report `coverage`.
    fn validate(
        r: &Resolution,
        manifest: &Manifest,
        pin: &DefsPin,
        coverage: &Coverage,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        if r.roms_commit != manifest.commit() {
            problems.push(format!(
                "roms_commit {} is not the manifest's {}",
                r.roms_commit,
                manifest.commit()
            ));
        }
        if r.defs_commit != pin.commit {
            problems.push(format!(
                "defs_commit {} is not the manifest's {}",
                r.defs_commit, pin.commit
            ));
        }
        let clean: Vec<&str> = manifest.clean().map(|f| f.path.as_str()).collect();
        let listed: Vec<&str> = r.roms.iter().map(|rom| rom.path.as_str()).collect();
        if listed != clean {
            let want: BTreeSet<_> = clean.iter().collect();
            let got: BTreeSet<_> = listed.iter().collect();
            problems.push(format!(
                "[[roms]] is not every non-suspect manifest image once, in manifest order: \
                 missing {:?}, not non-suspect images {:?}",
                want.difference(&got).collect::<Vec<_>>(),
                got.difference(&want).collect::<Vec<_>>(),
            ));
        }
        let want = summarise(&r.roms, &r.summary);
        if r.summary != want {
            problems.push(format!(
                "[summary] does not add up: {:?}, the entries give {want:?}",
                r.summary
            ));
        }
        if r.summary.images != manifest.len()
            || r.summary.suspect != manifest.suspect().count()
            || r.summary.images != r.summary.suspect + r.summary.non_suspect
        {
            problems.push(format!(
                "[summary] images {} / suspect {} are not the manifest's {} / {}",
                r.summary.images,
                r.summary.suspect,
                manifest.len(),
                manifest.suspect().count()
            ));
        }
        if r.summary.ambiguous != 0 {
            problems.push("[summary] ambiguous is not 0".to_owned());
        }
        let xml: BTreeSet<&str> = pin
            .files
            .iter()
            .filter(|f| f.is_xml())
            .map(|f| f.path.as_str())
            .collect();
        let covered: BTreeMap<&str, &CoverageRom> =
            coverage.roms.iter().map(|c| (c.path.as_str(), c)).collect();
        let mut known_seen: BTreeSet<(&str, &str)> = BTreeSet::new();
        for rom in &r.roms {
            let at = &rom.path;
            let files: Vec<&str> = rom.definitions.iter().map(|m| m.file.as_str()).collect();
            if !files.is_sorted_by(|a, b| a < b) {
                problems.push(format!(
                    "{at}: definitions not sorted by file, or a file twice"
                ));
            }
            match (rom.status, rom.definitions.is_empty()) {
                (Status::Unique, true) => problems.push(format!("{at}: unique with no definition")),
                (Status::NoDefinition, false) => {
                    problems.push(format!("{at}: no-definition with definitions"));
                }
                _ => {}
            }
            for m in &rom.definitions {
                if !xml.contains(m.file.as_str()) {
                    problems.push(format!("{at}: {} is not a pinned XML file", m.file));
                }
                if let Some(b) = &m.base_from
                    && (b == &m.file || !xml.contains(b.as_str()))
                {
                    problems.push(format!(
                        "{at}: {}: base_from {b} is not another pinned XML file",
                        m.file
                    ));
                }
                let sizes = (m.declared.is_some(), m.image.is_some());
                if (m.size == SizeCheck::Mismatch) != (sizes == (true, true))
                    || (m.size != SizeCheck::Mismatch && sizes != (false, false))
                    || m.declared.is_some() && m.declared == m.image
                {
                    problems.push(format!(
                        "{at}: {}: declared and image are given, and differ, exactly for a mismatch",
                        m.file
                    ));
                }
            }
            // Agreement with the coverage report: the same (file, xmlid)
            // pairs, a base found elsewhere exactly where legacy found none,
            // but for the known differences.
            let Some(c) = covered.get(at.as_str()) else {
                problems.push(format!("{at}: not in {COVERAGE_PATH}"));
                continue;
            };
            let theirs: BTreeSet<(&str, &str, bool)> = c
                .definitions
                .iter()
                .map(|d| (d.file.as_str(), d.xmlid.as_str(), d.base_error.is_some()))
                .collect();
            let mut ours: BTreeSet<(&str, &str, bool)> = BTreeSet::new();
            for m in &rom.definitions {
                let known = KNOWN_DIFFERENCES
                    .iter()
                    .any(|(image, file, _)| image == at && *file == m.file);
                if known {
                    if c.definitions.iter().any(|d| d.file == m.file) {
                        problems.push(format!(
                            "{at}: {}: a known difference that {COVERAGE_PATH} no longer has; \
                             remove it from KNOWN_DIFFERENCES",
                            m.file
                        ));
                    }
                    known_seen.insert((at.as_str(), m.file.as_str()));
                } else {
                    ours.insert((m.file.as_str(), m.xmlid.as_str(), m.base_from.is_some()));
                }
            }
            let matched = c.status == "matched";
            if matched == ours.is_empty() || theirs != ours {
                problems.push(format!(
                    "{at}: {} {theirs:?} in {COVERAGE_PATH}, {} {ours:?} here (file, xmlid, \
                     base elsewhere)",
                    c.status,
                    rom.status.name()
                ));
            }
        }
        for (image, file, _) in KNOWN_DIFFERENCES {
            if !known_seen.contains(&(*image, *file)) {
                problems.push(format!(
                    "{image}: {file}: a known difference not in the fixture; remove it from \
                     KNOWN_DIFFERENCES"
                ));
            }
        }
        problems
    }

    fn committed_manifest() -> Manifest {
        Manifest::load(repo_root().join(MANIFEST_PATH)).expect("load the manifest")
    }

    fn committed_pin() -> DefsPin {
        DefsPin::load(&repo_root().join(MANIFEST_PATH)).expect("load [defs] of the manifest")
    }

    fn committed_text() -> String {
        std::fs::read_to_string(repo_root().join(FIXTURE_PATH)).expect("read the fixture")
    }

    #[test]
    fn the_resolution_fixture_is_valid() {
        let text = committed_text();
        let r = parse_fixture(FIXTURE_PATH, &text).expect("the fixture parses");
        assert!(
            render(&r) == text,
            "{FIXTURE_PATH} is not in its generated form: regenerate it, never edit it"
        );
        let problems = validate(
            &r,
            &committed_manifest(),
            &committed_pin(),
            &load_coverage(),
        );
        assert!(
            problems.is_empty(),
            "{FIXTURE_PATH}:\n  {}",
            problems.join("\n  ")
        );
        assert!(r.summary.unique > 0, "no image resolves");
    }

    #[test]
    fn the_validator_rejects_what_it_should() {
        let manifest = committed_manifest();
        let pin = committed_pin();
        let coverage = load_coverage();
        let good = parse_fixture(FIXTURE_PATH, &committed_text()).expect("the fixture parses");
        assert_eq!(
            validate(&good, &manifest, &pin, &coverage),
            Vec::<String>::new()
        );
        let first_unique = good
            .roms
            .iter()
            .position(|r| r.status == Status::Unique)
            .expect("a unique image");
        let first_none = good
            .roms
            .iter()
            .position(|r| r.status == Status::NoDefinition)
            .expect("an image with no definition");

        type Edit = fn(&mut Resolution, usize, usize);
        let cases: [(&str, Edit, &str); 9] = [
            (
                "another commit",
                |r, _, _| r.defs_commit.push('0'),
                "defs_commit",
            ),
            (
                "an image dropped",
                |r, u, _| {
                    r.roms.remove(u);
                },
                "every non-suspect",
            ),
            (
                "two images swapped",
                |r, u, n| r.roms.swap(u, n),
                "manifest order",
            ),
            (
                "a summary off",
                |r, _, _| r.summary.unique += 1,
                "does not add up",
            ),
            (
                "an unpinned file",
                |r, u, _| r.roms[u].definitions[0].file = "ecu/none.xml".into(),
                "not a pinned XML",
            ),
            (
                "another xmlid",
                |r, u, _| r.roms[u].definitions[0].xmlid.push('X'),
                "here",
            ),
            (
                "a definition the report lacks",
                |r, u, n| {
                    let m = r.roms[u].definitions[0].clone();
                    r.roms[n].definitions.push(m);
                    r.roms[n].status = Status::Unique;
                },
                "here",
            ),
            (
                "a mismatch without sizes",
                |r, u, _| r.roms[u].definitions[0].size = SizeCheck::Mismatch,
                "exactly for a mismatch",
            ),
            (
                "unique with none",
                |r, _, n| r.roms[n].status = Status::Unique,
                "unique with no definition",
            ),
        ];
        for (what, edit, want) in cases {
            let mut r = good.clone();
            edit(&mut r, first_unique, first_none);
            let problems = validate(&r, &manifest, &pin, &coverage);
            assert!(
                problems.iter().any(|p| p.contains(want)),
                "{what}: want a problem with {want:?}, got {problems:?}"
            );
        }

        // Unknown keys, anywhere, and a missing schema, do not parse.
        let text = committed_text();
        for (what, bad) in [
            (
                "top level",
                text.replacen("schema = 1\n", "schema = 1\ntypo = 1\n", 1),
            ),
            (
                "summary",
                text.replacen("[summary]\n", "[summary]\ntypo = 1\n", 1),
            ),
            ("rom", text.replacen("status = ", "typo = 1\nstatus = ", 1)),
            (
                "definition",
                text.replacen("size = \"", "typo = 1, size = \"", 1),
            ),
            (
                "status",
                text.replacen("status = \"unique\"", "status = \"ambiguous\"", 1),
            ),
            ("missing schema", text.replacen("schema = 1\n", "", 1)),
        ] {
            assert_ne!(bad, text, "{what}: the edit applied");
            assert!(parse_fixture("bad.toml", &bad).is_err(), "{what}: parsed");
        }
    }

    // ----------------------------------------------------------------------
    // The corpus.

    /// What one (file, image) lookup gave.
    enum Outcome {
        /// The one `<rom>` of the file the image matches, resolved and the
        /// image opened through it.
        Unique(Match),
        /// More than one `<rom>` matched: their `xmlid`s.
        Ambiguous(Vec<String>),
        /// One `<rom>` matched but did not resolve, or resolved with an
        /// Error: why.
        Failed(String),
    }

    /// Every image `file` defines, by image number.
    struct FileResult {
        file: String,
        ecu: bool,
        outcomes: Vec<(usize, Outcome)>,
    }

    /// A resolved definition, or why not.
    type Resolved = Result<(Definition, Option<String>), String>;

    /// The Errors of `diagnostics`, shown.
    fn errors(diagnostics: &[Diagnostic]) -> Vec<String> {
        diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(ToString::to_string)
            .collect()
    }

    /// Resolves `rom` of the one-file `set`. A `<rom>` whose only Errors
    /// are an unknown base is resolved again in a set of its file and
    /// `base` ([`BASE_PROVIDER`]), after it (MF-17: a base may be in
    /// another loaded file), and says so.
    fn resolve_rom(
        set: &DefinitionSet,
        resolver: &mut DefinitionResolver<'_>,
        rom: RomRef,
        base: &(String, Arc<DefinitionFile>),
    ) -> Resolved {
        let (definition, diagnostics) = resolver.resolve(rom).accept_errors();
        let shown = errors(&diagnostics);
        if let Some(d) = definition.filter(|_| shown.is_empty()) {
            return Ok((d, None));
        }
        let unknown_base = diagnostics
            .iter()
            .filter(|d| d.is_error())
            .all(|d| d.code() == codes::UNKNOWN_BASE);
        let file = &set.files()[rom.file()];
        if !unknown_base || Arc::ptr_eq(file, &base.1) {
            return Err(format!("does not resolve: {}", shown.join("; ")));
        }
        let (with_base, _) =
            DefinitionSet::new([Arc::clone(file), Arc::clone(&base.1)]).accept_errors();
        let again = with_base
            .roms()
            .find(|(r, _)| r.file() == 0 && r.index() == rom.index())
            .map(|(r, _)| r)
            .expect("the <rom> is in the set with its base");
        let (definition, diagnostics) = DefinitionResolver::new(&with_base)
            .resolve(again)
            .accept_errors();
        let again_shown = errors(&diagnostics);
        match definition {
            Some(d) if again_shown.is_empty() => Ok((d, Some(base.0.clone()))),
            _ => Err(format!(
                "does not resolve, alone ({}) or with {} ({})",
                shown.join("; "),
                base.0,
                again_shown.join("; ")
            )),
        }
    }

    /// The size check of opening `image` (`path`) through `definition`.
    fn size_check(
        definition: &Definition,
        image: &[u8],
        path: &str,
    ) -> (SizeCheck, Option<u64>, Option<u64>) {
        let (rom, _) =
            OpenRom::open(definition.clone(), RomImage::new(image.to_vec()), path).accept_errors();
        match rom.editing_disabled() {
            None if definition.romid().file_size().is_some() => (SizeCheck::Ok, None, None),
            None => (SizeCheck::Undeclared, None, None),
            Some(EditingDisabled::SizeMismatch { declared, actual }) => (
                SizeCheck::Mismatch,
                Some(u64::from(declared)),
                Some(u64::try_from(actual).expect("an image size fits u64")),
            ),
            Some(other) => panic!("{path}: editing disabled for a new reason: {other}"),
        }
    }

    /// Reads the pinned `file`, and looks every image of `images` up in it
    /// as a set on its own.
    fn resolve_file(
        corpus: &Corpus,
        file: &DefFile,
        images: &[(String, Vec<u8>)],
        base: &(String, Arc<DefinitionFile>),
    ) -> Result<FileResult, DefsError> {
        let mut result = FileResult {
            file: file.path.clone(),
            ecu: false,
            outcomes: Vec::new(),
        };
        // Errors in reading belong to the known-bad registry
        // (rr-defs/tests/corpus_romraider_loads.rs); a file that is not an
        // ECU definition (a logger or dyno root) is no set.
        let read = if file.path == base.0 {
            Arc::clone(&base.1)
        } else {
            let bytes = defs::read_def(corpus.dir(), file)?;
            match romraider::read(&file.corpus_path(), &bytes).accept_errors() {
                (Some(read), _) => Arc::new(read),
                (None, _) => return Ok(result),
            }
        };
        result.ecu = true;
        let (set, _) = DefinitionSet::new([read]).accept_errors();
        let index = RomIdIndex::new(&set);
        let mut resolver = DefinitionResolver::new(&set);
        let mut resolved: BTreeMap<RomRef, Resolved> = BTreeMap::new();
        for (n, (path, image)) in images.iter().enumerate() {
            let outcome = match index.lookup(image) {
                RomMatch::None => continue,
                RomMatch::Ambiguous(all) => {
                    Outcome::Ambiguous(all.iter().map(|c| c.xmlid().to_owned()).collect())
                }
                RomMatch::Unique(c) => {
                    let r = resolved
                        .entry(c.rom())
                        .or_insert_with(|| resolve_rom(&set, &mut resolver, c.rom(), base));
                    match r {
                        Err(why) => Outcome::Failed(format!("`{}` {why}", c.xmlid())),
                        Ok((definition, base_from)) => {
                            let (size, declared, actual) = size_check(definition, image, path);
                            Outcome::Unique(Match {
                                file: file.path.clone(),
                                xmlid: c.xmlid().to_owned(),
                                base_from: base_from.clone(),
                                size,
                                declared,
                                image: actual,
                            })
                        }
                    }
                }
            };
            result.outcomes.push((n, outcome));
        }
        Ok(result)
    }

    /// [`resolve_file`] over every pinned XML file, on every available
    /// thread, in pin order.
    fn resolve_all(
        corpus: &Corpus,
        files: &[&DefFile],
        images: &[(String, Vec<u8>)],
        base: &(String, Arc<DefinitionFile>),
    ) -> Result<Vec<FileResult>, DefsError> {
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let chunk = files.len().div_ceil(threads).max(1);
        std::thread::scope(|s| {
            let handles: Vec<_> = files
                .chunks(chunk)
                .map(|c| {
                    s.spawn(move || {
                        c.iter()
                            .map(|f| resolve_file(corpus, f, images, base))
                            .collect::<Result<Vec<_>, _>>()
                    })
                })
                .collect();
            let mut all = Vec::new();
            for h in handles {
                all.extend(h.join().expect("a resolving thread panicked")?);
            }
            Ok(all)
        })
    }

    /// Reads [`BASE_PROVIDER`].
    fn read_base_provider(
        corpus: &Corpus,
        pin: &DefsPin,
    ) -> Result<(String, Arc<DefinitionFile>), DefsError> {
        let file = pin
            .files
            .iter()
            .find(|f| f.path == BASE_PROVIDER)
            .unwrap_or_else(|| panic!("{BASE_PROVIDER} is not pinned"));
        let bytes = defs::read_def(corpus.dir(), file)?;
        let (read, _) = romraider::read(&file.corpus_path(), &bytes).accept_errors();
        let read = read.unwrap_or_else(|| panic!("{BASE_PROVIDER} is not an ECU definition"));
        Ok((file.path.clone(), Arc::new(read)))
    }

    /// The first line where `a` and `b` differ, 1-based.
    fn first_difference(a: &str, b: &str) -> usize {
        a.lines()
            .zip(b.lines())
            .position(|(x, y)| x != y)
            .map_or_else(|| a.lines().count().min(b.lines().count()) + 1, |i| i + 1)
    }

    rr_testkit::corpus_test! {
        /// Every non-suspect image is unique or undefined in every pinned
        /// definition file, every definition resolves with no Error, and the
        /// results are the committed fixture's.
        fn every_defined_stock_rom_resolves_uniquely() -> Result<(), DefsError> {
            let corpus = Corpus::open()?;
            let manifest = corpus.manifest();
            let pin = DefsPin::load(manifest.path())?;
            let mut images = Vec::new();
            let mut suspect = 0;
            for entry in manifest.entries() {
                let file = entry.file();
                match (entry, corpus.read(file)?) {
                    (Entry::Clean(_), Image::Raw(bytes)) => images.push((file.path.clone(), bytes)),
                    (Entry::Suspect { .. }, Image::Suspect { .. }) => suspect += 1,
                    _ => panic!("{}: read as another kind than the manifest says", file.path),
                }
            }
            let xml: Vec<&DefFile> = pin.files.iter().filter(|f| f.is_xml()).collect();
            let base = read_base_provider(&corpus, &pin)?;
            let results = resolve_all(&corpus, &xml, &images, &base)?;

            let mut found: Vec<Vec<Match>> = vec![Vec::new(); images.len()];
            let mut ambiguous: BTreeSet<usize> = BTreeSet::new();
            let mut failures = Vec::new();
            let mut definition_files = 0;
            for r in results {
                definition_files += usize::from(r.ecu);
                for (n, outcome) in r.outcomes {
                    let path = &images[n].0;
                    match outcome {
                        Outcome::Unique(m) => found[n].push(m),
                        Outcome::Ambiguous(xmlids) => {
                            ambiguous.insert(n);
                            failures.push(format!("{path}: ambiguous in {}: {xmlids:?}", r.file));
                        }
                        Outcome::Failed(why) => failures.push(format!("{path}: {}: {why}", r.file)),
                    }
                }
            }
            let roms: Vec<Rom> = images
                .iter()
                .zip(found)
                .map(|((path, _), mut definitions)| {
                    definitions.sort_by(|a, b| a.file.cmp(&b.file));
                    Rom {
                        path: path.clone(),
                        status: if definitions.is_empty() { Status::NoDefinition } else { Status::Unique },
                        definitions,
                    }
                })
                .collect();
            let given = Summary {
                images: manifest.len(),
                suspect,
                definition_files,
                ..Summary::default()
            };
            let mut summary = summarise(&roms, &given);
            summary.ambiguous = ambiguous.len();
            let fresh = Resolution {
                roms_commit: manifest.commit().to_owned(),
                defs_commit: pin.commit.clone(),
                summary,
                roms,
            };
            let s = &fresh.summary;
            eprintln!(
                "[resolution] {} image(s): {} suspect (skipped), {} non-suspect: {} unique, \
                 {} no definition, {} ambiguous; {} ECU definition file(s), {} match(es): \
                 size ok {}, undeclared {}, mismatch {}",
                s.images, s.suspect, s.non_suspect, s.unique, s.no_definition, s.ambiguous,
                s.definition_files, s.matches, s.size_ok, s.size_undeclared, s.size_mismatch,
            );
            assert!(
                failures.is_empty(),
                "{} image/file pair(s) do not resolve to exactly one clean definition:\n  {}",
                failures.len(),
                failures.join("\n  ")
            );
            assert!(s.unique > 0, "nothing resolved");

            let text = render(&fresh);
            let committed = std::fs::read_to_string(repo_root().join(FIXTURE_PATH)).unwrap_or_default();
            if text != committed {
                if std::env::var_os(UPDATE_ENV).is_some() {
                    std::fs::write(repo_root().join(FIXTURE_PATH), &text).expect("write the fixture");
                    eprintln!("[resolution] wrote {FIXTURE_PATH}");
                } else {
                    panic!(
                        "{FIXTURE_PATH} is not what the corpus gives (first difference at line {}); \
                         review the change and regenerate it: RR_CORPUS_DIR=<dir> {UPDATE_ENV}=1 \
                         cargo nextest run -p rr-rom --test corpus_rom_resolution",
                        first_difference(&text, &committed)
                    );
                }
            }
            Ok(())
        }
    }
}
