//! Legacy behaviour catalogue check (`cargo xtask check-legacy-docs`, story
//! US-RR-12, task US-RR-12-1).
//!
//! The catalogue in [`DOCS_DIR`] specifies what the Java RomRaider does, one
//! page per area, citing `legacy/` `file:line` for every behaviour it
//! describes (read, never run: D-024). This check keeps that true as the pages
//! and `legacy/` change. It fails on any of:
//!
//! 1. **Areas.** Every area of the story ([`AREAS`]) maps to its pinned page,
//!    which exists; `README.md`'s "Pages" table links exactly those pages, each
//!    row's area cell naming its area (the pinned keyword); no other page sits
//!    in the directory unlisted.
//! 2. **References resolve.** Every reference in a page (headings, prose,
//!    tables, lists and code blocks alike) resolves and is in range. A
//!    reference is `path:N`, `path:N-M` or a comma list of those
//!    (`Settings.java:212-215,222`), where `path` ends in a file extension;
//!    a bare `` `:N` `` continues the last file of the same paragraph, item or
//!    row; [`ALIASES`] are a page's declared short names (`Unmarshaller:N`).
//!    `path` resolves, in order: from the repository root when its first
//!    component is a top-level directory of the repository (`legacy/...`,
//!    `crates/...`); from [`JAVA_ROOT`] (the pages' stated convention);
//!    otherwise by unique suffix, first among the files under [`JAVA_ROOT`],
//!    then among all of `legacy/` (`iso9141/SSMProtocol.java`,
//!    `LoggerDefinitionHandler.java`). A suffix that matches two files is an
//!    error: the page must say which. Files in [`EXTERNAL`] (the pinned
//!    definition corpus, not in the repository) are checked for form only.
//!    Every range must have `1 <= N <= M <= lines in the file`.
//! 3. **Each behaviour carries a reference.** A *behaviour unit* is a
//!    paragraph, a list item (with its continuation lines) or a table body
//!    row of a page, outside [`EXCLUDED_SECTIONS`] (how to read the page and
//!    its source-file list, "Mapping to the port", "Fixtures to write") and
//!    outside worked-example sections (heading "Worked example(s)",
//!    "Example", "Hand-composed ..."). In "Ledgered issues" only the table
//!    rows are units (its prose is triage notes). A unit is *referenced* when
//!    it contains a legacy reference: a `legacy/` file reference as in rule
//!    2, a page alias, a continuation, a corpus reference ([`EXTERNAL`]), a
//!    JEP bytecode reference (`JEP.<init>@24-26`, the encoding page's
//!    convention for the binary `jep.jar`), or a pointer to the section that
//!    holds the reference (`section 4.1`, `(1.6)`, or a sibling page). A
//!    `section N.M` pointer must name a heading of the page, unless the unit
//!    names another page. Then:
//!    * a table with a `Source` column needs a reference **in that cell** of
//!      every row; other table rows and list items need one in the row or
//!      item, or in the lead-in paragraph (ending with `:`) just above the
//!      table or list; a nested item may lean on its parent item likewise; a
//!      table without a `Source` column may also lean on a `Sources: ...`
//!      paragraph right below it, or on a `Section` cell naming a section of
//!      the page;
//!    * a paragraph needs one, unless it is a lead-in (ends with `:`, and a
//!      list, table or code block follows: those units are checked
//!      instead) or a worked example (`*Worked example ...*`, `*Example*`),
//!      which also covers the list, table and code blocks right after it;
//!    * [`EXEMPT_UNITS`] names the few intentional reference-free statements,
//!      each with a reason. An entry that no longer matches a unit fails.
//! 4. **References point at what they describe.** Line ranges can be in
//!    range and still wrong (US-RR-12-4 found 24 references 39 lines off).
//!    For each reference into a `legacy/` text file, the *identifiers* named
//!    in backticks in the same clause (the text between the previous
//!    reference, or the start of the sentence or table cell, and this one;
//!    failing that, the text up to the next reference) are code-like tokens
//!    (`camelCase`, `PascalCase` with two capitals, `snake_case`, `ALLCAPS4`,
//!    at least [`MIN_IDENT`] characters). Those that occur anywhere in the
//!    cited file are the reference's *anchors*; if it has any, at least one
//!    must occur within the cited lines, widened by [`SLACK`] lines each
//!    side. A reference with no anchors is not content-checked (the report
//!    counts them). [`CONTENT_EXEMPT`] lists the references whose anchors are
//!    correctly absent, each with a reason; stale entries fail. Plain words
//!    (XML names such as `swapxy`) count only when the Java file spells them
//!    as a string literal; Java keywords, paths, the file's own class name,
//!    and anchors found on more than [`MAX_ANCHOR_LINES`] lines are not
//!    used; a method named in the text may sit on the declaration line of
//!    the cited body. In a table row, a reference with no anchor in its own
//!    cell takes the anchors of the row's first cell.
//! 5. **Pinned sample.** [`PINNED_ANCHORS`] pins at least eight references
//!    per page, and every `iso9141/SSMResponseProcessor.java` reference, to
//!    text their cited lines must contain, for references whose sentence
//!    names nothing rule 4 can use. A pinned reference that is no longer on
//!    its page fails, so a corrected reference is re-pinned.
//! 6. **Every fixture is tracked** (US-RR-12-2, [`fixtures`]). Each page's
//!    last section is a "Fixtures to write" table of stable IDs
//!    (`LEG-<PREFIX>-NN`, the page's prefix in [`FIXTURE_PREFIXES`]); an ID
//!    mentioned anywhere on a page is one of those. [`fixtures::FIXTURES_MAP`]
//!    maps exactly those IDs, each to existing KAT, fixture or test files
//!    (`kat`), a ProjectMan task (`task`: `.project/tasks/<task>.md` exists,
//!    is not archived and names the ID), or both.
//! 7. **Bugs cite the ledger** (US-RR-12-3, [`ledger`]). Every `MF-NN` /
//!    `EC-NN` a page cites is an entry of PROJECT.md's Must-Fix ledger or
//!    edge-case list; the retired triage IDs (`UL-<PREFIX>-NN`) appear only
//!    as the first cell of the page's "Ledgered issues" table, which maps
//!    each to its entry (or says why it is not a bug); and every behaviour
//!    unit that calls legacy behaviour a bug (a word of [`ledger::BUG_WORDS`])
//!    cites an entry, leans on a lead-in that does, or is in
//!    [`ledger::BUG_EXEMPT`].
//!
//! CI runs it in the `clippy` job of `ci.yml` (wiring pinned by
//! `xtask/tests/legacy_docs_ci_config.rs`); `xtask/tests/legacy_docs.rs` runs
//! it on the committed pages and on planted bad pages.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

pub mod fixtures;
pub mod ledger;

pub use fixtures::FixtureStats;
pub use ledger::LedgerStats;

/// The catalogue directory, relative to the repository root.
pub const DOCS_DIR: &str = "docs/legacy";

/// The catalogue index page.
pub const README: &str = "README.md";

/// The legacy tree, relative to the repository root.
pub const LEGACY_DIR: &str = "legacy";

/// Where page references are relative to, unless they say otherwise.
pub const JAVA_ROOT: &str = "legacy/src/main/java/com/romraider";

/// Story US-RR-12's areas: `(area, page, keyword the README's area cell
/// must contain, case-insensitively)`.
pub const AREAS: &[(&str, &str, &str)] = &[
    (
        "definition parsing and inheritance",
        "definitions.md",
        "definition parsing and inheritance",
    ),
    (
        "storage, endian and scaling semantics incl. JEP dialect quirks",
        "encoding-scaling.md",
        "scaling",
    ),
    ("each checksum algorithm", "checksums.md", "checksum"),
    ("SSM/K-line protocol flow", "ssm.md", "K-line"),
    (
        "logger definition semantics",
        "logger.md",
        "logger definition",
    ),
    ("CSV format", "csv.md", "CSV"),
    ("settings.xml schema", "settings.md", "settings.xml"),
];

/// Each page's fixture-ID prefix: `(page, PREFIX)`, its "Fixtures to write"
/// IDs being `LEG-PREFIX-NN` (rule 6).
pub const FIXTURE_PREFIXES: &[(&str, &str)] = &[
    ("definitions.md", "DEF"),
    ("encoding-scaling.md", "ENC"),
    ("checksums.md", "CHK"),
    ("ssm.md", "SSM"),
    ("logger.md", "LOG"),
    ("csv.md", "CSV"),
    ("settings.md", "SET"),
];

/// Short names a page declares in its "How to read this page":
/// `(page, alias, target)`; `alias:N` means `target:N`.
pub const ALIASES: &[(&str, &str, &str)] = &[
    (
        "settings.md",
        "Unmarshaller",
        "xml/DOMSettingsUnmarshaller.java",
    ),
    ("settings.md", "Builder", "xml/DOMSettingsBuilder.java"),
];

