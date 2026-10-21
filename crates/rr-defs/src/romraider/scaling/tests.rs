//! Unit tests of the `<scaling>` / `<scalingbase>` reader and of
//! [`build_cells`] (US-RR-29-7): a hand-checked fixture (XML in, typed
//! partial document out), keys and repeats, every diagnostic with its
//! `line:col`, and the encoding and conversion built after the merge.

use proptest::prelude::*;
use rr_core::conversion::{Form, Linear};
use rr_core::diag::{Code, Diagnostic, Provenance, Span, codes};
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Storage};
use rr_core::layer::{Field, Keyed, Merge, Nested};

use super::super::value::parse_java_double;
use super::super::{DefinitionFile, RomEntry, TableDoc, read};
use super::{
    CellFormat, DEFAULT_CATEGORY, ScalingDoc, build_cells, scaling_key, select_scaling,
    to_raw_mismatch,
};
use crate::romraider::{CellsDoc, TableKind};

/// `(code, "line:col")` of each diagnostic.
fn found(diags: &[Diagnostic]) -> Vec<(Code, String)> {
    diags
        .iter()
        .map(|d| {
            let at = match d.provenance().span() {
                Span::LineCol { line, col } => format!("{line}:{col}"),
                Span::ByteOffset(o) => format!("@{o}"),
            };
            (d.code(), at)
        })
        .collect()
}

/// Reads `xml` expecting exactly `expected` diagnostics; the file (if any)
/// and the diagnostics.
fn load(xml: &str, expected: &[(Code, &str)]) -> (Option<DefinitionFile>, Vec<Diagnostic>) {
    let (file, diags) = read("t.xml", xml.as_bytes()).accept_errors();
    let want: Vec<(Code, String)> = expected
        .iter()
        .map(|(c, a)| (*c, (*a).to_owned()))
        .collect();
    assert_eq!(found(&diags), want, "{diags:#?}");
    (file, diags)
}

/// A file of one base-less `<rom>` whose one table (named `T`, 1D, on line
/// 3) holds `inner`, which starts on line 4, column 1.
fn in_table(inner: &str) -> String {
    format!(
        "<roms>\n<rom><romid><xmlid>X</xmlid></romid>\n\
         <table name=\"T\" type=\"1D\" storagetype=\"uint8\">\n{inner}\n</table>\n</rom>\n</roms>"
    )
}

/// The only rom of a file.
fn only_rom(file: Option<DefinitionFile>) -> RomEntry {
    let mut roms = file.map(DefinitionFile::into_roms).unwrap_or_default();
    assert_eq!(roms.len(), 1);
    roms.remove(0)
}

/// The table `T` of the only rom of `xml`, read with exactly `expected`
/// diagnostics.
fn table_t(xml: &str, expected: &[(Code, &str)]) -> TableDoc {
    let (file, _) = load(xml, expected);
    let rom = only_rom(file);
    let state = rom.doc().tables.get(&"t".to_owned()).expect("table T");
    state.get().expect("present").clone()
}

/// The scalings of the table holding `inner`.
fn scalings(inner: &str, expected: &[(Code, &str)]) -> Keyed<String, ScalingDoc> {
    table_t(&in_table(inner), expected).cells.scalings
}

/// The one scaling of category `Default` among `inner`.
fn default_of(inner: &str, expected: &[(Code, &str)]) -> ScalingDoc {
    let s = scalings(inner, expected);
    assert_eq!(s.len(), 1, "{s:?}");
    s.get(&"default".to_owned())
        .and_then(Nested::get)
        .expect("a Default scaling")
        .clone()
}

fn text(value: &str) -> Field<String> {
    Field::Set(value.to_owned())
}

fn formula_text(f: &Field<super::Formula>) -> Option<&str> {
    f.get().map(super::Formula::text)
}

// ---- values -----------------------------------------------------------------

