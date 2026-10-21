//! Body of the `romraider_defs` fuzz target (task US-RR-29-9): untrusted
//! bytes, as a RomRaider ECU-definition file downloaded from a forum would
//! supply them, through the hardened XML reader, the RomRaider reader
//! (`rr_defs::romraider::read`) and a one-file [`DefinitionSet`] whose every
//! `<rom>` is resolved (story US-RR-29, "the reader never panics on any
//! fuzzed definition input").
//!
//! # Input
//!
//! Two modes, chosen by the first byte:
//!
//! * **raw** (anything but `0x00`): the whole input is one definition file.
//!   Seeds are small hand-written definitions covering every table kind,
//!   static axes, switch states and bits, scalings with `expression` and
//!   `to_byte`, `<scalingbase>`, a `base` chain with `omit`, and unknown
//!   elements and attributes;
//! * **built** (`0x00`, then choices): the rest of the input drives
//!   [`build`], which writes a well-formed definition from the format's own
//!   element and attribute names and a vocabulary of good and bad values
//!   (with, now and then, a raw value taken from the input), so that a few
//!   bytes reach the table, scaling and resolve paths that random XML
//!   rarely gets past. (A raw input starting with `0x00` is malformed XML,
//!   which the `xml` target covers.)
//!
//! # Limits
//!
//! The input is capped at [`MAX_INPUT_LEN`] (32 KiB): longer inputs return
//! at once. Resolving is the expensive step: each distinct (scaling,
//! encoding) with a `to_byte` costs a [`to_raw_mismatch`] check of up to
//! 65 536 raw values, three formula evaluations each, and a formula may have
//! up to 1 024 nodes. So a file is resolved only when [`Cost::fits`]: at
//! most [`MAX_ROMS`] `<rom>`s and [`MAX_TABLES`] `<table>`s, and, if any
//! `to_byte` is given, at most [`MAX_TO_BYTE_CHECKS`] checks (bounded by
//! `<rom>`s times `<table>`s) of formulas no longer than
//! [`MAX_FORMULA_LEN`] bytes. A file over the cost is still read, checked
//! and loaded into a set, just not resolved, with one exception: a file
//! over the cost only because of its `to_byte`s (more checks, or longer
//! formulas, than the `to_byte` caps allow, within the `<rom>` and
//! `<table>` caps) is checked a second time with every `to_byte` renamed
//! `tx_byte` ([`without_to_byte`]: same length, so every span still
//! holds; an unknown attribute, so a Warning), which resolves it in full,
//! the `to_byte` check apart. So no resolve path but the check itself is
//! closed to a file for having a `to_byte`. Under these caps the slowest
//! inputs written to hit them, built with the fuzzer's sanitizer, run in
//! about 2 s (`-timeout` is 10 s): 16 tables of distinct 32-bit encodings
//! each with a 31-byte `to_byte` that agrees with its `expression`
//! everywhere (65 536 raw values checked each) in 2.0 s, 64 `<rom>`s
//! chained on one of 390 tables in 1.8 s at about 320 MiB peak RSS
//! (sanitizer quarantine included; no single allocation near
//! `-malloc_limit_mb`).
//!
//! [`to_raw_mismatch`]: rr_defs::romraider::to_raw_mismatch
//!
//! # What must hold
//!
//! * Every diagnostic (read, set, resolve) has a registered code with its
//!   registered severity, names the source, and has a `line:col` inside the
//!   input.
//! * `read` reports the XML reader's diagnostics first, unchanged, then its
//!   own (none of them `XML`); [`read_document`] on the parsed tree gives
//!   the same. The file is there exactly when the XML has no Error and the
//!   root is `<roms>` or `<rom>`; when the root is neither, `DEF0002` is
//!   the reader's only diagnostic. [`Loaded::value`] is there exactly when
//!   no diagnostic is an Error.
//! * Every `<rom>` read has a non-empty `xmlid` and spans inside the input.
//! * A one-file set never has an Error (only `DEF` warnings for a
//!   repeated `xmlid`); [`DefinitionSet::read_all`] gives the same
//!   diagnostics as `read` then [`DefinitionSet::new`]; each distinct
//!   `xmlid` (any case) is one resolvable `<rom>`, the first of its name,
//!   which [`DefinitionSet::find`] finds; `identify` on any bytes (the input
//!   itself) does not panic and names a resolvable `<rom>`.
//! * Resolving each `<rom>`: no value means an Error; a value's lineage
//!   ends at the `<rom>` itself, every layer is in the set's registry and
//!   from the source; each table is listed once, under its key, apart from
//!   the unplaced ones, with as many axes as its kind has, cells unless it
//!   is a static 1D table, and an [`explain`](rr_core::layer::Lineage::explain)
//!   of its storage that agrees with the merged document (when no table
//!   `base` changed it). Resolving again with the same resolver (its caches
//!   warm) gives the same value and diagnostics.