/// Files cited by line that are not in the repository: the pinned logger
/// definition corpus (TD-D/SubaruDefs, D-028) and the private v370 file. Only
/// the form of these references is checked.
pub const EXTERNAL: &[&str] = &[
    "logger_STD_EN_v290.xml",
    "logger_METRIC_EN_v290.xml",
    "logger_STD_EN_v370.xml",
];

/// Top-level `##` sections that are not behaviour (matched on the heading
/// text with any `N.` numbering removed).
pub const EXCLUDED_SECTIONS: &[&str] = &[
    "How to read this page",
    "Mapping to the port",
    "Fixtures to write",
];

/// The `##` section whose prose is notes on the ledger: only its table rows
/// are behaviour units (rule 7's [`ledger::LEDGERED_SECTION`]).
pub const ROWS_ONLY_SECTION: &str = ledger::LEDGERED_SECTION;

/// Minimum length of an identifier used as a content anchor.
pub const MIN_IDENT: usize = 4;

/// An anchor that occurs on more lines of the cited file than this is not
/// used: it would be found near almost any line.
pub const MAX_ANCHOR_LINES: usize = 30;

/// Lines either side of a cited range in which an anchor still counts.
pub const SLACK: usize = 2;

/// Intentionally reference-free behaviour units: `(page, start of the unit's
/// text after any list marker, why it needs no reference)`.
pub const EXEMPT_UNITS: &[(&str, &str, &str)] = &[
    (
        "definitions.md",
        "EC-04 lists the cases the port must test.",
        "a pointer to the PROJECT.md edge-case ledger; the cases are cited in the table above it",
    ),
    (
        "settings.md",
        "The importer must therefore not trust the declaration",
        "importer guidance drawn from the cited encoding behaviour just above, not a legacy behaviour",
    ),
];

/// References whose anchors are correctly outside the cited lines:
/// `(page, reference as written, why)`.
pub const CONTENT_EXEMPT: &[(&str, &str, &str)] = &[
    (
        "definitions.md",
        "swing/ScalesTableModel.java:70-72",
        "the cited lines are `getMin()`/`getMax()`; 3-letter anchors match whole words only",
    ),
    (
        "definitions.md",
        "xml/TableScaleUnmarshaller.java:461",
        "line 461 is `scales.put`, the map the `<scalingbase>`s share; the element is read elsewhere",
    ),
    (
        "logger.md",
        "EcuDataLoaderImpl.java:114-118,137-140",
        "`HashMap` is the type of `protocolList`, declared elsewhere; the cited lines iterate it",
    ),
    (
        "encoding-scaling.md",
        "util/JEPUtil.java:62-71",
        "the text says this entry point registers no `BitWise`: the name is rightly absent",
    ),
    (
        "logger.md",
        "EcuLogger.java:581-582",
        "the cited call passes `getLoggerProtocol()`; `SSM` is the default named in Settings.java",
    ),
    (
        "logger.md",
        "LoggerDefinitionHandler.java:382",
        "line 382 is the `ecuAddressMap.put` inside the `</ecu>` branch that starts at line 378",
    ),
    (
        "logger.md",
        "LoggerDefinitionHandler.java:367",
        "line 367 is the `ecuAddressMap.containsKey(ecuInit.getEcuId())` test the row describes",
    ),
    (
        "logger.md",
        "LoggerDefinitionHandler.java:329-342",
        "the cited lines resolve `ref` IDs through `dependsList`/`refid`, not the word `ref`",
    ),
    (
        "ssm.md",
        "iso9141/SSMProtocol.java:97,105",
        "cites the frame comments that call the byte \"padding\"; `padContent` is cited separately",
    ),
];

/// A pinned sample of references per page, each with text the cited lines
/// must contain: `(page, reference as written on the page, needle)`. The
/// reference must still be on the page, and some cited line must contain
/// the needle (exact, case-sensitive). This backs rule 4 where the text names
/// no identifier, including every `iso9141/SSMResponseProcessor.java`
/// reference of `ssm.md` (the US-RR-12-4 defect).
pub const PINNED_ANCHORS: &[(&str, &str, &str)] = &[
    (
        "checksums.md",
        "logger/external/ecotrons/io/AlmChecksumCalculator.java:31-37",
        "calculateChecksum",
    ),
    (
        "checksums.md",
        "maps/checksum/ChecksumE38PCM.java:77-95",
        "for (int k = 0; k < 2; k++)",
    ),
    (
        "checksums.md",
        "maps/Rom.java:312-318,330-332",
        "faultyTables",
    ),
    (
        "checksums.md",
        "maps/checksum/ChecksumMOTRONICDOUBLE.java:52-54",
        "start2, end2",
    ),
    (
        "checksums.md",
        "maps/Rom.java:453-455,469",
        "binDataPos + 207",
    ),
    (
        "checksums.md",
        "xml/ConversionLayer/BMWCodingConversionLayer.java:59,107-109",
        "guessChecksums",
    ),
    (
        "checksums.md",
        "maps/checksum/NissanChecksum.java:76-81",
        "public int update",
    ),
    (
        "checksums.md",
        "xml/RomAttributeParser.java:175-177",
        "IndexOutOfBoundsException",
    ),
    (
        "csv.md",
        "logger/ecu/ui/tab/dyno/DynoControlPanel.java:1009-1094",
        "SEMICOLON",
    ),
    (
        "csv.md",
        "logger/ecu/ui/handler/file/FileLoggerImpl.java:111",
        "getBytes()",
    ),
    (
        "csv.md",
        "file/FileUpdateHandlerImpl.java:191-198",
        "headers()",
    ),
    (
        "csv.md",
        "logger/ecu/definition/ExternalDataConvertorImpl.java:57-59",
        "getUnits",
    ),
    (
        "csv.md",
        "logger/ecu/ui/swing/tools/ReadCodesResultsPanel.java:64,161-203",
        "DT_FORMAT",
    ),
    (
        "csv.md",
        "logger/ecu/ui/playback/PlaybackManagerImpl.java:30-83",
        "TODO: Finish me",
    ),
    (
        "csv.md",
        "DynoControlPanel.java:1049-1053,1055-1081",
        "replace(',', '.')",
    ),
    (
        "csv.md",
        "logger/ecu/EcuLogger.java:953-962",
        "addHandler(fileUpdateHandler)",
    ),
    (
        "definitions.md",
        "xml/TableScaleUnmarshaller.java:162-170",
        "memModelEndian == null",
    ),
    (
        "definitions.md",
        "maps/DataCell.java:481-484",
        "table.locked",
    ),
    (
        "definitions.md",
        "maps/RomID.java:241-244",
        "disableRamOffset",
    ),
    (
        "definitions.md",
        "xml/TableScaleUnmarshaller.java:151-152",
        "\"category\"",
    ),
    (
        "definitions.md",
        "xml/TableScaleUnmarshaller.java:87-98",
        "ObjectCloner",
    ),
    (
        "definitions.md",
        "xml/DOMRomUnmarshaller.java:151-152,208",
        "getBaseRom",
    ),
    (
        "definitions.md",
        "xml/TableScaleUnmarshaller.java:150",
        "\"dataLayout\"",
    ),
    (
        "definitions.md",
        "xml/DOMRomUnmarshaller.java:118-119",
        "unmarshallRomID",
    ),
    ("definitions.md", "Settings.java:187", "displayHighTables"),
    (
        "encoding-scaling.md",
        "xml/RomAttributeParser.java:133-178",
        "parseByteValue",
    ),
    (
        "encoding-scaling.md",
        "maps/DataCell.java:440-442",
        "approximateToByteFunction",
    ),
    (
        "encoding-scaling.md",
        "xml/RomAttributeParser.java:124",
        "OK = 11110000",
    ),
    ("encoding-scaling.md", "util/JEPUtil.java:28-83", "LRUCache"),
    (
        "encoding-scaling.md",
        "maps/DataCell.java:161-167",
        "STORAGE_TYPE_FLOAT",
    ),
    (
        "encoding-scaling.md",
        "maps/DataCell.java:132-134",
        "Math.pow(256",
    ),
    (
        "encoding-scaling.md",
        "util/ByteUtil.java:122-133",
        "firstOneOfMask",
    ),
    (
        "encoding-scaling.md",
        "maps/DataCell.java:261,447-449,530",
        "isBoschSubtract",
    ),
    (
        "logger.md",
        "LoggerDefinitionHandler.java:440-451",
        "Engine Control Unit",
    ),
    (
        "logger.md",
        "EcuDataLoaderImpl.java:114-117,130-132",
        "protocolList.keySet().iterator().next()",
    ),
    (
        "logger.md",
        "LoggerDefinitionLocationAction.java:46",
        "loadLoggerParams",
    ),
    (
        "logger.md",
        "logger/ecu/ui/tab/maf/MafControlPanel.java:74-85",
        "\"P8\"",
    ),
    (
        "logger.md",
        "LoggerDefinitionHandler.java:246,331-336",
        "dependsList",
    ),
    (
        "logger.md",
        "LoggerDefinitionHandler.java:274-275",
        "ATTR_ECUBYTEINDEX",
    ),
    (
        "logger.md",
        "EcuSwitchConvertorImpl.java:39-41,55-58",
        "(1 << bit)",
    ),
    ("logger.md", "EcuDefinitionHandler.java:124-161", "TAG_ROM"),
    ("settings.md", "Unmarshaller:159", "setDisplayHighTables"),
    ("settings.md", "Builder:342-343", "\"protocol\""),
    ("settings.md", "Builder:225-227", "alwaysopentableatzero"),
    ("settings.md", "Unmarshaller:135", "setDebug"),
    ("settings.md", "Builder:140-146", "ecudefinitionfile"),
    ("settings.md", "Unmarshaller:199-201", "cellsize"),
    ("settings.md", "Unmarshaller:256-258", "setLocale"),
    ("settings.md", "Unmarshaller:91-92", "setWindowLocation"),
    (
        "ssm.md",
        "iso9141/SSMChecksumCalculator.java:31-38",
        "bytes.length - 1",
    ),
    (
        "ssm.md",
        "QueryManagerImpl.java:359-367,172-185",
        "sleep(500L)",
    ),
    (
        "ssm.md",
        "TestSerialConnection2.java:130,136-145,150",
        "0x80F01006E83F600000000D",
    ),
    ("ssm.md", "iso9141/SSMProtocol.java:57", "ADDRESS_SIZE = 3"),
    (
        "ssm.md",
        "SSMLoggerConnection.java:163-164",
        "getBytes().length == 3",
    ),
    (
        "ssm.md",
        "iso9141/SSMLoggerProtocol.java:98-114",
        "processReadAddressResponses",
    ),
    (
        "ssm.md",
        "logger/ecu/comms/readcodes/ReadCodesManagerImpl.java:66-74",
        "ecuInitLength < 104",
    ),
    ("ssm.md", "iso9141/SSMProtocol.java:192-194", "return 55"),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:66-68",
        "module.getAddress()",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:46-60",
        "filterRequestFromResponse",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:55-58",
        "STATE_1",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:52",
        "response.length - request.length",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:65",
        "Invalid header",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:66",
        "Invalid diagnostic tool id",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:67-68",
        "module.getName()",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:69",
        "Invalid response data length",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:70",
        "Invalid response code",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:74-81",
        "extractResponseData",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:71",
        "Invalid checksum",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:64",
        "Invalid response length",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:62-72,84-111",
        "InvalidResponseException",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:48",
        "checkNotNullOrEmpty(response",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:52-53",
        "System.arraycopy(response, request.length",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:62-72",
        "validateResponse",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:51-54",
        "STATE_0",
    ),
    (
        "ssm.md",
        "iso9141/SSMResponseProcessor.java:31,66-68",
        "SSMProtocol.module",
    ),
];