#[test]
fn decimals_read_as_legacy_double_parse_double() {
    for (text, want) in [
        ("1", 1.0),
        (".01", 0.01),
        ("50", 50.0),
        ("-40", -40.0),
        ("+2.5", 2.5),
        ("1.", 1.0),
        ("1e3", 1000.0),
        ("1.5E-2", 0.015),
        ("2e+1", 20.0),
        (" .0625\n", 0.0625),
        ("3d", 3.0),
        ("0.5F", 0.5),
        ("-0", -0.0),
    ] {
        assert_eq!(parse_java_double(text), Some(want), "{text:?}");
    }
    assert_eq!(parse_java_double("Infinity"), Some(f64::INFINITY));
    assert_eq!(parse_java_double("-Infinity"), Some(f64::NEG_INFINITY));
    assert!(parse_java_double("NaN").is_some_and(f64::is_nan));
    for text in [
        "", ".", "-", "e3", "1e", "1e+", "1.2.3", "inf", "infinity", "nan", "0x10", "1,5", "1 2",
        "--1", "1f2", "ten", "1ee2",
    ] {
        assert_eq!(parse_java_double(text), None, "{text:?}");
    }
}

// ---- the fixture ------------------------------------------------------------

/// A `<scaling>` with every attribute, as the pinned corpus writes them,
/// and the typed document expected, field by field.
#[test]
fn kat_scaling() {
    let doc = default_of(
        "<scaling name=\"AFR\" units=\"AFR\" expression=\"14.7/(1+x*.0078125)\" \
         to_byte=\"(14.7/x-1)/.0078125\" format=\"0.00\" min=\"8\" max=\"20\" \
         coarseincrement=\".5\" fineincrement=\".01\"/>",
        &[],
    );
    assert_eq!(doc.name, text("AFR"));
    assert_eq!(doc.units, text("AFR"));
    assert_eq!(doc.category, Field::Unset);
    assert_eq!(doc.base, Field::Unset);
    assert_eq!(formula_text(&doc.to_real), Some("14.7/(1+x*.0078125)"));
    let to_raw = doc.to_raw.get().and_then(Option::as_ref).expect("to_byte");
    assert_eq!(to_raw.text(), "(14.7/x-1)/.0078125");
    assert_eq!(to_raw.expr().eval(14.7, &[]), Ok(0.0));
    assert_eq!(
        doc.to_real.get().map(|f| f.expr().eval(0.0, &[])),
        Some(Ok(14.7))
    );
    assert_eq!(doc.format.get().map(|f| f.pattern()), Some("0.00"));
    assert_eq!(doc.min, Field::Set(8.0));
    assert_eq!(doc.max, Field::Set(20.0));
    assert_eq!(doc.coarse_increment, Field::Set(0.5));
    assert_eq!(doc.fine_increment, Field::Set(0.01));
}

#[test]
fn an_empty_scaling_says_nothing_and_is_the_default_category() {
    assert_eq!(default_of("<scaling/>", &[]), ScalingDoc::default());
}

#[test]
fn attribute_names_compare_as_legacy_did() {
    // Legacy's loop over the attributes compared these ignoring case ...
    let doc = default_of(
        "<scaling UNITS=\"rpm\" Expression=\"x*2\" FORMAT=\"#\" Min=\"0\" MAX=\"9\" \
         Increment=\"5\" FineIncrement=\"1\"/>",
        &[],
    );
    assert_eq!(doc.units, text("rpm"));
    assert_eq!(formula_text(&doc.to_real), Some("x*2"));
    assert_eq!(doc.format.get().map(|f| f.pattern()), Some("#"));
    assert_eq!((doc.min, doc.max), (Field::Set(0.0), Field::Set(9.0)));
    assert_eq!(doc.coarse_increment, Field::Set(5.0));
    assert_eq!(doc.fine_increment, Field::Set(1.0));
    // ... and read these by exact name.
    let doc = default_of(
        "<scaling To_Byte=\"x\" Name=\"n\" BASE=\"b\" Category=\"c\"/>",
        &[
            (codes::UNKNOWN_ATTRIBUTE, "4:10"),
            (codes::UNKNOWN_ATTRIBUTE, "4:22"),
            (codes::UNKNOWN_ATTRIBUTE, "4:31"),
            (codes::UNKNOWN_ATTRIBUTE, "4:40"),
        ],
    );
    assert_eq!(doc, ScalingDoc::default());
}