use std::fmt::Write as _;

use rr_core::diag::{Diagnostic, Loaded, Span, codes};
use rr_core::layer::Field;
use rr_defs::romraider::{
    self, CellsDoc, DefinitionFile, DefinitionSet, ResolvedRom, TableDoc, TableKind, read_document,
    table_key, table_path, xmlid_key,
};
use rr_defs::xml::{self, Document, Element};

/// The `source` every read reports.
const SOURCE: &str = "fuzz.xml";

/// The longest input read: see the module docs.
pub const MAX_INPUT_LEN: usize = 32 << 10;

/// The most `<rom>` elements a resolved file may have.
pub const MAX_ROMS: usize = 64;

/// The most `<table>` elements (axes included) a resolved file may have.
pub const MAX_TABLES: usize = 512;

/// The most `to_byte` checks (`<rom>`s times `<table>`s) a resolved file
/// with a `to_byte` may need.
pub const MAX_TO_BYTE_CHECKS: usize = 16;

/// The longest `expression` or `to_byte` (bytes) a resolved file with a
/// `to_byte` may have.
pub const MAX_FORMULA_LEN: usize = 32;

/// The `romraider_defs` target: see the module docs.
pub fn romraider_defs(data: &[u8]) {
    if data.len() > MAX_INPUT_LEN {
        return;
    }
    match data.split_first() {
        Some((0, choices)) => check(build(choices).as_bytes(), true),
        _ => check(data, true),
    }
}

/// Runs every check on one definition file; `retry`: and again on
/// [`without_to_byte`] of it when only its `to_byte`s keep it from being
/// resolved (see the module docs).
fn check(input: &[u8], retry: bool) {
    let lines = Lines::new(input);
    let parsed = xml::parse(SOURCE, input);
    let xml_diags = parsed.diagnostics().to_vec();
    let loaded = romraider::read(SOURCE, input);
    check_loaded(&loaded, &lines);
    let diags = loaded.diagnostics().to_vec();
    assert!(
        diags.starts_with(&xml_diags),
        "read does not report the XML diagnostics first: {diags:#?}"
    );
    let own = &diags[xml_diags.len()..];
    assert!(
        own.iter().all(|d| !d.code().as_str().starts_with("XML")),
        "{own:#?}"
    );
    let (file, _) = loaded.accept_errors();
    let (document, _) = parsed.accept_errors();
    let Some(document) = document else {
        assert!(file.is_none(), "a file from XML with an Error");
        assert!(own.is_empty(), "{own:#?}");
        return;
    };
    let again = read_document(SOURCE, &document);
    assert_eq!(again.diagnostics(), own, "read_document differs from read");
    let (again, _) = again.accept_errors();
    assert_eq!(again, file, "read_document differs from read");
    let not_roms = own.iter().any(|d| d.code() == codes::NOT_ROM_DEFINITION);
    assert_eq!(file.is_none(), not_roms, "{own:#?}");
    if not_roms {
        assert_eq!(own.len(), 1, "{own:#?}");
    }
    let Some(file) = file else { return };
    check_file(&file, &lines);
    let cost = Cost::of(&document);
    check_set(file, input, &diags, cost, &lines);
    if retry && cost.fits_without_to_byte() {
        check(&without_to_byte(input), false);
    }
}