/// What the check is configured with; [`Config::pinned`] is the committed
/// configuration, tests build their own.
#[derive(Debug, Clone)]
pub struct Config<'a> {
    pub areas: &'a [(&'a str, &'a str, &'a str)],
    pub aliases: &'a [(&'a str, &'a str, &'a str)],
    pub external: &'a [&'a str],
    pub exempt_units: &'a [(&'a str, &'a str, &'a str)],
    pub content_exempt: &'a [(&'a str, &'a str, &'a str)],
    pub pinned_anchors: &'a [(&'a str, &'a str, &'a str)],
    /// Rule 6's page prefixes; empty turns rule 6 off (planted trees that
    /// test rules 1 to 5 only).
    pub fixture_prefixes: &'a [(&'a str, &'a str)],
    /// Rule 7's ledger file, relative to the root; `None` turns rule 7 off.
    pub ledger: Option<&'a str>,
    /// Rule 7's bug words and their allowlist.
    pub bug_words: &'a [&'a str],
    pub bug_exempt: &'a [(&'a str, &'a str, &'a str)],
}

impl Config<'static> {
    /// The committed configuration.
    pub fn pinned() -> Self {
        Self {
            areas: AREAS,
            aliases: ALIASES,
            external: EXTERNAL,
            exempt_units: EXEMPT_UNITS,
            content_exempt: CONTENT_EXEMPT,
            pinned_anchors: PINNED_ANCHORS,
            fixture_prefixes: FIXTURE_PREFIXES,
            ledger: Some(ledger::LEDGER),
            bug_words: ledger::BUG_WORDS,
            bug_exempt: ledger::BUG_EXEMPT,
        }
    }
}

/// Per-page counts, for the summary line and the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PageStats {
    /// References of every kind found on the page.
    pub refs: usize,
    /// References resolved to a file in the repository and range-checked.
    pub resolved: usize,
    /// References whose cited lines were checked for an anchor.
    pub content_checked: usize,
    /// Behaviour units, and how many carry or inherit a reference.
    pub units: usize,
    pub referenced_units: usize,
    /// [`PINNED_ANCHORS`] entries checked on this page.
    pub pinned: usize,
    /// IDs in the page's "Fixtures to write" table (rule 6).
    pub fixtures: usize,
}

/// The outcome of a run.
#[derive(Debug, Default)]
pub struct Report {
    pub problems: Vec<String>,
    pub pages: BTreeMap<String, PageStats>,
    /// Rule 6's fixture map.
    pub fixtures: FixtureStats,
    /// Rule 7's ledger cross-references.
    pub ledger: LedgerStats,
}

/// Runs the check on the repository at `root` with `config`.
pub fn run(root: &Path, config: &Config<'_>) -> Result<Report> {
    let mut report = Report::default();
    let docs = root.join(DOCS_DIR);
    if !docs.is_dir() {
        report
            .problems
            .push(format!("{DOCS_DIR}: missing (the catalogue directory)"));
        return Ok(report);
    }
    let legacy = Legacy::index(root)?;

    // 1. Areas, pages and the README index.
    let pinned: BTreeSet<&str> = config.areas.iter().map(|(_, p, _)| *p).collect();
    for (area, page, _) in config.areas {
        if !docs.join(page).is_file() {
            report.problems.push(format!(
                "{DOCS_DIR}/{page}: missing (the page for area \"{area}\")"
            ));
        }
    }
    for entry in fs::read_dir(&docs).with_context(|| format!("reading {}", docs.display()))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && name != README && !pinned.contains(name.as_str()) {
            report.problems.push(format!(
                "{DOCS_DIR}/{name}: a page that maps to no area of US-RR-12 (add it to AREAS or remove it)"
            ));
        }
    }
    match fs::read_to_string(docs.join(README)) {
        Ok(text) => report.problems.extend(readme_problems(&text, config)),
        Err(e) => report
            .problems
            .push(format!("{DOCS_DIR}/{README}: unreadable: {e}")),
    }

    // Headings of every page, for section pointers.
    let mut texts = BTreeMap::new();
    for (_, page, _) in config.areas {
        if let Ok(t) = fs::read_to_string(docs.join(page)) {
            texts.insert((*page).to_owned(), t.replace("\r\n", "\n"));
        }
    }
    let mut used_exempt = BTreeSet::new();
    let mut used_content = BTreeSet::new();
    for (page, text) in &texts {
        let mut ctx = PageCtx {
            page,
            config,
            legacy: &legacy,
            problems: &mut report.problems,
            stats: PageStats::default(),
            used_exempt: &mut used_exempt,
            used_content: &mut used_content,
        };
        ctx.check(text);
        let stats = ctx.stats;
        report.pages.insert(page.clone(), stats);
    }
    for (page, r, needle) in config.pinned_anchors {
        let Some(text) = texts.get(*page) else {
            report.problems.push(format!(
                "{DOCS_DIR}/{page}: PINNED_ANCHORS names a page that is not an area page"
            ));
            continue;
        };
        if let Some(stats) = report.pages.get_mut(*page) {
            stats.pinned += 1;
        }
        if let Some(p) = pinned_problem(&legacy, config, page, text, r, needle) {
            report.problems.push(p);
        }
    }
    // 6. Fixtures to write, and the map that tracks them.
    let (fixture_stats, per_page) = fixtures::check(root, config, &texts, &mut report.problems);
    report.fixtures = fixture_stats;
    for (page, n) in per_page {
        if let Some(stats) = report.pages.get_mut(&page) {
            stats.fixtures = n;
        }
    }
    // 7. Bugs cite the ledger.
    report.ledger = ledger::check(root, config, &texts, &mut report.problems);
    for (i, (page, starts, _)) in config.exempt_units.iter().enumerate() {
        if !used_exempt.contains(&i) {
            report.problems.push(format!(
                "{DOCS_DIR}/{page}: EXEMPT_UNITS entry {starts:?} matches no unreferenced behaviour unit (stale: remove it)"
            ));
        }
    }
    for (i, (page, r, _)) in config.content_exempt.iter().enumerate() {
        if !used_content.contains(&i) {
            report.problems.push(format!(
                "{DOCS_DIR}/{page}: CONTENT_EXEMPT entry {r:?} matches no failing reference (stale: remove it)"
            ));
        }
    }
    Ok(report)
}