#[test]
fn an_empty_to_byte_is_none_and_nan_is_absent() {
    let doc = default_of(
        "<scaling to_byte=\"\" expression=\"NaN\" min=\"NaN\"/>",
        &[],
    );
    assert_eq!(doc.to_raw, Field::Set(None));
    assert_eq!(doc.to_real, Field::Unset);
    assert_eq!(doc.min, Field::Unset);
}

#[test]
fn base_none_is_no_base() {
    assert_eq!(
        default_of("<scaling base=\"NONE\"/>", &[]).base,
        Field::Unset
    );
    assert_eq!(
        default_of("<scaling base=\"Temp\"/>", &[]).base,
        text("Temp")
    );
}

#[test]
fn both_coarse_increment_spellings_warn_and_the_later_wins() {
    let doc = default_of(
        "<scaling increment=\"1\" coarseincrement=\"2\"/>",
        &[(codes::CONTENT_IGNORED, "4:24")],
    );
    assert_eq!(doc.coarse_increment, Field::Set(2.0));
}

// ---- formulas ---------------------------------------------------------------

#[test]
fn a_bad_expression_is_its_error_and_a_bad_to_byte_a_warning_at_the_value() {
    let (_, diags) = load(
        &in_table("<scaling expression=\"x*(2\"\n to_byte=\"x/P8\"/>"),
        &[
            (Code::new("EXP0001"), "4:22"),
            (codes::UNUSABLE_TO_BYTE, "5:11"),
        ],
    );
    assert!(diags[0].is_error());
    assert!(!diags[1].is_error());
    let m = diags[0].message();
    assert!(
        m.starts_with("`expression` \"x*(2\": the to_real formula"),
        "{m}"
    );
    assert!(m.ends_with("(at character 3)"), "{m}");
    let m = diags[1].message();
    assert!(
        m.starts_with("`to_byte` \"x/P8\": the to_raw formula refers to P8"),
        "{m}"
    );
    assert!(
        m.ends_with(
            "(at character 3) [CNV0001]; ignored: values are stored with the inverse of the \
             expression"
        ),
        "{m}"
    );
    // Neither formula is kept.
    let doc = default_of(
        "<scaling expression=\"\" to_byte=\"x+\"/>",
        &[
            (Code::new("EXP0001"), "4:22"),
            (codes::UNUSABLE_TO_BYTE, "4:33"),
        ],
    );
    assert_eq!((doc.to_real, doc.to_raw), (Field::Unset, Field::Unset));
}

#[test]
fn every_corpus_style_formula_reads() {
    for f in [
        "x",
        "x*.01",
        "(x-128)*.3515625",
        "14.7/(1+x*.0078125)",
        "60000000/x",
        "x*(5/4)-40",
        "x^2*0.001",
        "sqrt(x)",
        "if(x>128,x-256,x)",
    ] {
        let doc = default_of(&format!("<scaling expression=\"{f}\"/>"), &[]);
        assert_eq!(formula_text(&doc.to_real), Some(f));
    }
}

// ---- numbers ----------------------------------------------------------------

#[test]
fn bad_numbers_are_errors_and_infinity_a_warning_at_the_value() {
    let doc = default_of(
        "<scaling min=\"low\" max=\"Infinity\" fineincrement=\"0x1\" coarseincrement=\"1\"/>",
        &[
            (codes::INVALID_SCALING_NUMBER, "4:15"),
            (codes::CONTENT_IGNORED, "4:25"),
            (codes::INVALID_SCALING_NUMBER, "4:50"),
        ],
    );
    assert_eq!(doc.min, Field::Unset);
    assert_eq!(doc.max, Field::Unset);
    assert_eq!(doc.fine_increment, Field::Unset);
    assert_eq!(doc.coarse_increment, Field::Set(1.0));
}

// ---- unknown content ----------------------------------------------------------