/// `input` with every `to_byte` (any ASCII case) renamed `tx_byte`, byte
/// for byte the same length (see the module docs).
#[must_use]
pub fn without_to_byte(input: &[u8]) -> Vec<u8> {
    const NAME: &[u8] = b"to_byte";
    let mut out = input.to_vec();
    let mut i = 0;
    while i + NAME.len() <= out.len() {
        if out[i..i + NAME.len()].eq_ignore_ascii_case(NAME) {
            out[i + 1] = b'x';
            i += NAME.len();
        } else {
            i += 1;
        }
    }
    out
}

fn check_loaded<T>(loaded: &Loaded<T>, lines: &Lines) {
    let diags = loaded.diagnostics();
    for d in diags {
        check_diagnostic(d, lines);
    }
    assert_eq!(
        loaded.value().is_some(),
        !diags.iter().any(Diagnostic::is_error)
    );
}

fn check_diagnostic(d: &Diagnostic, lines: &Lines) {
    let info = codes::lookup(d.code()).unwrap_or_else(|| panic!("unregistered code {d}"));
    assert_eq!(info.severity, d.severity(), "{d}");
    assert!(!d.message().is_empty(), "{d}");
    assert_eq!(d.provenance().source(), SOURCE, "{d}");
    lines.check(d.provenance().span(), &d.to_string());
}

fn check_file(file: &DefinitionFile, lines: &Lines) {
    assert_eq!(file.source(), SOURCE);
    for rom in file.roms() {
        assert!(!rom.xmlid().is_empty());
        assert_eq!(rom.source(), SOURCE);
        lines.check(rom.span(), rom.xmlid());
        if let Some(base) = rom.base() {
            lines.check(base.span(), base.name());
        }
        for (key, _) in rom.doc().tables.entries() {
            assert_eq!(&table_key(key), key, "table key not folded");
            let span = rom
                .table_span(key)
                .unwrap_or_else(|| panic!("no span for table {key:?}"));
            lines.check(span, key);
        }
        assert_eq!(rom.layer().source(), SOURCE);
    }
    for (key, _) in file.scaling_bases().entries() {
        let span = file
            .scaling_base_span(key)
            .unwrap_or_else(|| panic!("no span for scalingbase {key:?}"));
        lines.check(span, key);
    }
}

fn check_set(
    file: DefinitionFile,
    input: &[u8],
    read_diags: &[Diagnostic],
    cost: Cost,
    lines: &Lines,
) {
    let names: Vec<String> = file.roms().iter().map(|r| r.xmlid().to_owned()).collect();
    let set = DefinitionSet::new([file]);
    check_loaded(&set, lines);
    assert!(!set.has_errors(), "{:#?}", set.diagnostics());
    assert!(
        set.diagnostics()
            .iter()
            .all(|d| d.code() == codes::DUPLICATE_XMLID),
        "{:#?}",
        set.diagnostics()
    );
    let all = DefinitionSet::read_all([(SOURCE, input)]);
    let want: Vec<Diagnostic> = read_diags
        .iter()
        .chain(set.diagnostics())
        .cloned()
        .collect();
    assert_eq!(all.diagnostics(), want, "read_all differs from read + new");
    let (set, _) = set.accept_errors();
    let mut keys: Vec<String> = names.iter().map(|n| xmlid_key(n)).collect();
    keys.sort();
    keys.dedup();
    let roms: Vec<_> = set.roms().map(|(r, _)| r).collect();
    assert_eq!(roms.len(), keys.len(), "one resolvable <rom> per xmlid");
    for &r in &roms {
        let entry = set.rom(r).expect("listed rom");
        let found = set.find(entry.xmlid()).expect("find");
        assert_eq!(found, r, "find gives another <rom> of {:?}", entry.xmlid());
        assert!(set.id(r).is_some());
    }
    if let Some(r) = set.identify(input) {
        assert!(roms.contains(&r));
    }
    if !cost.fits() {
        return;
    }
    let mut resolver = set.resolver();
    if input.len() % 2 == 1 {
        resolver = resolver.with_preferred_scaling("Metric");
    }
    for &r in &roms {
        let first = resolver.resolve(r);
        check_loaded(&first, lines);
        let second = resolver.resolve(r);
        assert_eq!(first, second, "resolving twice differs");
        let (resolved, diags) = first.accept_errors();
        match resolved {
            None => assert!(diags.iter().any(Diagnostic::is_error), "{diags:#?}"),
            Some(rom) => check_resolved(&set, r, &rom),
        }
    }
}