/// Rules 2 to 4 for one page's `text`, against the `legacy/` tree under
/// `root`, without the README and allowlist-staleness checks of [`run`].
pub fn check_page(
    root: &Path,
    page: &str,
    text: &str,
    config: &Config<'_>,
) -> Result<(Vec<String>, PageStats)> {
    let legacy = Legacy::index(root)?;
    let mut problems = Vec::new();
    let mut used_exempt = BTreeSet::new();
    let mut used_content = BTreeSet::new();
    let mut ctx = PageCtx {
        page,
        config,
        legacy: &legacy,
        problems: &mut problems,
        stats: PageStats::default(),
        used_exempt: &mut used_exempt,
        used_content: &mut used_content,
    };
    ctx.check(&text.replace("\r\n", "\n"));
    let stats = ctx.stats;
    Ok((problems, stats))
}

/// The problem with one [`PINNED_ANCHORS`] entry, if any.
fn pinned_problem(
    legacy: &Legacy,
    config: &Config<'_>,
    page: &str,
    text: &str,
    r: &str,
    needle: &str,
) -> Option<String> {
    let at = format!("{DOCS_DIR}/{page}");
    if !text.contains(r) {
        return Some(format!(
            "{at}: PINNED_ANCHORS reference `{r}` is no longer on the page (re-pin it to the corrected reference)"
        ));
    }
    let aliases: Vec<(&str, &str)> = config
        .aliases
        .iter()
        .filter(|(p, _, _)| *p == page)
        .map(|(_, a, t)| (*a, *t))
        .collect();
    let tok = scan_refs(r, &aliases).into_iter().next();
    let path = match tok.as_ref().map(|t| &t.kind) {
        Some(RefKind::File(p) | RefKind::Alias(p)) => p.clone(),
        _ => {
            return Some(format!(
                "{at}: PINNED_ANCHORS entry `{r}` is not a file reference"
            ));
        }
    };
    let Resolved::File { rel, .. } = legacy.resolve(&path, config.external) else {
        return Some(format!(
            "{at}: PINNED_ANCHORS reference `{r}` does not resolve"
        ));
    };
    let lines = legacy.lines(&rel)?;
    let ranges = tok.map(|t| t.ranges).unwrap_or_default();
    let hit = ranges.iter().any(|(a, b)| {
        (*a..=*b).any(|n| {
            lines
                .get(n.wrapping_sub(1))
                .is_some_and(|l| l.contains(needle))
        })
    });
    if hit {
        return None;
    }
    let found: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(n, _)| (n + 1).to_string())
        .take(6)
        .collect();
    Some(format!(
        "{at}: `{r}`: the cited lines of {rel} do not contain the pinned anchor {needle:?} (it is at line(s) {})",
        if found.is_empty() {
            "none".to_owned()
        } else {
            found.join(",")
        }
    ))
}

/// Rule 1 over the README text.
pub fn readme_problems(text: &str, config: &Config<'_>) -> Vec<String> {
    let mut problems = Vec::new();
    let text = text.replace("\r\n", "\n");
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut in_pages = false;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            in_pages = h.trim() == "Pages";
            continue;
        }
        if in_pages && line.trim_start().starts_with('|') && !is_delimiter_row(line) {
            rows.push(split_cells(line));
        }
    }
    if rows.is_empty() {
        problems.push(format!(
            "{DOCS_DIR}/{README}: no \"## Pages\" table (the index of area pages)"
        ));
        return problems;
    }
    // Skip the header row.
    let mut linked: BTreeMap<String, String> = BTreeMap::new();
    for row in rows.iter().skip(1) {
        let first = row.first().map(String::as_str).unwrap_or("");
        let Some(page) = link_target(first) else {
            problems.push(format!(
                "{DOCS_DIR}/{README}: Pages row {first:?} does not link a page"
            ));
            continue;
        };
        let area = row.get(1).cloned().unwrap_or_default();
        if linked.insert(page.clone(), area).is_some() {
            problems.push(format!("{DOCS_DIR}/{README}: page {page} is listed twice"));
        }
    }
    for (area, page, keyword) in config.areas {
        match linked.get(*page) {
            None => problems.push(format!(
                "{DOCS_DIR}/{README}: Pages table does not list {page} (area \"{area}\")"
            )),
            Some(cell) if !cell.to_lowercase().contains(&keyword.to_lowercase()) => {
                problems.push(format!(
                    "{DOCS_DIR}/{README}: the area cell of {page} does not name its area (expected {keyword:?} in {cell:?})"
                ));
            }
            Some(_) => {}
        }
    }
    for page in linked.keys() {
        if !config.areas.iter().any(|(_, p, _)| p == page) {
            problems.push(format!(
                "{DOCS_DIR}/{README}: Pages table lists {page}, which maps to no area"
            ));
        }
    }
    problems
}

/// `[x](page.md)` or `[x](page.md#frag)` → `page.md`.
fn link_target(cell: &str) -> Option<String> {
    let start = cell.find("](")? + 2;
    let rest = cell.get(start..)?;
    let end = rest.find(')')?;
    let target = rest.get(..end)?;
    let target = target.split('#').next()?;
    target.ends_with(".md").then(|| target.to_owned())
}

// ---------------------------------------------------------------------------
// The legacy tree.

/// Every file under `legacy/` (relative to the root, `/`-separated), with a
/// cache of file lines.
struct Legacy {
    root: PathBuf,
    files: Vec<String>,
    cache: std::cell::RefCell<HashMap<String, Option<std::rc::Rc<Vec<String>>>>>,
}

/// Where a path in a reference leads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resolved {
    /// A file in the repository (relative to the root); `legacy` when under
    /// `legacy/`.
    File {
        rel: String,
        legacy: bool,
    },
    /// A pinned external file: form only.
    External,
    Missing,
    Ambiguous(Vec<String>),
    Glob,
}

impl Legacy {
    fn index(root: &Path) -> Result<Self> {
        let mut files = Vec::new();
        let base = root.join(LEGACY_DIR);
        if base.is_dir() {
            walk(&base, LEGACY_DIR, &mut files)?;
        }
        files.sort();
        Ok(Self {
            root: root.to_path_buf(),
            files,
            cache: std::cell::RefCell::new(HashMap::new()),
        })
    }

    fn resolve(&self, path: &str, external: &[&str]) -> Resolved {
        if path.contains('*') {
            return Resolved::Glob;
        }
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let first = path.split('/').next().unwrap_or("");
        if path.contains('/') && !first.is_empty() && self.root.join(first).is_dir() {
            // Rooted at the repository: `legacy/...`, `crates/...`.
            return if self.root.join(path).is_file() {
                Resolved::File {
                    rel: path.to_owned(),
                    legacy: first == LEGACY_DIR,
                }
            } else {
                Resolved::Missing
            };
        }
        if external.contains(&file_name) && (path == file_name || path.ends_with(file_name)) {
            return Resolved::External;
        }
        let direct = format!("{JAVA_ROOT}/{path}");
        if self.files.binary_search(&direct).is_ok() {
            return Resolved::File {
                rel: direct,
                legacy: true,
            };
        }
        let suffix = format!("/{path}");
        let main: Vec<&String> = self
            .files
            .iter()
            .filter(|f| f.starts_with(JAVA_ROOT) && f.ends_with(&suffix))
            .collect();
        let pick = |m: Vec<&String>| match m.as_slice() {
            [one] => Some(Resolved::File {
                rel: (*one).clone(),
                legacy: true,
            }),
            [] => None,
            many => Some(Resolved::Ambiguous(
                many.iter().map(|s| (*s).clone()).collect(),
            )),
        };
        if let Some(r) = pick(main) {
            return r;
        }
        let all: Vec<&String> = self.files.iter().filter(|f| f.ends_with(&suffix)).collect();
        pick(all).unwrap_or(Resolved::Missing)
    }