#[test]
fn unknown_attributes_children_and_text_are_warnings_naming_them() {
    let xml = in_table(
        "<scaling units=\"C\" storagetype=\"uint16\" endian=\"little\">\
         <data>1</data>stray</scaling>",
    );
    let (_, diags) = load(
        &xml,
        &[
            (codes::UNKNOWN_ATTRIBUTE, "4:20"),
            (codes::UNKNOWN_ATTRIBUTE, "4:41"),
            (codes::UNEXPECTED_TEXT, "4:71"),
            (codes::UNKNOWN_ELEMENT, "4:57"),
        ],
    );
    let messages: Vec<&str> = diags.iter().map(Diagnostic::message).collect();
    assert_eq!(
        messages,
        [
            "unknown attribute `storagetype` on <scaling>; ignored",
            "unknown attribute `endian` on <scaling>; ignored",
            "text directly inside <scaling>; ignored",
            "unknown element <data> in <scaling>; ignored",
        ]
    );
}

// ---- categories and repeats -------------------------------------------------------

#[test]
fn scalings_are_keyed_by_category_ignoring_case() {
    let s = scalings(
        "<scaling category=\"Metric\" units=\"C\"/>\n\
         <scaling category=\"Imperial\" units=\"F\"/>\n\
         <scaling units=\"raw\"/>",
        &[],
    );
    let keys: Vec<&str> = s.entries().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["metric", "imperial", "default"]);
    let metric = s.get(&"metric".to_owned()).and_then(Nested::get).unwrap();
    assert_eq!(metric.category, text("Metric"));
    assert_eq!(scaling_key("METRIC"), "metric");
}

#[test]
fn a_repeated_category_merges_with_a_warning_naming_both_places() {
    let (file, diags) = load(
        &in_table(
            "<scaling category=\"Metric\" units=\"C\" expression=\"x-40\"/>\n\
             <scaling category=\"METRIC\" format=\"0.0\"/>",
        ),
        &[(codes::DUPLICATE_ELEMENT, "5:1")],
    );
    assert!(
        diags[0].message().contains("(the first is at 4:1)"),
        "{diags:?}"
    );
    assert!(diags[0].message().contains("in table `T`"), "{diags:?}");
    let rom = only_rom(file);
    let t = rom
        .doc()
        .tables
        .get(&"t".to_owned())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(t.cells.scalings.len(), 1);
    let s = t
        .cells
        .scalings
        .get(&"metric".to_owned())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(s.units, text("C"));
    assert_eq!(s.category, text("METRIC"));
    assert_eq!(formula_text(&s.to_real), Some("x-40"));
    assert_eq!(s.format.get().map(|f| f.pattern()), Some("0.0"));
}

#[test]
fn an_axis_reads_its_own_scalings() {
    let xml = "<roms>\n<rom><romid><xmlid>X</xmlid></romid>\n\
               <table name=\"T\" type=\"2D\" storagetype=\"uint8\" sizey=\"2\">\n\
               <scaling units=\"ms\" expression=\"x*.1\"/>\n\
               <table type=\"Y Axis\" name=\"RPM\" storagetype=\"uint16\">\n\
               <scaling units=\"rpm\" expression=\"x*50\"/>\n\
               <scaling units=\"rpm2\"/></table>\n\
               </table>\n</rom>\n</roms>";
    let t = table_t(xml, &[(codes::DUPLICATE_ELEMENT, "7:1")]);
    let axis = t.y_axis.get().expect("Y axis");
    let s = axis
        .cells
        .scalings
        .get(&"default".to_owned())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(s.units, text("rpm2"));
    assert_eq!(formula_text(&s.to_real), Some("x*50"));
    let own = t
        .cells
        .scalings
        .get(&"default".to_owned())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(own.units, text("ms"));
}