fn check_resolved(set: &DefinitionSet, r: romraider::RomRef, rom: &ResolvedRom) {
    assert_eq!(rom.rom(), r);
    let names = rom.lineage().names();
    let own = names.last().expect("a lineage has its own layer");
    assert_eq!(Some(own.id()), set.id(r));
    for name in names {
        assert!(set.registry().get(name).is_some(), "{name:?}");
    }
    for (layer, _) in rom.lineage().stack().layers() {
        assert_eq!(layer.source(), SOURCE);
    }
    let merged = &rom.doc().tables;
    let present: Vec<&String> = merged.present().map(|(k, _)| k).collect();
    let mut seen: Vec<&str> = Vec::new();
    for t in rom.tables() {
        assert!(!seen.contains(&t.key.as_str()), "table {} twice", t.key);
        seen.push(&t.key);
        assert!(
            present.contains(&&t.key),
            "table {} not in the merged doc",
            t.key
        );
        assert_eq!(table_key(&t.name), t.key);
        assert_eq!(rom.table(&t.name).map(|x| &x.key), Some(&t.key));
        assert_eq!(t.at.source(), SOURCE);
        let axes = match t.kind {
            TableKind::TwoD => 1,
            TableKind::ThreeD => 2,
            TableKind::OneD | TableKind::Switch | TableKind::BitwiseSwitch => 0,
        };
        assert_eq!(t.axes.len(), axes, "{}", t.key);
        let is_static = t.kind == TableKind::OneD && t.doc.cells.static_values.get().is_some();
        assert_eq!(t.cells.is_none(), is_static, "{}", t.key);
        for axis in &t.axes {
            assert_eq!(
                axis.cells.is_none(),
                axis.doc.cells.static_values.get().is_some()
            );
        }
    }
    for name in rom.unplaced() {
        let key = table_key(name);
        assert!(
            !seen.contains(&key.as_str()),
            "{key} both placed and unplaced"
        );
        assert!(
            present.contains(&&key),
            "unplaced {key} not in the merged doc"
        );
    }
    assert!(rom.tables().len() + rom.unplaced().len() <= present.len());
    // Provenance agrees with the merge, for a table no table `base` changed.
    for (key, table) in merged.present().take(8) {
        let path = table_path(key)
            .field("cells", |t: &TableDoc| &t.cells)
            .field("storage", |c: &CellsDoc| &c.storage);
        let explained = rom.lineage().explain(&path);
        let _ = explained.to_string();
        if matches!(table.base, Field::Unset) {
            assert_eq!(explained.value(), table.cells.storage.get(), "{key}");
        }
    }
}

/// What resolving a file may cost, counted on its XML tree before it is
/// resolved (see the module docs).
#[derive(Debug, Default, Clone, Copy)]
struct Cost {
    roms: usize,
    tables: usize,
    to_byte: bool,
    longest_formula: usize,
}

impl Cost {
    fn of(document: &Document) -> Self {
        let mut cost = Self::default();
        cost.add(document.root());
        cost
    }