    /// The lines of `rel` (relative to the root), `None` if unreadable.
    fn lines(&self, rel: &str) -> Option<std::rc::Rc<Vec<String>>> {
        if let Some(hit) = self.cache.borrow().get(rel) {
            return hit.clone();
        }
        let loaded = fs::read(self.root.join(rel)).ok().map(|bytes| {
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
            let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
            if text.ends_with('\n') {
                lines.pop();
            }
            std::rc::Rc::new(lines)
        });
        self.cache
            .borrow_mut()
            .insert(rel.to_owned(), loaded.clone());
        loaded
    }
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk(&entry.path(), &child, out)?;
        } else if ty.is_file() {
            out.push(child);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// References in text.

/// One `first-last` range (1-based, inclusive).
type Range = (usize, usize);

/// What a reference token is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RefKind {
    /// `path:lines`.
    File(String),
    /// A page alias `Alias:lines`, with its target path.
    Alias(String),
    /// `` `:lines` ``: the previous file of the unit.
    Continuation,
    /// `Class.method@offsets` into a binary library.
    Bytecode,
}

/// One reference token in a unit's text.
#[derive(Debug, Clone)]
struct RefTok {
    kind: RefKind,
    /// As written (`path:lines`).
    text: String,
    ranges: Vec<Range>,
    /// Byte span in the unit text.
    start: usize,
    end: usize,
}

fn is_path_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'/' | b'-' | b'$' | b'*')
}

/// Parses `N`, `N-M` and comma lists at `s[i..]`; returns the ranges and the
/// end. A malformed range (`M < N`) is returned as is, for the caller to flag.
fn parse_lines(s: &[u8], mut i: usize) -> (Vec<Range>, usize) {
    let mut ranges = Vec::new();
    let num = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while s.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        std::str::from_utf8(s.get(start..*i)?).ok()?.parse().ok()
    };
    while let Some(a) = num(&mut i) {
        let mut b = a;
        if s.get(i) == Some(&b'-') && s.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 1;
            b = num(&mut i).unwrap_or(a);
        }
        ranges.push((a, b));
        if s.get(i) == Some(&b',') && s.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 1;
        } else {
            break;
        }
    }
    (ranges, i)
}

/// Every reference token in `text`.
fn scan_refs(text: &str, aliases: &[(&str, &str)]) -> Vec<RefTok> {
    let s = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        let digit_next = s.get(i + 1).is_some_and(u8::is_ascii_digit);
        if (c == b':' || c == b'@') && digit_next {
            let mut start = i;
            let back = |b: u8| {
                if c == b'@' {
                    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$' | b'<' | b'>')
                } else {
                    is_path_char(b)
                }
            };
            while start > 0 && s.get(start - 1).copied().is_some_and(back) {
                start -= 1;
            }
            let path = text.get(start..i).unwrap_or("");
            let (ranges, end) = parse_lines(s, i + 1);
            let tok_text = text.get(start..end).unwrap_or("").to_owned();
            let kind = if c == b'@' {
                let (class, member) = path.split_once('.').unwrap_or(("", ""));
                (class.chars().next().is_some_and(|f| f.is_ascii_uppercase()) && !member.is_empty())
                    .then_some(RefKind::Bytecode)
            } else if path.is_empty() {
                (start > 0 && s.get(start - 1) == Some(&b'`')).then_some(RefKind::Continuation)
            } else if let Some((_, target)) = aliases.iter().find(|(a, _)| *a == path) {
                Some(RefKind::Alias((*target).to_owned()))
            } else if has_extension(path) {
                Some(RefKind::File(path.to_owned()))
            } else {
                None
            };
            if let Some(kind) = kind {
                out.push(RefTok {
                    kind,
                    text: tok_text,
                    ranges,
                    start,
                    end,
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Whether the last path component has an alphabetic extension (`.java`).
fn has_extension(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=12).contains(&ext.len())
                && ext.bytes().all(|b| b.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Section pointers in `text`: `section 4.1`, `sections 2 and 3`, `(1.6)`;
/// returns the numbers, and whether the text names a sibling page
/// (`` `logger.md` `` or a `](logger.md...)` link), which is a pointer too and
/// means the numbers need not be this page's.
fn section_pointers(text: &str, pages: &[&str]) -> (Vec<String>, bool) {
    let mut nums = Vec::new();
    let lower = text.to_lowercase();
    let b = lower.as_bytes();
    let mut i = 0;
    while let Some(off) = lower.get(i..).and_then(|r| r.find("section")) {
        let mut j = i + off + "section".len();
        if b.get(j) == Some(&b's') {
            j += 1;
        }
        if b.get(j) == Some(&b' ') && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
            let (n, _) = read_section_number(b, j + 1);
            nums.push(n);
        }
        i = j;
    }
    // `(1.6)`, `(8.6);`, `(section 2.3)` already counted above.
    for (k, _) in lower.match_indices('(') {
        if b.get(k + 1).is_some_and(u8::is_ascii_digit) {
            let (n, end) = read_section_number(b, k + 1);
            if n.contains('.') && matches!(b.get(end), Some(b')' | b',' | b';')) {
                nums.push(n);
            }
        }
    }
    let names_page = pages
        .iter()
        .any(|p| text.contains(&format!("`{p}`")) || text.contains(&format!("]({p}")));
    (nums, names_page)
}

fn read_section_number(b: &[u8], mut i: usize) -> (String, usize) {
    let start = i;
    while b.get(i).is_some_and(|c| c.is_ascii_digit())
        || (b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit))
    {
        i += 1;
    }
    (
        String::from_utf8_lossy(b.get(start..i).unwrap_or(&[])).into_owned(),
        i,
    )
}

/// Extensions that make a backtick span a file name rather than code.
const FILE_EXTENSIONS: &[&str] = &[
    "java",
    "xml",
    "txt",
    "dtd",
    "properties",
    "md",
    "rs",
    "toml",
    "csv",
    "plugin",
    "template",
    "jar",
    "html",
    "zip",
];

/// Java keywords and literals: as plain-word anchors they would match code
/// that has nothing to do with the attribute value of the same spelling.
const JAVA_WORDS: &[&str] = &[
    "abstract",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "continue",
    "default",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "true",
    "try",
    "void",
    "while",
    "string",
];

/// A token named in backticks next to a reference, lower-cased.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Anchor {
    text: String,
    /// A code-like identifier, matched as a substring (`storageType` in
    /// `getStorageType`). Otherwise a plain word such as an XML element or
    /// attribute name, matched as a whole word, which in Java also finds the
    /// `"word"` literal.
    code: bool,
    /// Java constants holding the word (`TAG_ECU = "ecu"`), matched as
    /// whole words too: the code uses the constant, not the literal.
    constants: Vec<String>,
}

fn whole_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(i, m)| {
        let before = line.get(..i).and_then(|b| b.chars().next_back());
        let after = line.get(i + m.len()..).and_then(|a| a.chars().next());
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

impl Anchor {
    fn matches(&self, line: &str) -> bool {
        if self.code || self.text.len() >= MIN_IDENT {
            // Also inside identifiers: `sizex` in `getSizeX`.
            line.contains(self.text.as_str()) || self.constants.iter().any(|c| whole_word(line, c))
        } else {
            whole_word(line, &self.text) || self.constants.iter().any(|c| whole_word(line, c))
        }
    }

    /// Whether a plain word is spelled in the file as itself: a `"word"`
    /// string literal (or in a constant), or anywhere in a non-Java file.
    /// A word that is only a local name (`csum`) is no anchor.
    fn is_literal_in(&self, lower: &[String]) -> bool {
        let quoted = format!("\"{}\"", self.text);
        !self.constants.is_empty() || lower.iter().any(|l| l.contains(&quoted))
    }

    /// Fills in the constants that hold this word in a Java file (`lower`
    /// is the file, lower-cased).
    fn with_constants(mut self, lower: &[String]) -> Self {
        if self.code {
            return self;
        }
        let literal = format!("= \"{}\"", self.text);
        for l in lower {
            if let Some(at) = l.find(&literal) {
                let name = l.get(..at).unwrap_or("").trim_end();
                let name: String = name
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if is_code_like(&name) || name.len() >= MIN_IDENT {
                    self.constants.push(name);
                }
            }
        }
        self
    }
}

/// The tokens inside backtick spans of `text[from..to]` (spans that are
/// references themselves give none): code-like identifiers, and plain words
/// of at least 3 letters.
fn identifiers(text: &str, from: usize, to: usize, refs: &[RefTok]) -> Vec<Anchor> {
    let mut out: Vec<Anchor> = Vec::new();
    let mut i = 0;
    while let Some(open) = text.get(i..).and_then(|r| r.find('`')) {
        let open = i + open;
        let Some(close) = text.get(open + 1..).and_then(|r| r.find('`')) else {
            break;
        };
        let close = open + 1 + close;
        i = close + 1;
        if open < from || close > to {
            continue;
        }
        if refs.iter().any(|r| r.start > open && r.start < close) {
            continue;
        }
        let span = text.get(open + 1..close).unwrap_or("");
        // A path names a file, not lines in it (`Table.setRealValue` is no path).
        let file_like = span
            .rsplit_once('.')
            .is_some_and(|(_, ext)| FILE_EXTENSIONS.contains(&ext));
        if span.contains('/') || file_like {
            continue;
        }
        for tok in span.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            let code = is_code_like(tok);
            let word = tok.len() >= 3
                && tok.chars().all(|c| c.is_ascii_alphabetic())
                && !JAVA_WORDS.contains(&tok.to_lowercase().as_str());
            let text = tok.to_lowercase();
            if (code || word) && !out.iter().any(|o| o.text == text) {
                out.push(Anchor {
                    text,
                    code,
                    constants: Vec::new(),
                });
            }
        }
    }
    out
}

/// `camelCase`, `PascalCase` with a second capital, `snake_case`, `ALLCAPS`.
fn is_code_like(tok: &str) -> bool {
    if tok.chars().count() < MIN_IDENT || tok.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    let upper_after_first = tok.chars().skip(1).any(|c| c.is_ascii_uppercase());
    let lower = tok.chars().any(|c| c.is_ascii_lowercase());
    let letters = tok.chars().filter(char::is_ascii_alphabetic).count();
    (upper_after_first && lower)
        || (tok.contains('_') && letters >= 2 && !tok.starts_with('_'))
        || (!lower && letters >= MIN_IDENT)
}

// ---------------------------------------------------------------------------
// Markdown blocks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Heading,
    Para,
    List,
    Table,
    Code,
}

/// Which units of a section are behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    All,
    RowsOnly,
    None,
}

#[derive(Debug, Clone)]
struct Unit {
    line: usize,
    text: String,
    indent: usize,
}

#[derive(Debug, Clone)]
struct Block {
    kind: Kind,
    units: Vec<Unit>,
    scope: Scope,
}

fn is_delimiter_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | ':' | '-' | ' '))
}