#[test]
fn a_child_definition_patches_an_inherited_scaling_by_category() {
    let base = default_of(
        "<scaling units=\"C\" expression=\"x-40\" to_byte=\"x+40\" format=\"0\"/>",
        &[],
    );
    let xml = "<roms>\n<rom base=\"B\"><romid><xmlid>X</xmlid></romid>\n\
               <table name=\"T\"><scaling format=\"0.0\"/>\
               <scaling category=\"Imperial\" units=\"F\"/></table>\n</rom>\n</roms>";
    let child = table_t(xml, &[]);
    let mut merged = Keyed::new();
    merged.patch("default".to_owned(), base);
    merged.merge_from(&child.cells.scalings);
    let keys: Vec<&str> = merged.entries().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["default", "imperial"]);
    let s = merged.get(&"default".to_owned()).unwrap().get().unwrap();
    assert_eq!(s.units, text("C"));
    assert_eq!(formula_text(&s.to_real), Some("x-40"));
    assert_eq!(s.format.get().map(|f| f.pattern()), Some("0.0"));
    // `to_byte` is inherited (legacy reset it).
    assert!(s.to_raw.get().is_some_and(Option::is_some));
}

// ---- scaling bases ----------------------------------------------------------

#[test]
fn scaling_bases_are_read_keyed_by_name_and_a_repeat_replaces() {
    let xml = "<roms>\n\
               <scalingbase name=\"Temp\" units=\"C\" expression=\"x-40\" category=\"Metric\"/>\n\
               <scalingbase units=\"kPa\" expression=\"x*2\"/>\n\
               <scalingbase name=\"TEMP\" units=\"F\"/>\n\
               <scalingbase/>\n\
               <rom><romid><xmlid>X</xmlid></romid>\n\
               <table name=\"T\" type=\"1D\" storagetype=\"uint8\"><scaling base=\"temp\"/></table>\n\
               </rom>\n</roms>";
    let (file, diags) = load(xml, &[(codes::DUPLICATE_ELEMENT, "4:1")]);
    assert!(diags[0].message().contains("(first at 2:1)"), "{diags:?}");
    let file = file.unwrap();
    let bases = file.scaling_bases();
    let keys: Vec<&str> = bases.entries().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["temp", "kpa", "none"]);
    let temp = bases.get(&"temp".to_owned()).unwrap();
    assert!(matches!(temp, Nested::Replace(_)), "{temp:?}");
    let temp = temp.get().unwrap();
    assert_eq!(temp.units, text("F"));
    assert_eq!(temp.to_real, Field::Unset, "replaced whole");
    let rom = &file.roms()[0];
    let t = rom
        .doc()
        .tables
        .get(&"t".to_owned())
        .unwrap()
        .get()
        .unwrap();
    let s = t
        .cells
        .scalings
        .get(&"default".to_owned())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(s.base, text("temp"));
}

// ---- selecting the scaling shown --------------------------------------------

#[test]
fn the_scaling_shown_is_the_preferred_category_else_default_else_raw() {
    let s = scalings(
        "<scaling category=\"Metric\" units=\"C\"/>\n<scaling units=\"raw\"/>\n\
         <scaling category=\"Imperial\" units=\"F\"/>",
        &[],
    );
    let units = |p: Option<&str>| select_scaling(&s, p).and_then(|s| s.units.get().cloned());
    assert_eq!(units(Some("metric")), Some("C".to_owned()));
    assert_eq!(units(Some("IMPERIAL")), Some("F".to_owned()));
    assert_eq!(units(Some("Other")), Some("raw".to_owned()));
    assert_eq!(units(None), Some("raw".to_owned()));
    let only_imperial = scalings("<scaling category=\"Imperial\" units=\"F\"/>", &[]);
    assert!(select_scaling(&only_imperial, Some("Metric")).is_none());
    assert_eq!(DEFAULT_CATEGORY, "Default");
}

// ---- building the encoding and conversion -----------------------------------------

fn at() -> Provenance {
    Provenance::new("t.xml", Span::line_col(9, 9))
}

fn cells(storage: Option<Storage>) -> CellsDoc {
    CellsDoc {
        storage: storage.map_or(Field::Unset, Field::Set),
        ..CellsDoc::default()
    }
}

/// [`build_cells`] with no diagnostics.
fn built(
    cells: &CellsDoc,
    kind: Option<TableKind>,
    scaling: Option<&ScalingDoc>,
    mem: Option<Endian>,
) -> CellFormat {
    let (value, diags) = build_cells(cells, kind, scaling, mem, &at()).accept_errors();
    assert!(diags.is_empty(), "{diags:#?}");
    value.expect("built")
}