    fn add(&mut self, e: &Element) {
        if e.name().eq_ignore_ascii_case("rom") {
            self.roms += 1;
        } else if e.name().eq_ignore_ascii_case("table") {
            self.tables += 1;
        }
        for a in e.attributes() {
            let name = a.name();
            if name.eq_ignore_ascii_case("to_byte") || name.eq_ignore_ascii_case("expression") {
                self.longest_formula = self.longest_formula.max(a.value().len());
            }
            if name.eq_ignore_ascii_case("to_byte") && !a.value().trim().is_empty() {
                self.to_byte = true;
            }
        }
        for child in e.elements() {
            self.add(child);
        }
    }

    /// Over the cost only because of a `to_byte`: it would fit without.
    const fn fits_without_to_byte(self) -> bool {
        self.to_byte
            && !self.fits()
            && Self {
                to_byte: false,
                ..self
            }
            .fits()
    }

    const fn fits(self) -> bool {
        self.roms <= MAX_ROMS
            && self.tables <= MAX_TABLES
            && (!self.to_byte
                || (self.roms.saturating_mul(self.tables) <= MAX_TO_BYTE_CHECKS
                    && self.longest_formula <= MAX_FORMULA_LEN))
    }
}

/// Line lengths of the raw input, in bytes (as in the `xml` target): lines
/// end at LF, CRLF or a lone CR, and a column is never past `len + 1`.
struct Lines(Vec<usize>);

impl Lines {
    fn new(data: &[u8]) -> Self {
        let mut lens = vec![0];
        let mut i = 0;
        while i < data.len() {
            match data[i] {
                b'\r' => {
                    if data.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    lens.push(0);
                }
                b'\n' => lens.push(0),
                _ => {
                    if let Some(last) = lens.last_mut() {
                        *last += 1;
                    }
                }
            }
            i += 1;
        }
        Self(lens)
    }

    fn check(&self, span: Span, what: &str) {
        let Span::LineCol { line, col } = span else {
            panic!("not a line:col span: {what}");
        };
        assert!(line >= 1 && col >= 1, "{what}");
        let len = self
            .0
            .get(line as usize - 1)
            .unwrap_or_else(|| panic!("line {line} past the end ({} lines): {what}", self.0.len()));
        assert!(
            col as usize <= len + 1,
            "column {col} past line {line} ({len} bytes): {what}"
        );
    }
}

// ---------------------------------------------------------------------------
// Built mode: a definition written from choices.
// ---------------------------------------------------------------------------

/// The choices of a built input, one byte at a time; `0` once used up, so
/// every list ends.
struct Choices<'a>(&'a [u8]);

impl Choices<'_> {
    fn byte(&mut self) -> u8 {
        match self.0.split_first() {
            Some((&b, rest)) => {
                self.0 = rest;
                b
            }
            None => 0,
        }
    }

    /// A number below `n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        usize::from(self.byte()) % n
    }

    fn pick<'v>(&mut self, values: &[&'v str]) -> &'v str {
        values[self.below(values.len())]
    }

    /// A value: usually one of `values`, now and then up to 15 raw bytes
    /// of the input.
    fn value(&mut self, values: &[&str]) -> String {
        if self.byte() >= 0xF0 {
            let len = self.below(16).min(self.0.len());
            let (raw, rest) = self.0.split_at(len);
            self.0 = rest;
            String::from_utf8_lossy(raw).into_owned()
        } else {
            self.pick(values).to_owned()
        }
    }

    /// An element name: `name` in its usual case, or upper-cased (element
    /// names compare ignoring ASCII case).
    fn name(&mut self, name: &str) -> String {
        if self.byte() >= 0xF8 {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        }
    }
}

/// Escapes `s` for an attribute value or text: no markup, no character
/// XML refuses.
fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