/// Splits a table row into cells, keeping `|` inside backticks and `\|`.
fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut tick = false;
    let mut prev = '\0';
    for c in t.chars() {
        if c == '`' {
            tick = !tick;
        }
        if c == '|' && !tick && prev != '\\' {
            cells.push(cur.trim().to_owned());
            cur.clear();
        } else {
            cur.push(c);
        }
        prev = c;
    }
    cells.push(cur.trim().to_owned());
    cells
}

/// `- x`, `* x`, `+ x`, `12. x` → `(indent, number if ordered, text after the marker)`.
fn list_marker(line: &str) -> Option<(usize, Option<u64>, &str)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some((indent, None, rest));
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let rest = t.get(digits..)?;
        if let Some(rest) = rest.strip_prefix(". ") {
            return Some((indent, t.get(..digits)?.parse().ok(), rest));
        }
    }
    None
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&level).then_some(())?;
    let title = line.get(level..)?.strip_prefix(' ')?;
    Some((level, title.trim()))
}

/// Heading text with its `N.` / `N.M` numbering removed.
fn unnumbered(title: &str) -> &str {
    let t = title.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    if t.len() == title.len() {
        title
    } else {
        t.trim_start()
    }
}

fn is_example_heading(title: &str) -> bool {
    let t = unnumbered(title).to_lowercase();
    t.starts_with("worked example") || t.starts_with("example") || t.contains("hand-composed")
}

fn is_example_para(text: &str) -> bool {
    let t = text.trim_start().trim_start_matches(['*', '_']);
    t.starts_with("Worked example") || t.starts_with("Example")
}

/// Splits a page into blocks, with the behaviour scope of each.
fn blocks(text: &str) -> (Vec<Block>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Block> = Vec::new();
    let mut section_numbers = Vec::new();
    let mut h2_scope = Scope::None;
    let mut scope = Scope::None;
    let mut cur: Option<Block> = None;
    let mut fence = false;
    let flush = |cur: &mut Option<Block>, out: &mut Vec<Block>| {
        if let Some(b) = cur.take() {
            out.push(b);
        }
    };
    for (idx, raw) in lines.iter().enumerate() {
        let n = idx + 1;
        let line = *raw;
        if line.trim_start().starts_with("```") {
            flush(&mut cur, &mut out);
            fence = !fence;
            if fence {
                cur = Some(Block {
                    kind: Kind::Code,
                    units: Vec::new(),
                    scope,
                });
            }
            continue;
        }
        if fence {
            if let Some(b) = cur.as_mut() {
                b.units.push(Unit {
                    line: n,
                    text: line.to_owned(),
                    indent: 0,
                });
            }
            continue;
        }
        if let Some((level, title)) = heading(line) {
            flush(&mut cur, &mut out);
            let num: String = title
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let num = num.trim_end_matches('.').to_owned();
            if !num.is_empty() {
                section_numbers.push(num);
            }
            let bare = unnumbered(title);
            if level == 1 {
                h2_scope = Scope::None;
                scope = Scope::None;
            } else if level == 2 {
                h2_scope = if EXCLUDED_SECTIONS.contains(&bare) || is_example_heading(title) {
                    Scope::None
                } else if bare == ROWS_ONLY_SECTION {
                    Scope::RowsOnly
                } else {
                    Scope::All
                };
                scope = h2_scope;
            } else {
                scope = if is_example_heading(title) {
                    Scope::None
                } else {
                    h2_scope
                };
            }
            out.push(Block {
                kind: Kind::Heading,
                units: vec![Unit {
                    line: n,
                    text: line.to_owned(),
                    indent: 0,
                }],
                scope: Scope::None,
            });
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut cur, &mut out);
            continue;
        }
        let table_row = line.trim_start().starts_with('|');
        let in_table = cur.as_ref().is_some_and(|b| b.kind == Kind::Table);
        if table_row && in_table {
            if !is_delimiter_row(line)
                && let Some(b) = cur.as_mut()
            {
                b.units.push(Unit {
                    line: n,
                    text: line.to_owned(),
                    indent: 0,
                });
            }
            continue;
        }
        if table_row && lines.get(idx + 1).is_some_and(|l| is_delimiter_row(l)) {
            flush(&mut cur, &mut out);
            cur = Some(Block {
                kind: Kind::Table,
                units: vec![Unit {
                    line: n,
                    text: line.to_owned(),
                    indent: 0,
                }],
                scope,
            });
            continue;
        }
        if in_table {
            flush(&mut cur, &mut out);
        }
        let in_para = cur.as_ref().is_some_and(|b| b.kind == Kind::Para);
        if let Some((indent, number, _)) = list_marker(line) {
            // An ordered marker other than 1 cannot interrupt a paragraph.
            let interrupts = !in_para || number.is_none_or(|k| k == 1);
            if interrupts {
                if cur.as_ref().is_none_or(|b| b.kind != Kind::List) {
                    flush(&mut cur, &mut out);
                    cur = Some(Block {
                        kind: Kind::List,
                        units: Vec::new(),
                        scope,
                    });
                }
                if let Some(b) = cur.as_mut() {
                    b.units.push(Unit {
                        line: n,
                        text: line.to_owned(),
                        indent,
                    });
                }
                continue;
            }
        }
        match cur.as_mut() {
            Some(b) if matches!(b.kind, Kind::Para | Kind::List) => {
                if let Some(u) = b.units.last_mut() {
                    u.text.push(' ');
                    u.text.push_str(line.trim());
                }
            }
            _ => {
                flush(&mut cur, &mut out);
                cur = Some(Block {
                    kind: Kind::Para,
                    units: vec![Unit {
                        line: n,
                        text: line.trim().to_owned(),
                        indent: 0,
                    }],
                    scope,
                });
            }
        }
    }
    flush(&mut cur, &mut out);
    (out, section_numbers)
}

/// A unit's text with any list marker removed.
fn unit_body(text: &str) -> &str {
    match list_marker(text) {
        Some((_, _, rest)) => rest.trim(),
        None => text.trim(),
    }
}

// ---------------------------------------------------------------------------
// Per page.

struct PageCtx<'a, 'c> {
    page: &'a str,
    config: &'a Config<'c>,
    legacy: &'a Legacy,
    problems: &'a mut Vec<String>,
    stats: PageStats,
    used_exempt: &'a mut BTreeSet<usize>,
    used_content: &'a mut BTreeSet<usize>,
}