#[test]
fn the_encoding_takes_each_part_from_the_merged_cells() {
    let c = CellsDoc {
        storage: Field::Set(Storage::I16),
        endian: Field::Set(Endian::Little),
        bitfield: Field::Set(BitField::new(0x0FF0, None, false).unwrap()),
        layout: Field::Set(Layout::Default),
        ..CellsDoc::default()
    };
    let got = built(&c, Some(TableKind::OneD), None, None);
    assert_eq!(
        got.encoding,
        Encoding::new(
            Storage::I16,
            Endian::Little,
            Some(BitField::new(0x0FF0, None, false).unwrap()),
            Layout::Default
        )
        .unwrap()
    );
    let c = CellsDoc {
        layout: Field::Set(Layout::BoschSubtract),
        ..cells(Some(Storage::U16))
    };
    assert_eq!(
        built(&c, None, None, None).encoding.layout(),
        Layout::BoschSubtract
    );
}

#[test]
fn byte_order_is_the_memmodel_s_else_the_table_s_else_big() {
    let mut c = cells(Some(Storage::U16));
    assert_eq!(built(&c, None, None, None).encoding.endian(), Endian::Big);
    c.endian = Field::Set(Endian::Little);
    assert_eq!(
        built(&c, None, None, None).encoding.endian(),
        Endian::Little
    );
    assert_eq!(
        built(&c, None, None, Some(Endian::Big)).encoding.endian(),
        Endian::Big
    );
    c.endian = Field::Unset;
    assert_eq!(
        built(&c, None, None, Some(Endian::Little))
            .encoding
            .endian(),
        Endian::Little
    );
}

#[test]
fn a_switch_without_a_storage_type_is_uint8_and_any_other_table_an_error() {
    for kind in [TableKind::Switch, TableKind::BitwiseSwitch] {
        let got = built(&cells(None), Some(kind), None, None);
        assert_eq!(got.encoding.storage(), Storage::U8);
    }
    for kind in [
        Some(TableKind::OneD),
        Some(TableKind::TwoD),
        Some(TableKind::ThreeD),
        None,
    ] {
        let (value, diags) = build_cells(&cells(None), kind, None, None, &at()).accept_errors();
        assert!(value.is_none());
        assert_eq!(
            found(&diags),
            [(codes::MISSING_STORAGE_TYPE, "9:9".to_owned())]
        );
    }
}

#[test]
fn a_combination_the_codec_refuses_is_its_enc_error() {
    let c = CellsDoc {
        bitfield: Field::Set(BitField::new(0x1FF, None, false).unwrap()),
        ..cells(Some(Storage::U8))
    };
    let (value, diags) = build_cells(&c, None, None, None, &at()).accept_errors();
    assert!(value.is_none());
    assert_eq!(found(&diags), [(Code::new("ENC0003"), "9:9".to_owned())]);
    let c = CellsDoc {
        endian: Field::Set(Endian::Little),
        ..cells(Some(Storage::U20))
    };
    let (value, diags) = build_cells(&c, None, None, None, &at()).accept_errors();
    assert!(value.is_none());
    assert_eq!(diags.len(), 1);
    assert!(diags[0].code().as_str().starts_with("ENC"), "{diags:?}");
}

#[test]
fn no_scaling_is_raw_values() {
    let got = built(&cells(Some(Storage::U8)), None, None, None);
    let c = &got.conversion;
    assert_eq!(c.form(), &Form::Linear(Linear::new(1.0, 0.0).unwrap()));
    assert_eq!((c.unit(), c.format()), (None, None));
    assert_eq!(c.to_real(7.0), Ok(7.0));
}