const XMLIDS: &[&str] = &["BASE", "base", "CHILD", "GRAND", "Ünï", "", "32BITBASE"];
const TABLE_NAMES: &[&str] = &["Boost", "boost", "Fuel Map", "Mode", "Flags", "Idle", ""];
const SCALING_BASES: &[&str] = &["psi", "PSI", "afr", "none", "missing"];
const TYPES: &[&str] = &[
    "1D",
    "2D",
    "3D",
    "Switch",
    "BitwiseSwitch",
    "X Axis",
    "Static Y Axis",
    "4",
    "bogus",
];
const AXIS_TYPES: &[&str] = &[
    "X Axis",
    "Y Axis",
    "Static X Axis",
    "Static Y Axis",
    "5",
    "3D",
];
const STORAGE: &[&str] = &[
    "uint8", "int8", "uint16", "int16", "uint24", "uint32", "int32", "uint64", "float", "movi20",
    "movi20s", "bloblist", "7",
];
const ENDIAN: &[&str] = &["big", "little", "LITTLE", "lsb", ""];
const ADDRESSES: &[&str] = &[
    "0x0",
    "0x10",
    "200",
    "0xFFFFFFFF",
    "0x100000000",
    "-1",
    "zz",
];
const SIZES: &[&str] = &["0", "1", "2", "4", "16", "65536", "4294967296", "x"];
const MASKS: &[&str] = &[
    "0F", "F0", "FF00", "80000000", "0", "1FF", "101", "0x0F", "x",
];
const LAYOUTS: &[&str] = &["default", "bosch_subtract", "BOSCH_SUBTRACT", "bogus"];
const BOOLS: &[&str] = &["true", "false", "TRUE", "1", ""];
const LEVELS: &[&str] = &["1", "5", "9", "x"];
const TEXT: &[&str] = &["c", "P8", "", " padded  "];
const EXPRESSIONS: &[&str] = &[
    "x",
    "x*0.5",
    "x/2+40",
    "(x-128)*0.1",
    "x^2",
    "1/x",
    "x*",
    "[e]",
    "sqrt(x)",
    "x*y",
    "14.7/(1+x*0.01)",
];
const TO_BYTES: &[&str] = &["x", "x*2", "(x+128)/0.1", "x/0", "", "(", "x-40", "x*10"];
const FORMATS: &[&str] = &["0.00", "#", "%d", "0.0#"];
const NUMBERS: &[&str] = &["0", "-40", "1e3", "NaN", "Infinity", ".01", "x"];
const CATEGORIES: &[&str] = &["Metric", "Imperial", "Default", "default"];
const STATE_NAMES: &[&str] = &["on", "off", "ON", ""];
const STATE_DATA: &[&str] = &["00", "01", "01 00", "FF FF FF FF", "zz", "100", ""];
const POSITIONS: &[&str] = &["0", "7", "15", "31", "32", "x"];
const ROMID_FIELDS: &[(&str, &[&str])] = &[
    ("internalidaddress", &["200", "0x10", "FFFFFFFF", "zz"]),
    ("internalidstring", &["fuzz", "0x00FF", "0xZZ", ""]),
    ("filesize", &["192kb", "1mb", "512", "0kb", "lots"]),
    ("memmodel", &["SH7058", ""]),
    ("noramoffset", &["", "x"]),
    ("offset", &["0x10", "-0x10", "zz"]),
    ("obsolete", &["1", "true", "0"]),
    ("make", &["Subaru"]),
    ("checksummodule", &["subarudbw"]),
    ("bogus", &["?"]),
];

/// Writes the definition the choices describe (see the module docs).
#[must_use]
pub fn build(choices: &[u8]) -> String {
    let mut c = Choices(choices);
    let mut out = String::from("<roms>\n");
    for _ in 0..c.below(3) {
        out.push_str("<scalingbase");
        let name = c.value(SCALING_BASES);
        let _ = write!(out, " name=\"{}\"", escaped(&name));
        scaling_attributes(&mut c, &mut out);
        out.push_str("/>\n");
    }
    for _ in 0..=c.below(4) {
        rom(&mut c, &mut out);
    }
    if c.byte() >= 0xF0 {
        out.push_str("<bogus/>stray text\n");
    }
    out.push_str("</roms>\n");
    out
}