impl<'c> PageCtx<'_, 'c> {
    fn at(&self, line: usize) -> String {
        format!("{DOCS_DIR}/{}:{line}", self.page)
    }

    fn aliases(&self) -> Vec<(&'c str, &'c str)> {
        self.config
            .aliases
            .iter()
            .filter(|(p, _, _)| *p == self.page)
            .map(|(_, a, t)| (*a, *t))
            .collect()
    }

    fn pages(&self) -> Vec<&'c str> {
        self.config.areas.iter().map(|(_, p, _)| *p).collect()
    }

    fn check(&mut self, text: &str) {
        let (blocks, numbers) = blocks(text);
        let aliases = self.aliases();
        // Rules 2 and 4 over every unit; rule 3 over behaviour units.
        let mut lead_file: Option<String> = None;
        for b in &blocks {
            // A `:N` in a list item or table row continues the last file of
            // the paragraph just above it (`Foo.java:72-114`: 1. ... (`:73`)).
            let inherited = match b.kind {
                Kind::List | Kind::Table => lead_file.clone(),
                _ => None,
            };
            let mut last = None;
            for u in &b.units {
                let refs = scan_refs(&u.text, &aliases);
                last = self.check_refs(u, &refs, b.kind, inherited.clone());
            }
            lead_file = if b.kind == Kind::Para { last } else { None };
        }
        self.check_coverage(&blocks, &numbers, &aliases);
    }

    /// Rules 2 and 4 for the references of one unit.
    /// Returns the unit's last file, for continuations below it.
    fn check_refs(
        &mut self,
        u: &Unit,
        refs: &[RefTok],
        kind: Kind,
        inherited: Option<String>,
    ) -> Option<String> {
        let mut last_file: Option<String> = inherited;
        for (k, r) in refs.iter().enumerate() {
            self.stats.refs += 1;
            for (a, b) in &r.ranges {
                // Bytecode offsets start at 0; lines at 1.
                let first = usize::from(r.kind != RefKind::Bytecode);
                if *a < first || b < a {
                    self.problems.push(format!(
                        "{}: `{}`: malformed line range {a}-{b}",
                        self.at(u.line),
                        r.text
                    ));
                }
            }
            let path = match &r.kind {
                RefKind::Bytecode => continue,
                RefKind::Continuation => match &last_file {
                    Some(p) => p.clone(),
                    None => {
                        self.problems.push(format!(
                            "{}: `{}` continues a reference, but no file reference precedes it in this paragraph, item or row",
                            self.at(u.line),
                            r.text
                        ));
                        continue;
                    }
                },
                RefKind::File(p) | RefKind::Alias(p) => p.clone(),
            };
            let resolved = self.legacy.resolve(&path, self.config.external);
            let rel = match resolved {
                Resolved::File { rel, legacy } => (rel, legacy),
                Resolved::External => {
                    last_file = None;
                    continue;
                }
                Resolved::Missing => {
                    self.problems.push(format!(
                        "{}: `{}`: no such file (looked for {JAVA_ROOT}/{path}, then any file under {LEGACY_DIR}/ ending in /{path})",
                        self.at(u.line),
                        r.text
                    ));
                    continue;
                }
                Resolved::Ambiguous(c) => {
                    self.problems.push(format!(
                        "{}: `{}`: ambiguous, {path} could be any of {}",
                        self.at(u.line),
                        r.text,
                        c.join(", ")
                    ));
                    continue;
                }
                Resolved::Glob => {
                    self.problems.push(format!(
                        "{}: `{}`: a glob cannot carry line numbers",
                        self.at(u.line),
                        r.text
                    ));
                    continue;
                }
            };
            let (rel, is_legacy) = rel;
            last_file = Some(rel.clone());
            let Some(lines) = self.legacy.lines(&rel) else {
                self.problems.push(format!(
                    "{}: `{}`: {rel} is unreadable",
                    self.at(u.line),
                    r.text
                ));
                continue;
            };
            self.stats.resolved += 1;
            let mut in_range = true;
            for (a, b) in &r.ranges {
                if *b > lines.len() {
                    in_range = false;
                    self.problems.push(format!(
                        "{}: `{}`: line {b} is past the end of {rel} ({} lines)",
                        self.at(u.line),
                        r.text,
                        lines.len()
                    ));
                } else if *a == 0 || b < a {
                    in_range = false;
                }
            }
            if in_range && is_legacy && kind != Kind::Code && kind != Kind::Heading {
                self.check_content(u, refs, k, &rel, &lines);
            }
        }
        last_file
    }

    /// Rule 4 for `refs[k]`, which resolved to `rel`.
    fn check_content(&mut self, u: &Unit, refs: &[RefTok], k: usize, rel: &str, lines: &[String]) {
        let Some(r) = refs.get(k) else { return };
        let text = &u.text;
        // Before the reference: back to the previous reference, `; ` or the
        // sentence or cell start. After it (the fallback): up to the next
        // reference, `, `, `; ` or the sentence or cell end.
        let lo = clause_start(text, r.start);
        let hi = clause_end(text, r.end);
        let prev_end = refs
            .iter()
            .take(k)
            .map(|p| p.end)
            .filter(|e| *e <= r.start)
            .max()
            .unwrap_or(0)
            .max(lo);
        let next_start = refs
            .iter()
            .skip(k + 1)
            .map(|p| p.start)
            .min()
            .unwrap_or(text.len())
            .min(hi);
        // Imports and the package line name every class; blank them out.
        let lower: Vec<String> = lines
            .iter()
            .map(|l| {
                let t = l.trim_start();
                if t.starts_with("import ") || t.starts_with("package ") {
                    String::new()
                } else {
                    l.to_lowercase()
                }
            })
            .collect();
        // The file's own class name says nothing about which lines are meant.
        let stem = rel
            .rsplit('/')
            .next()
            .and_then(|f| f.split('.').next())
            .unwrap_or("")
            .to_lowercase();
        let java = rel.ends_with(".java");
        // An anchor found on more than MAX_ANCHOR_LINES lines of the file
        // tells too little about which lines are meant to count.
        let in_file = |ids: Vec<Anchor>| -> Vec<Anchor> {
            ids.into_iter()
                .filter(|id| !stem.contains(id.text.as_str()))
                .map(|id| id.with_constants(&lower))
                .filter(|id| {
                    let n = lower.iter().filter(|l| id.matches(l)).count();
                    (1..=MAX_ANCHOR_LINES).contains(&n)
                        && (id.code || !java || id.is_literal_in(&lower))
                })
                .collect()
        };
        let mut anchors = in_file(identifiers(text, prev_end, r.start, refs));
        if anchors.is_empty() && next_start > r.end {
            anchors = in_file(identifiers(text, r.end, next_start, refs));
        }
        // In a table, then the row's first cell: a `Source` cell holds only
        // references, the first cell names the element, attribute or case.
        if anchors.is_empty() && text.trim_start().starts_with('|') {
            let first = boundaries(text, b"").nth(1).unwrap_or(0);
            if first < r.start {
                anchors = in_file(identifiers(text, 0, first, refs));
            }
        }
        if anchors.is_empty() {
            return;
        }
        self.stats.content_checked += 1;
        let mentions = |n: usize| {
            lower
                .get(n.wrapping_sub(1))
                .is_some_and(|l| anchors.iter().any(|id| id.matches(l)))
        };
        let hit = r.ranges.iter().any(|(a, b)| {
            let from = a.saturating_sub(SLACK).max(1);
            let to = (b + SLACK).min(lines.len());
            // The cited lines, or the declaration they sit in (a method
            // named in the text, its body cited).
            (from..=to).any(mentions) || enclosing_declaration(lines, *a).is_some_and(mentions)
        });
        if hit {
            return;
        }
        if let Some(i) = self
            .config
            .content_exempt
            .iter()
            .position(|(p, t, _)| *p == self.page && *t == r.text)
        {
            self.used_content.insert(i);
            return;
        }
        let found: Vec<String> = anchors
            .iter()
            .map(|id| {
                let at: Vec<String> = lower
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| id.matches(l))
                    .map(|(n, _)| (n + 1).to_string())
                    .take(6)
                    .collect();
                format!("`{}` at {}", id.text, at.join(","))
            })
            .collect();
        self.problems.push(format!(
            "{}: `{}`: the cited lines of {rel} do not mention what the text names next to it ({})",
            self.at(u.line),
            r.text,
            found.join("; ")
        ));
    }

    /// Rule 3.
    fn check_coverage(&mut self, blocks: &[Block], numbers: &[String], aliases: &[(&str, &str)]) {
        let pages = self.pages();
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
            let prev = k.checked_sub(1).and_then(|p| blocks.get(p));
            let lead = prev.filter(|p| {
                p.kind == Kind::Para
                    && p.units
                        .first()
                        .is_some_and(|u| u.text.trim_end().ends_with(':'))
            });
            let lead_ref = lead
                .and_then(|p| p.units.first())
                .is_some_and(|u| self.referenced(u, numbers, aliases, &pages));
            match b.kind {
                Kind::Para => {
                    let Some(u) = b.units.first() else { continue };
                    if is_example_para(&u.text) {
                        in_example = true;
                        continue;
                    }
                    self.stats.units += 1;
                    let next = blocks.get(k + 1).map(|n| n.kind);
                    let lead_in = u.text.trim_end().ends_with(':')
                        && matches!(next, Some(Kind::List | Kind::Table | Kind::Code));
                    if lead_in || self.referenced(u, numbers, aliases, &pages) {
                        self.stats.referenced_units += 1;
                    } else {
                        self.unreferenced(u, "paragraph");
                    }
                }
                Kind::List => {
                    if in_example {
                        continue;
                    }
                    let mut parents: Vec<(usize, bool)> = Vec::new();
                    for u in &b.units {
                        while parents.last().is_some_and(|(ind, _)| *ind >= u.indent) {
                            parents.pop();
                        }
                        let own = self.referenced(u, numbers, aliases, &pages);
                        let inherited = parents.last().map_or(lead_ref, |(_, r)| *r);
                        self.stats.units += 1;
                        if own || inherited {
                            self.stats.referenced_units += 1;
                        } else {
                            self.unreferenced(u, "list item");
                        }
                        let passes_down =
                            (own || inherited) && unit_body(&u.text).trim_end().ends_with(':');
                        parents.push((u.indent, passes_down));
                    }
                }
                Kind::Table => {
                    if in_example {
                        continue;
                    }
                    let Some((head, body)) = b.units.split_first() else {
                        continue;
                    };
                    let header = split_cells(&head.text);
                    let source_col = header
                        .iter()
                        .position(|c| c.to_lowercase().starts_with("source"));
                    let section_col = header
                        .iter()
                        .position(|c| c.eq_ignore_ascii_case("section"));
                    // A `Sources:` paragraph right below covers the rows too.
                    let trailer_ref = blocks.get(k + 1).is_some_and(|n| {
                        n.kind == Kind::Para
                            && n.units.first().is_some_and(|u| {
                                u.text.starts_with("Source")
                                    && self.referenced(u, numbers, aliases, &pages)
                            })
                    });
                    let covered = lead_ref || trailer_ref;
                    for u in body {
                        self.stats.units += 1;
                        let ok = match source_col {
                            Some(col) => {
                                let cells = split_cells(&u.text);
                                let cell = Unit {
                                    line: u.line,
                                    text: cells.get(col).cloned().unwrap_or_default(),
                                    indent: 0,
                                };
                                self.referenced(&cell, numbers, aliases, &pages)
                            }
                            None => {
                                // A `Section` column cell naming a section of this page points there.
                                let section_cell = section_col
                                    .and_then(|c| split_cells(&u.text).get(c).cloned())
                                    .is_some_and(|c| numbers.contains(&c));
                                covered
                                    || section_cell
                                    || self.referenced(u, numbers, aliases, &pages)
                            }
                        };
                        if ok {
                            self.stats.referenced_units += 1;
                        } else if source_col.is_some() {
                            self.unreferenced(u, "table row (its Source cell)");
                        } else {
                            self.unreferenced(u, "table row");
                        }
                    }
                }
                Kind::Heading | Kind::Code => {}
            }
        }
    }

    /// Whether a unit carries a legacy reference or a section pointer; flags
    /// pointers to sections this page does not have.
    fn referenced(
        &mut self,
        u: &Unit,
        numbers: &[String],
        aliases: &[(&str, &str)],
        pages: &[&str],
    ) -> bool {
        let refs = scan_refs(&u.text, aliases);
        let legacy_ref = refs.iter().any(|r| match &r.kind {
            RefKind::Bytecode | RefKind::Continuation | RefKind::Alias(_) => true,
            RefKind::File(p) => match self.legacy.resolve(p, self.config.external) {
                Resolved::File { legacy, .. } => legacy,
                Resolved::External => true,
                _ => false,
            },
        });
        let (nums, names_page) = section_pointers(&u.text, pages);
        if !names_page {
            for n in &nums {
                if !numbers.iter().any(|h| h == n) {
                    self.problems.push(format!(
                        "{}: points to section {n}, which this page does not have",
                        self.at(u.line)
                    ));
                }
            }
        }
        legacy_ref || !nums.is_empty() || names_page
    }

    fn unreferenced(&mut self, u: &Unit, what: &str) {
        let body = unit_body(&u.text);
        if let Some(i) = self
            .config
            .exempt_units
            .iter()
            .position(|(p, s, _)| *p == self.page && body.starts_with(s))
        {
            self.used_exempt.insert(i);
            self.stats.referenced_units += 1;
            return;
        }
        let shown: String = body.chars().take(100).collect();
        self.problems.push(format!(
            "{}: {what} describes behaviour with no legacy file:line reference (nor a lead-in, section pointer or EXEMPT_UNITS entry): {shown:?}",
            self.at(u.line)
        ));
    }
}

