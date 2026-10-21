//! Known-answer tests of the RomRaider ECU-definition reader (US-RR-29-2):
//! one hand-checked fixture per table kind (1D, 2D, 3D, Switch,
//! BitwiseSwitch), with every axis type (`X Axis`, `Y Axis`,
//! `Static X Axis`, `Static Y Axis`), and one `base` / `omit` inheritance
//! case. Each fixture is minimal definition XML in and the expected typed,
//! layered document out, written by hand.
//!
//! # The fixtures
//!
//! `fixtures/defs-kat/romraider/*.toml`, one file per case. The format is
//! documented in `fixtures/README.md` ("Definition-reader fixtures") and,
//! key by key, on the types below; in short, each file holds:
//!
//! * `xml`: the whole definition file, verbatim, in a TOML literal string
//!   (its line 1 is the line after the opening `'''`, so the `line:col` of
//!   a diagnostic is the XML's own);
//! * `resolve`: the `xmlid` of the `<rom>` resolved (with every `base` of
//!   its chain);
//! * `diagnostics`: every diagnostic reading and resolving gives, exactly
//!   (code, severity, `line:col`); none unless listed;
//! * `unplaced`, `omitted`: table names the merged document has but that are
//!   not on the ROM (no address), and those an `omit` removed;
//! * `[[table]]`: every resolved table, in order, exactly: its merged
//!   [`TableDoc`] field by field (a key left out is a field the XML does not
//!   set: [`Field::Unset`]), the provenance of its `<table>`, its axes, and
//!   the [`Encoding`] and [`Conversion`] built for its cells. A derived value
//!   (anything not copied from one attribute: the resolved encoding and
//!   conversion, a decoded cell) carries a `working` that says how it was
//!   worked out by hand, and each `[[table.cell]]` decodes hand-picked bytes
//!   with that encoding and conversion to a hand-calculated real value.
//!
//! Unknown keys are rejected (`deny_unknown_fields`), so a typo cannot drop
//! an expectation; [`rr_testkit::fixtures::load_kat`] checks `schema = 1`.
//! These are not schema-1 byte-decoding KATs (`fixtures/kat/`), whose
//! vocabulary is the reference codec's: they live in their own directory,
//! and this test, not `cargo xtask validate-fixtures`, validates them.
//!
//! # What is compared
//!
//! The expected values are compared with what the reader gives, field by
//! field and exhaustively: a field the reader sets that the fixture does not
//! list fails as surely as a wrong value. The axes are compared as resolved
//! ([`ResolvedTable::axes`]: the slot the kind uses, `axis` for a 2D table,
//! `x_axis` then `y_axis` for 3D); the other axis slots of the merged
//! document are the reader's working (`TableDoc` "Per-element fields legacy
//! read twice") and are not part of the answer.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod kat {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use rr_core::conversion::{Conversion, Form};
    use rr_core::diag::{Diagnostic, Span};
    use rr_core::encoding::{BitField, Encoding, Endian, Layout, Storage};
    use rr_core::layer::{Field, Nested};
    use rr_defs::romraider::{
        AxisDoc, CellFormat, CellsDoc, DefinitionSet, ResolvedAxis, ResolvedTable, ScalingDoc,
        TableKind, table_key,
    };
    use rr_testkit::fixtures::{self, Tier};
    use serde::Deserialize;

    /// The fixture directory, relative to the public tier (`<repo>/fixtures`).
    const DIR: &str = "defs-kat/romraider";

    // ---- the fixture format ------------------------------------------------------

    /// One fixture file (every top-level key but `schema`).
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fixture {
        /// One line: what the fixture pins down.
        title: String,
        /// Free text for reviewers.
        #[serde(default)]
        notes: Option<String>,
        /// The `xmlid` of the `<rom>` to resolve.
        resolve: String,
        /// The definition file, verbatim.
        xml: String,
        /// Every diagnostic, exactly.
        #[serde(default)]
        diagnostics: Vec<ExpectedDiagnostic>,
        /// The names of the tables in the merged document but not on the ROM.
        #[serde(default)]
        unplaced: Vec<String>,
        /// The names of the tables an `omit` removed from the merged document.
        #[serde(default)]
        omitted: Vec<String>,
        /// Every resolved table, in order.
        table: Vec<ExpectedTable>,
    }

    /// One expected diagnostic.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedDiagnostic {
        /// `DEF0005`.
        code: String,
        /// `warning` or `error`.
        severity: String,
        /// `line:col` in the XML.
        at: String,
    }

    /// One resolved table: the merged [`TableDoc`] fields (each `None` is
    /// [`Field::Unset`]), then what resolution built.
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedTable {
        /// The name, as the strongest layer wrote it.
        name: String,
        /// `1D`, `2D`, `3D`, `Switch` or `BitwiseSwitch`.
        kind: String,
        /// `line:col` of the strongest `<table>` element naming it.
        at: String,
        /// The layer (registry id) of that element's `<rom>`.
        layer: String,
        base: Option<String>,
        category: Option<String>,
        description: Option<String>,
        user_level: Option<u8>,
        locked: Option<bool>,
        log_param: Option<String>,
        size_x: Option<u32>,
        size_y: Option<u32>,
        flip_x: Option<bool>,
        flip_y: Option<bool>,
        swap_xy: Option<bool>,
        skip_cells: Option<u32>,
        #[serde(default)]
        cells: ExpectedCells,
        /// Switch states, in order.
        #[serde(default)]
        state: Vec<ExpectedState>,
        /// Bitwise-switch bits, in order.
        #[serde(default)]
        bit: Vec<ExpectedBit>,
        /// The resolved axes, in order.
        #[serde(default)]
        axis: Vec<ExpectedAxis>,
        /// The cells' encoding; absent for a static 1D table (no cells to read).
        encoding: Option<ExpectedEncoding>,
        /// The cells' conversion; absent exactly when `encoding` is.
        conversion: Option<ExpectedConversion>,
        /// Cells decoded with that encoding and conversion.
        #[serde(default)]
        cell: Vec<CellCheck>,
    }

    /// One resolved axis: the merged [`AxisDoc`], then what resolution built.
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedAxis {
        name: Option<String>,
        description: Option<String>,
        log_param: Option<String>,
        #[serde(default)]
        cells: ExpectedCells,
        /// Absent for a static axis.
        encoding: Option<ExpectedEncoding>,
        conversion: Option<ExpectedConversion>,
        #[serde(default)]
        cell: Vec<CellCheck>,
    }

    /// The merged [`CellsDoc`] of a table or axis; each `None` is
    /// [`Field::Unset`].
    #[derive(Debug, Clone, Default, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedCells {
        storage_address: Option<u32>,
        /// `u8` .. `f64` (the storage, not the attribute spelling).
        storage: Option<String>,
        /// `big` or `little`, as the attribute says (before `<memmodel>`).
        endian: Option<String>,
        bitfield: Option<ExpectedBitField>,
        /// `default` or `bosch-subtract`.
        layout: Option<String>,
        before_ram: Option<bool>,
        size: Option<u32>,
        flip: Option<bool>,
        static_values: Option<Vec<String>>,
        /// The `<scaling>`s, by key (the category lower-cased).
        #[serde(default)]
        scaling: BTreeMap<String, ExpectedScaling>,
    }

    /// A bitfield: `raw = (stored & mask) >> shift`.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedBitField {
        mask: u64,
        shift: u32,
        signed: bool,
    }

    /// One merged `<scaling>`; each `None` is [`Field::Unset`].
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedScaling {
        base: Option<String>,
        name: Option<String>,
        category: Option<String>,
        units: Option<String>,
        /// The `expression` text.
        expression: Option<String>,
        /// The `to_byte` text; `""` for an empty `to_byte` (no formula).
        to_byte: Option<String>,
        format: Option<String>,
        min: Option<f64>,
        max: Option<f64>,
        coarse_increment: Option<f64>,
        fine_increment: Option<f64>,
    }

    /// One `<state>`, under its key (the name lower-cased).
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedState {
        key: String,
        name: Option<String>,
        data: Option<Vec<u32>>,
    }

    /// One `<bit>`.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedBit {
        position: u8,
        name: String,
    }

    /// The resolved [`Encoding`] of a table's or axis's cells.
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedEncoding {
        storage: String,
        endian: String,
        #[serde(default)]
        bitfield: Option<ExpectedBitField>,
        layout: String,
        /// How each part follows from the XML (defaults, overrides).
        working: String,
    }

    /// The resolved [`Conversion`] of a table's or axis's cells.
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExpectedConversion {
        /// `linear` (no scaling: raw values) or `expr` (a scaling's formulas).
        form: String,
        /// `linear` only: `m` and `b` of `m * x + b`.
        m: Option<f64>,
        b: Option<f64>,
        /// `expr` only: whether a `to_byte` formula was kept.
        #[serde(default)]
        to_raw: bool,
        unit: Option<String>,
        format: Option<String>,
        working: String,
    }

    /// One cell, decoded: `hex` (the cell's bytes as stored) through the
    /// encoding to a raw value, and through the conversion to `real`.
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CellCheck {
        hex: String,
        real: f64,
        working: String,
    }

    // ---- loading and validating --------------------------------------------------

    /// Every fixture file, sorted.
    fn fixture_paths() -> Vec<PathBuf> {
        let dir = fixtures::fixture_root(Tier::Public)
            .expect("the public tier is always there")
            .join(DIR);
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .map(|e| e.expect("a directory entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no fixtures in {}", dir.display());
        paths
    }

    /// The fixture at `path`, loaded and checked for the rules serde cannot
    /// express.
    fn load(path: &Path) -> Fixture {
        let fixture = fixtures::load_kat::<Fixture>(path)
            .unwrap_or_else(|e| panic!("{e}"))
            .data;
        if let Err(problem) = check(&fixture) {
            panic!("{}: {problem}", path.display());
        }
        fixture
    }

    /// The fixture rules beyond its shape: a title; a `working` that shows a
    /// result (`=`) and cites no Java (D-024) on every derived value; the
    /// conversion's keys matching its form; an encoding and a conversion
    /// together.
    fn check(fixture: &Fixture) -> Result<(), String> {
        if fixture.title.trim().is_empty() {
            return Err("`title` is empty".to_owned());
        }
        if fixture.table.is_empty() {
            return Err("no [[table]]".to_owned());
        }
        let working = |what: &str, w: &str| {
            if !w.contains('=') {
                Err(format!("{what}: `working` must show its result with `=`"))
            } else if w.to_lowercase().contains("java") {
                Err(format!(
                    "{what}: `working` cites Java, which is not a source (D-024)"
                ))
            } else {
                Ok(())
            }
        };
        let part = |what: &str,
                    encoding: Option<&ExpectedEncoding>,
                    conversion: Option<&ExpectedConversion>,
                    cells: &[CellCheck]| {
            if encoding.is_some() != conversion.is_some() {
                return Err(format!(
                    "{what}: `encoding` and `conversion` go together (both absent only for static \
                 values)"
                ));
            }
            if let Some(e) = encoding {
                working(&format!("{what} encoding"), &e.working)?;
            }
            if let Some(c) = conversion {
                working(&format!("{what} conversion"), &c.working)?;
                let linear = c.form == "linear";
                if linear != (c.m.is_some() && c.b.is_some()) || (!linear && c.m.or(c.b).is_some())
                {
                    return Err(format!(
                        "{what} conversion: `m` and `b` are given exactly for form = \"linear\""
                    ));
                }
                if linear && c.to_raw {
                    return Err(format!("{what} conversion: a linear form has no `to_raw`"));
                }
            }
            if encoding.is_none() && !cells.is_empty() {
                return Err(format!("{what}: [[cell]] needs an encoding"));
            }
            for (i, cell) in cells.iter().enumerate() {
                working(&format!("{what} cell #{}", i + 1), &cell.working)?;
            }
            Ok(())
        };
        for t in &fixture.table {
            let what = format!("table `{}`", t.name);
            part(&what, t.encoding.as_ref(), t.conversion.as_ref(), &t.cell)?;
            for (i, a) in t.axis.iter().enumerate() {
                let what = format!("{what} axis #{}", i + 1);
                part(&what, a.encoding.as_ref(), a.conversion.as_ref(), &a.cell)?;
            }
        }
        Ok(())
    }

    // ---- the reader's answer, in the fixture's terms -------------------------------

    /// `Unset` is `None`; an omitted field has no place in a resolved table.
    fn opt<T: Clone>(field: &Field<T>, what: &str) -> Option<T> {
        match field {
            Field::Unset => None,
            Field::Set(v) => Some(v.clone()),
            Field::Omit => panic!("{what} is omitted in a resolved table"),
        }
    }

    /// Spelled here, not with rr-core's own names, so the fixture vocabulary
    /// does not move with the code under test.
    fn storage_name(s: Storage) -> &'static str {
        match s {
            Storage::U8 => "u8",
            Storage::U16 => "u16",
            Storage::U20 => "u20",
            Storage::U24 => "u24",
            Storage::U32 => "u32",
            Storage::U64 => "u64",
            Storage::I8 => "i8",
            Storage::I16 => "i16",
            Storage::I24 => "i24",
            Storage::I32 => "i32",
            Storage::I64 => "i64",
            Storage::F32 => "f32",
            Storage::F64 => "f64",
        }
    }

    fn endian_name(e: Endian) -> &'static str {
        match e {
            Endian::Big => "big",
            Endian::Little => "little",
        }
    }

    fn layout_name(l: Layout) -> &'static str {
        match l {
            Layout::Default => "default",
            Layout::BoschSubtract => "bosch-subtract",
        }
    }

    fn kind_name(k: TableKind) -> &'static str {
        match k {
            TableKind::OneD => "1D",
            TableKind::TwoD => "2D",
            TableKind::ThreeD => "3D",
            TableKind::Switch => "Switch",
            TableKind::BitwiseSwitch => "BitwiseSwitch",
        }
    }

    fn bitfield_view(b: BitField) -> ExpectedBitField {
        ExpectedBitField {
            mask: b.mask(),
            shift: b.shift(),
            signed: b.is_signed(),
        }
    }

    fn line_col(span: Span) -> String {
        match span {
            Span::LineCol { line, col } => format!("{line}:{col}"),
            Span::ByteOffset(o) => format!("@{o}"),
        }
    }

    fn scaling_view(s: &ScalingDoc) -> ExpectedScaling {
        ExpectedScaling {
            base: opt(&s.base, "scaling base"),
            name: opt(&s.name, "scaling name"),
            category: opt(&s.category, "scaling category"),
            units: opt(&s.units, "scaling units"),
            expression: s.to_real.get().map(|f| f.text().to_owned()),
            to_byte: s
                .to_raw
                .get()
                .map(|f| f.as_ref().map_or_else(String::new, |f| f.text().to_owned())),
            format: s.format.get().map(|f| f.pattern().to_owned()),
            min: opt(&s.min, "scaling min"),
            max: opt(&s.max, "scaling max"),
            coarse_increment: opt(&s.coarse_increment, "scaling coarse increment"),
            fine_increment: opt(&s.fine_increment, "scaling fine increment"),
        }
    }

    fn cells_view(c: &CellsDoc) -> ExpectedCells {
        let scaling = c
            .scalings
            .entries()
            .map(|(key, state)| match state {
                Nested::Patch(s) | Nested::Replace(s) => (key.clone(), scaling_view(s)),
                Nested::Unset | Nested::Omit => panic!("scaling {key:?} is {state:?}"),
            })
            .collect();
        ExpectedCells {
            storage_address: opt(&c.storage_address, "storage_address"),
            storage: c.storage.get().map(|s| storage_name(*s).to_owned()),
            endian: c.endian.get().map(|e| endian_name(*e).to_owned()),
            bitfield: c.bitfield.get().map(|b| bitfield_view(*b)),
            layout: c.layout.get().map(|l| layout_name(*l).to_owned()),
            before_ram: opt(&c.before_ram, "before_ram"),
            size: opt(&c.size, "size"),
            flip: opt(&c.flip, "flip"),
            static_values: opt(&c.static_values, "static_values"),
            scaling,
        }
    }

    fn encoding_view(e: Encoding) -> ExpectedEncoding {
        ExpectedEncoding {
            storage: storage_name(e.storage()).to_owned(),
            endian: endian_name(e.endian()).to_owned(),
            bitfield: e.bitfield().map(bitfield_view),
            layout: layout_name(e.layout()).to_owned(),
            working: String::new(),
        }
    }

    fn conversion_view(c: &Conversion) -> ExpectedConversion {
        let (form, m, b, to_raw) = match c.form() {
            Form::Linear(l) => ("linear", Some(l.m()), Some(l.b()), false),
            Form::Expr(e) => ("expr", None, None, e.to_raw().is_some()),
            Form::Lut(_) => ("lut", None, None, false),
        };
        ExpectedConversion {
            form: form.to_owned(),
            m,
            b,
            to_raw,
            unit: c.unit().map(str::to_owned),
            format: c.format().map(|f| f.pattern().to_owned()),
            working: String::new(),
        }
    }

    /// The encoding and conversion views of `cells`.
    fn format_views(
        cells: Option<&CellFormat>,
    ) -> (Option<ExpectedEncoding>, Option<ExpectedConversion>) {
        cells.map_or((None, None), |c| {
            (
                Some(encoding_view(c.encoding)),
                Some(conversion_view(&c.conversion)),
            )
        })
    }

    fn axis_view(a: &ResolvedAxis) -> ExpectedAxis {
        let doc: &AxisDoc = &a.doc;
        let (encoding, conversion) = format_views(a.cells.as_ref());
        ExpectedAxis {
            name: opt(&doc.name, "axis name"),
            description: opt(&doc.description, "axis description"),
            log_param: opt(&doc.log_param, "axis log_param"),
            cells: cells_view(&doc.cells),
            encoding,
            conversion,
            cell: Vec::new(),
        }
    }

    fn table_view(t: &ResolvedTable) -> ExpectedTable {
        let d = &t.doc;
        let state = d
            .states
            .entries()
            .map(|(key, state)| {
                let s = state
                    .get()
                    .unwrap_or_else(|| panic!("state {key:?} is {state:?}"));
                ExpectedState {
                    key: key.clone(),
                    name: opt(&s.name, "state name"),
                    data: opt(&s.data, "state data"),
                }
            })
            .collect();
        assert!(
            !d.bits.replaces(),
            "bits of a resolved table replace nothing"
        );
        let bit = d
            .bits
            .items()
            .iter()
            .map(|b| ExpectedBit {
                position: b.position,
                name: b.name.clone(),
            })
            .collect();
        let (encoding, conversion) = format_views(t.cells.as_ref());
        ExpectedTable {
            name: t.name.clone(),
            kind: kind_name(t.kind).to_owned(),
            at: line_col(t.at.span()),
            layer: t.at.layer().unwrap_or("").to_owned(),
            base: opt(&d.base, "base"),
            category: opt(&d.category, "category"),
            description: opt(&d.description, "description"),
            user_level: opt(&d.user_level, "user_level"),
            locked: opt(&d.locked, "locked"),
            log_param: opt(&d.log_param, "log_param"),
            size_x: opt(&d.size_x, "size_x"),
            size_y: opt(&d.size_y, "size_y"),
            flip_x: opt(&d.flip_x, "flip_x"),
            flip_y: opt(&d.flip_y, "flip_y"),
            swap_xy: opt(&d.swap_xy, "swap_xy"),
            skip_cells: opt(&d.skip_cells, "skip_cells"),
            cells: cells_view(&d.cells),
            state,
            bit,
            axis: t.axes.iter().map(axis_view).collect(),
            encoding,
            conversion,
            cell: Vec::new(),
        }
    }

    /// `expected` without its hand-calculation notes and decode checks: what is
    /// compared with [`table_view`].
    fn strip(expected: &ExpectedTable) -> ExpectedTable {
        let mut t = expected.clone();
        let clear = |e: &mut Option<ExpectedEncoding>, c: &mut Option<ExpectedConversion>| {
            if let Some(e) = e {
                e.working.clear();
            }
            if let Some(c) = c {
                c.working.clear();
            }
        };
        clear(&mut t.encoding, &mut t.conversion);
        t.cell.clear();
        for a in &mut t.axis {
            clear(&mut a.encoding, &mut a.conversion);
            a.cell.clear();
        }
        t
    }

    fn diagnostic_view(d: &Diagnostic) -> ExpectedDiagnostic {
        ExpectedDiagnostic {
            code: d.code().to_string(),
            severity: d.severity().to_string(),
            at: line_col(d.provenance().span()),
        }
    }

    /// Hex bytes, `01 F4`.
    fn bytes(hex: &str) -> Vec<u8> {
        hex.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap_or_else(|e| panic!("hex {b:?}: {e}")))
            .collect()
    }

    /// Decodes each check's bytes with `cells` and compares the real value.
    fn check_cells(what: &str, cells: Option<&CellFormat>, checks: &[CellCheck]) {
        let Some(cells) = cells else {
            assert!(checks.is_empty(), "{what}: cells to decode but no encoding");
            return;
        };
        for check in checks {
            let raw = cells
                .encoding
                .decode(&bytes(&check.hex))
                .unwrap_or_else(|e| panic!("{what}: decoding {}: {e}", check.hex));
            let real = cells
                .conversion
                .to_real_raw(raw)
                .unwrap_or_else(|e| panic!("{what}: converting {}: {e}", check.hex));
            let tolerance = 1e-9 * check.real.abs().max(1.0);
            assert!(
                (real - check.real).abs() <= tolerance,
                "{what}: bytes {} decode to {real}, expected {} ({})",
                check.hex,
                check.real,
                check.working
            );
        }
    }

    /// Reads and resolves `fixture` and compares everything it expects.
    fn run(source: &str, fixture: &Fixture) {
        // The XML as authored, whatever line endings a checkout gave it.
        let xml = fixture.xml.replace("\r\n", "\n");
        let (set, mut diagnostics) =
            DefinitionSet::read_all([(source, xml.as_bytes())]).accept_errors();
        let rom = set
            .find(&fixture.resolve)
            .unwrap_or_else(|| panic!("{source}: no <rom> {:?}", fixture.resolve));
        let (resolved, more) = set.resolver().resolve(rom).accept_errors();
        diagnostics.extend(more);
        let found: Vec<ExpectedDiagnostic> = diagnostics.iter().map(diagnostic_view).collect();
        assert_eq!(
            found, fixture.diagnostics,
            "{source}: diagnostics differ:\n{diagnostics:#?}"
        );
        let resolved = resolved.unwrap_or_else(|| panic!("{source}: {:?} not resolved", rom));

        assert_eq!(resolved.unplaced(), fixture.unplaced, "{source}: unplaced");
        for name in &fixture.omitted {
            assert_eq!(
                resolved.doc().tables.get(&table_key(name)),
                Some(&Nested::Omit),
                "{source}: table {name:?} is not omitted in the merged document"
            );
        }
        let omitted: BTreeSet<&String> = resolved
            .doc()
            .tables
            .entries()
            .filter(|(_, s)| s.is_omitted())
            .map(|(k, _)| k)
            .collect();
        let want: BTreeSet<String> = fixture.omitted.iter().map(|n| table_key(n)).collect();
        assert_eq!(
            omitted,
            want.iter().collect(),
            "{source}: omitted tables differ"
        );

        let names: Vec<&str> = resolved.tables().iter().map(|t| t.name.as_str()).collect();
        let want: Vec<&str> = fixture.table.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, want, "{source}: resolved tables differ");
        for (table, expected) in resolved.tables().iter().zip(&fixture.table) {
            let what = format!("{source}: table `{}`", expected.name);
            assert_eq!(table_view(table), strip(expected), "{what}");
            check_cells(&what, table.cells.as_ref(), &expected.cell);
            for (i, (axis, want)) in table.axes.iter().zip(&expected.axis).enumerate() {
                let what = format!("{what} axis #{}", i + 1);
                check_cells(&what, axis.cells.as_ref(), &want.cell);
            }
        }
    }

    // ---- the tests ---------------------------------------------------------------

    /// Every fixture reads and resolves to exactly its hand-checked answer.
    #[test]
    fn every_fixture_resolves_to_its_expected_document() {
        // Each fixture runs on its own, so one failure does not hide another.
        let failed: Vec<String> = fixture_paths()
            .iter()
            .filter_map(|path| {
                let source = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .expect("a UTF-8 file name");
                let ran = std::panic::catch_unwind(|| run(source, &load(path)));
                ran.is_err().then(|| source.to_owned())
            })
            .collect();
        assert!(failed.is_empty(), "fixtures failed (see above): {failed:?}");
    }

    /// The fixtures cover every table kind, every axis type, a static axis, a
    /// static 1D table, a switch's default storage, a `<memmodel>` byte order
    /// and a `base` chain with an `omit`.
    #[test]
    fn the_fixtures_cover_every_table_kind_and_axis_type() {
        let all: Vec<Fixture> = fixture_paths().iter().map(|p| load(p)).collect();
        let kinds: BTreeSet<&str> = all
            .iter()
            .flat_map(|f| &f.table)
            .map(|t| t.kind.as_str())
            .collect();
        let want: BTreeSet<&str> = [
            TableKind::OneD,
            TableKind::TwoD,
            TableKind::ThreeD,
            TableKind::Switch,
            TableKind::BitwiseSwitch,
        ]
        .into_iter()
        .map(kind_name)
        .collect();
        assert_eq!(kinds, want, "a table kind has no fixture");
        for needle in [
            "type=\"X Axis\"",
            "type=\"Y Axis\"",
            "type=\"Static X Axis\"",
            "type=\"Static Y Axis\"",
            "<memmodel",
            "<rom base=",
            "omit=\"true\"",
        ] {
            assert!(
                all.iter().any(|f| f.xml.contains(needle)),
                "no fixture's XML has {needle}"
            );
        }
        let tables = || all.iter().flat_map(|f| &f.table);
        assert!(
            tables().any(|t| t.kind == "1D" && t.encoding.is_none()),
            "no static 1D table"
        );
        assert!(
            tables().any(|t| t.kind == "Switch" && t.cells.storage.is_none()),
            "no switch table relying on the default storage"
        );
        assert!(
            tables()
                .flat_map(|t| &t.axis)
                .any(|a| a.encoding.is_none() && a.cells.static_values.is_some()),
            "no static axis"
        );
        assert!(
            all.iter().any(|f| !f.omitted.is_empty()),
            "no fixture omits a table"
        );
    }

    /// The loader refuses a key it does not know and a working with no
    /// result, so a typo cannot quietly drop an expectation.
    #[test]
    fn the_loader_rejects_unknown_keys_and_empty_working() {
        let base = "schema = 1\ntitle = \"t\"\nresolve = \"X\"\nxml = \"<roms/>\"\n";
        let typo = format!(
            "{base}[[table]]\nname = \"a\"\nkind = \"1D\"\nat = \"1:1\"\nlayer = \"X\"\nuserlevel = 3\n"
        );
        let error = fixtures::parse_kat::<Fixture>("typo.toml", &typo)
            .expect_err("an unknown key is refused")
            .to_string();
        assert!(error.contains("userlevel"), "{error}");

        let nested = format!(
            "{base}[[table]]\nname = \"a\"\nkind = \"1D\"\nat = \"1:1\"\nlayer = \"X\"\n\
         [table.cells]\nstorage_adress = 1\n"
        );
        let error = fixtures::parse_kat::<Fixture>("nested.toml", &nested)
            .expect_err("an unknown nested key is refused")
            .to_string();
        assert!(error.contains("storage_adress"), "{error}");

        let bare = format!(
            "{base}[[table]]\nname = \"a\"\nkind = \"1D\"\nat = \"1:1\"\nlayer = \"X\"\n\
         [table.encoding]\nstorage = \"u8\"\nendian = \"big\"\nlayout = \"default\"\n\
         working = \"u8\"\n[table.conversion]\nform = \"linear\"\nm = 1.0\nb = 0.0\n\
         working = \"1*x + 0 = x\"\n"
        );
        let fixture = fixtures::parse_kat::<Fixture>("bare.toml", &bare)
            .expect("the shape is fine")
            .data;
        let problem = check(&fixture).expect_err("a working without `=` is refused");
        assert!(problem.contains("encoding"), "{problem}");
        // Used, so the field is not dead: notes are free text for reviewers.
        assert!(fixture.notes.is_none());
    }
}