fn rom(c: &mut Choices<'_>, out: &mut String) {
    let rom = c.name("rom");
    let _ = write!(out, "<{rom}");
    if c.byte() % 2 == 1 {
        let _ = write!(out, " base=\"{}\"", escaped(&c.value(XMLIDS)));
    }
    if c.byte() >= 0xF0 {
        out.push_str(" bogus=\"1\"");
    }
    out.push_str(">\n");
    if c.byte() < 0xF0 {
        let romid = c.name("romid");
        let xmlid = escaped(&c.value(XMLIDS));
        let _ = write!(out, "<{romid}><xmlid>{xmlid}</xmlid>");
        for _ in 0..c.below(4) {
            let (field, values) = ROMID_FIELDS[c.below(ROMID_FIELDS.len())];
            let value = escaped(&c.value(values));
            if field == "memmodel" && c.byte() % 2 == 1 {
                let endian = c.value(ENDIAN);
                let _ = write!(
                    out,
                    "<memmodel endian=\"{}\">{value}</memmodel>",
                    escaped(&endian)
                );
            } else {
                let _ = write!(out, "<{field}>{value}</{field}>");
            }
        }
        let _ = writeln!(out, "</{romid}>");
    }
    for _ in 0..c.below(6) {
        table(c, out, false);
    }
    match c.below(16) {
        0 => out.push_str("<checksum type=\"std\"/>\n"),
        1 => out.push_str("<unknown/>\n"),
        _ => {}
    }
    let _ = writeln!(out, "</{rom}>");
}

/// One `<table>`; `axis` for an axis child.
fn table(c: &mut Choices<'_>, out: &mut String, axis: bool) {
    let element = c.name("table");
    let _ = write!(out, "<{element}");
    let kind = if axis {
        c.value(AXIS_TYPES)
    } else {
        c.value(TYPES)
    };
    if c.byte() < 0xE0 {
        let _ = write!(out, " type=\"{}\"", escaped(&kind));
    }
    let name = c.value(TABLE_NAMES);
    let _ = write!(out, " name=\"{}\"", escaped(&name));
    let mut used: Vec<&str> = vec!["type", "name"];
    // Placed and stored is what gets a table (or axis) resolved: usually
    // say both, so that 2D and 3D tables with two typed axes are common.
    if c.byte() < 0xC0 {
        for (attr, values) in [("storageaddress", ADDRESSES), ("storagetype", STORAGE)] {
            used.push(attr);
            let value = c.value(values);
            let _ = write!(out, " {attr}=\"{}\"", escaped(&value));
        }
    }
    for _ in 0..c.below(8) {
        let (attr, values): (&str, &[&str]) = match c.below(22) {
            0 => ("storageaddress", ADDRESSES),
            1 => ("storagetype", STORAGE),
            2 => ("endian", ENDIAN),
            3 => ("mask", MASKS),
            4 => ("dataLayout", LAYOUTS),
            5 => ("beforeram", BOOLS),
            6 => ("sizex", SIZES),
            7 => ("sizey", SIZES),
            8 => ("flipx", BOOLS),
            9 => ("flipy", BOOLS),
            10 => ("swapxy", BOOLS),
            11 => ("skipCells", SIZES),
            12 => ("userlevel", LEVELS),
            13 => ("locked", BOOLS),
            14 => ("logparam", TEXT),
            15 => ("category", TEXT),
            16 => ("description", TEXT),
            17 => ("base", TABLE_NAMES),
            18 => ("omit", BOOLS),
            19 => ("bogus", TEXT),
            // Placement is what gets a table resolved: make it likely.
            _ => ("storageaddress", ADDRESSES),
        };
        if used.contains(&attr) {
            continue;
        }
        used.push(attr);
        let value = c.value(values);
        let _ = write!(out, " {attr}=\"{}\"", escaped(&value));
    }
    out.push_str(">\n");
    for _ in 0..c.below(5) {
        match c.below(9) {
            0 | 1 if !axis => table(c, out, true),
            2 | 3 => {
                out.push_str("<scaling");
                scaling_attributes(c, out);
                out.push_str("/>\n");
            }
            4 => {
                let name = escaped(&c.value(STATE_NAMES));
                let data = escaped(&c.value(STATE_DATA));
                let _ = writeln!(out, "<state name=\"{name}\" data=\"{data}\"/>");
            }
            5 => {
                let position = escaped(&c.value(POSITIONS));
                let name = escaped(&c.value(STATE_NAMES));
                let _ = writeln!(out, "<bit position=\"{position}\" name=\"{name}\"/>");
            }
            6 => {
                let data = escaped(&c.value(TEXT));
                let _ = writeln!(out, "<data>{data}</data>");
            }
            7 => out.push_str("<description>about</description>\n"),
            _ => out.push_str("<bogus/>\n"),
        }
    }
    let _ = writeln!(out, "</{element}>");
}