/// Clause boundaries outside backticks: a table cell edge, or `sep`
/// followed by a space.
fn boundaries<'t>(text: &'t str, seps: &'t [u8]) -> impl Iterator<Item = usize> + 't {
    let s = text.as_bytes();
    let mut tick = false;
    (0..s.len()).filter(move |&i| {
        let c = s.get(i).copied().unwrap_or(0);
        if c == b'`' {
            tick = !tick;
            return false;
        }
        !tick
            && ((c == b'|' && (i == 0 || s.get(i - 1) != Some(&b'\\')))
                || (seps.contains(&c) && s.get(i + 1) == Some(&b' ')))
    })
}

/// Start of the clause holding `pos`: after the last `|`, `. ` or `; ` before it.
fn clause_start(text: &str, pos: usize) -> usize {
    boundaries(text, b".;")
        .take_while(|i| *i < pos)
        .last()
        .map_or(0, |i| i + 1)
}

/// End of the clause after `pos`: the first `|`, `. `, `; ` or `, ` at or after it.
fn clause_end(text: &str, pos: usize) -> usize {
    boundaries(text, b".;,")
        .find(|i| *i >= pos)
        .unwrap_or(text.len())
}

/// The 1-based line of the method or class declaration that line `n` sits
/// in (the nearest declaration-shaped line at or above it), if any within
/// 400 lines.
fn enclosing_declaration(lines: &[String], n: usize) -> Option<usize> {
    const CONTROL: &[&str] = &[
        "if",
        "for",
        "while",
        "switch",
        "catch",
        "return",
        "else",
        "try",
        "do",
        "new",
        "throw",
        "case",
        "synchronized(",
        "super",
        "this",
    ];
    (n.saturating_sub(400).max(1)..=n).rev().find(|&i| {
        let Some(line) = lines.get(i - 1) else {
            return false;
        };
        let t = line.trim();
        if t.ends_with(';') || t.starts_with("//") || t.starts_with('*') || t.starts_with('@') {
            return false;
        }
        let Some(paren) = t.find('(').or_else(|| t.find(" class ").map(|c| c + 1)) else {
            return t.starts_with("class ");
        };
        let head = t.get(..paren).unwrap_or("").trim();
        let words: Vec<&str> = head.split_whitespace().collect();
        words.len() >= 2
            && !CONTROL.contains(words.first().unwrap_or(&""))
            && words.iter().all(|w| {
                w.chars()
                    .all(|c| c.is_alphanumeric() || "_<>[],.?$".contains(c))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_reference_forms() {
        let refs = scan_refs(
            "a (`x/Foo.java:12-15,20`), `:30`, `Unmarshaller:7`, `JEP.<init>@24-26`, 10:30, `legacy/docs/a.txt:5`",
            &[("Unmarshaller", "xml/U.java")],
        );
        let kinds: Vec<_> = refs
            .iter()
            .map(|r| (r.kind.clone(), r.ranges.clone()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (RefKind::File("x/Foo.java".into()), vec![(12, 15), (20, 20)]),
                (RefKind::Continuation, vec![(30, 30)]),
                (RefKind::Alias("xml/U.java".into()), vec![(7, 7)]),
                (RefKind::Bytecode, vec![(24, 26)]),
                (RefKind::File("legacy/docs/a.txt".into()), vec![(5, 5)]),
            ]
        );
    }

    #[test]
    fn code_like_identifiers() {
        for yes in [
            "getNamedItem",
            "DataCell",
            "MOVI20S",
            "max_value",
            "SSMResponseProcessor",
        ] {
            assert!(is_code_like(yes), "{yes}");
        }
        for no in ["Table", "endian", "true", "NaN", "0x10", "swapxy", "_x"] {
            assert!(!is_code_like(no), "{no}");
        }
    }

    #[test]
    fn section_pointer_forms() {
        let (n, p) = section_pointers("see section 4.1 and (1.6); sections 2 and 3", &[]);
        assert_eq!(n, vec!["4.1", "2", "1.6"]);
        assert!(!p);
    }

    #[test]
    fn paragraph_continuation_and_tables() {
        let (b, nums) = blocks(
            "## 1. A\n\nThe right answer is\n21. For x.\n\nIntro:\n\n| a | Source |\n|---|---|\n| x | y |\nnot | a table\n",
        );
        let kinds: Vec<_> = b.iter().map(|b| (b.kind, b.units.len())).collect();
        assert_eq!(
            kinds,
            vec![
                (Kind::Heading, 1),
                (Kind::Para, 1),
                (Kind::Para, 1),
                (Kind::Table, 2),
                (Kind::Para, 1)
            ]
        );
        assert_eq!(nums, vec!["1"]);
    }
}