#[test]
fn the_conversion_is_the_scaling_s_formulas_unit_and_format() {
    let s = default_of(
        "<scaling units=\"degC\" expression=\"x-40\" to_byte=\"x+40\" format=\"0.0\"/>",
        &[],
    );
    let got = built(&cells(Some(Storage::U8)), None, Some(&s), None);
    let c = &got.conversion;
    assert_eq!(c.unit(), Some("degC"));
    assert_eq!(c.format().map(|f| f.pattern()), Some("0.0"));
    assert_eq!(c.to_real(100.0), Ok(60.0));
    let Form::Expr(form) = c.form() else {
        panic!("{c:?}")
    };
    assert!(form.to_raw().is_some());
    assert_eq!(form.linear(), Some(Linear::new(1.0, -40.0).unwrap()));
    assert_eq!(to_raw_mismatch(&got), None);
}

#[test]
fn a_scaling_without_an_expression_is_x_as_in_legacy() {
    let s = default_of("<scaling units=\"steps\" to_byte=\"\"/>", &[]);
    let got = built(&cells(Some(Storage::U16)), None, Some(&s), None);
    let Form::Expr(form) = got.conversion.form() else {
        panic!("{got:?}")
    };
    assert_eq!(form.to_raw(), None);
    assert_eq!(got.conversion.to_real(513.0), Ok(513.0));
    assert_eq!(got.conversion.unit(), Some("steps"));
}

#[test]
fn a_to_byte_that_does_not_invert_the_expression_is_found() {
    let s = default_of("<scaling expression=\"x*2\" to_byte=\"x/3\"/>", &[]);
    let got = built(&cells(Some(Storage::U8)), None, Some(&s), None);
    let m = to_raw_mismatch(&got).expect("a mismatch");
    assert_eq!(m.code(), codes::TO_RAW_MISMATCH);
    // Over float storage a mismatch is not looked for.
    let got = built(&cells(Some(Storage::F32)), None, Some(&s), None);
    assert_eq!(to_raw_mismatch(&got), None);
    // Nor with no to_byte.
    let s = default_of("<scaling expression=\"x*2\"/>", &[]);
    let got = built(&cells(Some(Storage::U8)), None, Some(&s), None);
    assert_eq!(to_raw_mismatch(&got), None);
}

#[test]
fn errors_in_both_parts_are_all_reported() {
    let (value, diags) = build_cells(
        &cells(None),
        None,
        Some(&ScalingDoc::default()),
        None,
        &at(),
    )
    .accept_errors();
    assert!(value.is_none());
    assert_eq!(found(&diags).len(), 1);
}

// ---- never panics ---------------------------------------------------------------

/// Attribute text: numbers, formulas and odd.
fn attr_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[-+ .0-9eExX]{0,10}",
        "[-+*/^()x0-9. ]{0,16}",
        "(NaN|Infinity|none|Default|Metric|x|P8|\\[P8:rpm\\])",
        any::<String>().prop_map(|s| {
            s.chars()
                .filter(|c| !matches!(c, '<' | '&' | '"') && !c.is_control())
                .collect()
        }),
    ]
}

proptest! {
    #[test]
    fn any_scaling_attribute_values_read_and_build_without_panicking(
        values in proptest::collection::vec(attr_text(), 11),
        storage in prop_oneof![Just(Storage::U8), Just(Storage::I16), Just(Storage::U32), Just(Storage::F32)],
    ) {
        let names = [
            "base", "name", "category", "units", "expression", "to_byte", "format", "min",
            "max", "coarseincrement", "fineincrement",
        ];
        let attrs: String = names
            .iter()
            .zip(&values)
            .map(|(n, v)| format!(" {n}=\"{v}\""))
            .collect();
        let xml = format!(
            "<roms><scalingbase{attrs}/><rom><romid><xmlid>X</xmlid></romid>\
             <table name=\"T\" type=\"1D\" storagetype=\"uint8\"><scaling{attrs}/></table>\
             </rom></roms>"
        );
        let (file, _) = read("t.xml", xml.as_bytes()).accept_errors();
        if let Some(file) = file {
            for rom in file.roms() {
                for (_, t) in rom.doc().tables.present() {
                    for (_, s) in t.cells.scalings.present() {
                        let c = cells(Some(storage));
                        if let (Some(got), _) = build_cells(&c, None, Some(s), None, &at()).accept_errors() {
                            let _ = got.conversion.to_real(1.0);
                        }
                    }
                }
            }
        }
    }
}