/// The attributes of a `<scaling>` or `<scalingbase>`, after its name.
fn scaling_attributes(c: &mut Choices<'_>, out: &mut String) {
    let mut used: Vec<&str> = Vec::new();
    for _ in 0..c.below(7) {
        let (attr, values): (&str, &[&str]) = match c.below(13) {
            0 | 1 => ("expression", EXPRESSIONS),
            2 | 3 => ("to_byte", TO_BYTES),
            4 => ("units", TEXT),
            5 => ("format", FORMATS),
            6 => ("min", NUMBERS),
            7 => ("max", NUMBERS),
            8 => ("increment", NUMBERS),
            9 => ("fineincrement", NUMBERS),
            10 => ("category", CATEGORIES),
            11 => ("base", SCALING_BASES),
            _ => ("storagetype", STORAGE),
        };
        if used.contains(&attr) {
            continue;
        }
        used.push(attr);
        let value = c.value(values);
        let _ = write!(out, " {attr}=\"{}\"", escaped(&value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(input: &[u8]) -> Cost {
        let (document, _) = xml::parse(SOURCE, input).accept_errors();
        Cost::of(&document.expect("well-formed"))
    }

    /// A file kept from resolving only by its `to_byte`s is resolved on the
    /// retry, which has none; one over the `<table>` cap is not retried.
    #[test]
    fn a_file_over_only_the_to_byte_cost_is_retried_without_it() {
        let tables = |n: usize| {
            let mut s = String::from("<roms><rom><romid><xmlid>A</xmlid></romid>\n");
            for i in 0..n {
                let _ = writeln!(
                    s,
                    "<table type=\"1D\" name=\"t{i}\" storageaddress=\"{i}\" \
                     storagetype=\"uint8\"><scaling expression=\"x+{i}\" \
                     TO_Byte=\"x-{i}\"/></table>"
                );
            }
            s.push_str("</rom></roms>\n");
            s.into_bytes()
        };
        let few = tables(MAX_TO_BYTE_CHECKS);
        assert!(cost(&few).fits());
        assert!(!cost(&few).fits_without_to_byte());
        let many = tables(MAX_TO_BYTE_CHECKS + 1);
        assert!(!cost(&many).fits());
        assert!(cost(&many).fits_without_to_byte());
        let renamed = without_to_byte(&many);
        assert_eq!(renamed.len(), many.len());
        assert!(!String::from_utf8_lossy(&renamed).contains("TO_Byte"));
        assert!(String::from_utf8_lossy(&renamed).contains("Tx_Byte"));
        assert!(!cost(&renamed).to_byte && cost(&renamed).fits());
        romraider_defs(&many);
        assert!(!cost(&tables(MAX_TABLES + 1)).fits_without_to_byte());
        // The seed that starts the fuzzer on the retry.
        let seed = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("seeds/romraider_defs/to-byte-over-cost.xml");
        let seed = std::fs::read(&seed).unwrap_or_else(|e| panic!("{seed:?}: {e}"));
        assert!(cost(&seed).fits_without_to_byte(), "{:?}", cost(&seed));
        assert!(cost(&seed).longest_formula > MAX_FORMULA_LEN);
    }
}
