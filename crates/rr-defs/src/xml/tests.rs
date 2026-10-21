//! Reader tests: the tree for well-formed input, and one diagnostic, with
//! the right code and `line:col`, for each kind of hostile or broken input.

use rr_core::diag::{Code, Severity, Span, codes};

use super::*;

const SRC: &str = "t.xml";

/// Reads `xml` with `limits`, expecting exactly one diagnostic, an error:
/// its code and `line:col`, and its message.
fn refused_with(xml: &[u8], limits: &Limits) -> (Code, String, String) {
    let loaded = parse_with(SRC, xml, limits);
    let diags = loaded.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:#?}");
    let d = &diags[0];
    assert_eq!(d.severity(), Severity::Error, "{d}");
    assert_eq!(d.provenance().source(), SRC);
    let span = d.provenance().span();
    assert!(matches!(span, Span::LineCol { .. }), "{d}");
    let (code, span, message) = (
        d.code(),
        span.to_string().trim_start_matches(':').to_owned(),
        d.message().to_owned(),
    );
    // Fail closed, and no partial document even when the errors are accepted.
    assert_eq!(loaded.value(), None);
    assert_eq!(loaded.accept_errors().0, None);
    (code, span, message)
}

fn refused(xml: &str) -> (Code, String, String) {
    refused_with(xml.as_bytes(), &Limits::DEFAULT)
}

/// Asserts `xml` is refused with `code` at `at` (`line:col`).
#[track_caller]
fn assert_refused(xml: &str, code: Code, at: &str) {
    let (c, span, message) = refused(xml);
    assert_eq!((c, span.as_str()), (code, at), "{message}");
}

/// Reads `xml`, expecting no diagnostics at all.
#[track_caller]
fn read(xml: &str) -> Document {
    let loaded = parse(SRC, xml.as_bytes());
    assert_eq!(loaded.diagnostics(), &[], "{xml}");
    loaded
        .into_result()
        .ok()
        .and_then(|(d, _)| d)
        .unwrap_or_else(|| {
            unreachable!("a clean load has a document");
        })
}

fn lc(line: u32, col: u32) -> Span {
    Span::line_col(line, col)
}

// ---------------------------------------------------------------------------
// Well-formed input
// ---------------------------------------------------------------------------

const DEFINITION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE roms SYSTEM "ecu_defs.dtd">
<!-- a definition -->
<roms>
  <rom base="16BITBASE">
    <romid>
      <xmlid>A2WC522N</xmlid>
      <internalidaddress>2000</internalidaddress>
    </romid>
    <table name="Boost &lt;Target&gt;" storageaddress="0x1234" sizex="16">
      <scaling units="°C" expression="x*0.0078125" to_byte="x/0.0078125"/>
      <table type="X Axis" name="RPM" storageaddress="0x2000"/>
      <data>1 2 &#x33; &amp;4</data>
    </table>
  </rom>
</roms>
"#;

#[test]
fn a_small_definition_parses_to_the_expected_tree() {
    let doc = read(DEFINITION);
    let roms = doc.root();
    assert_eq!(roms.name(), "roms");
    assert_eq!(roms.span(), lc(4, 1));
    assert!(roms.attributes().is_empty());
    assert_eq!(roms.children().len(), 1);
    assert_eq!(roms.text(), "");

    let rom = roms.elements().next().unwrap_or_else(|| unreachable!());
    assert_eq!(rom.name(), "rom");
    assert_eq!(rom.span(), lc(5, 3));
    let base = rom.attribute("base").unwrap_or_else(|| unreachable!());
    assert_eq!((base.name(), base.value()), ("base", "16BITBASE"));
    assert_eq!(base.span(), lc(5, 8));
    assert_eq!(base.value_span(), lc(5, 14));
    assert_eq!(rom.attr("missing"), None);

    let romid = rom
        .elements_named("romid")
        .next()
        .unwrap_or_else(|| unreachable!());
    let fields: Vec<(&str, String)> = romid
        .elements()
        .map(|e| (e.name(), e.text().into_owned()))
        .collect();
    assert_eq!(
        fields,
        [
            ("xmlid", "A2WC522N".to_owned()),
            ("internalidaddress", "2000".to_owned())
        ]
    );
    let xmlid = romid.elements().next().unwrap_or_else(|| unreachable!());
    assert_eq!(xmlid.span(), lc(7, 7));
    let Some(Node::Text(text)) = xmlid.children().first() else {
        unreachable!("{xmlid:?}");
    };
    assert_eq!((text.value(), text.span()), ("A2WC522N", lc(7, 14)));

    let table = rom
        .elements_named("table")
        .next()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(table.attr("name"), Some("Boost <Target>"));
    assert_eq!(table.attr("sizex"), Some("16"));
    let names: Vec<&str> = table.elements().map(Element::name).collect();
    assert_eq!(names, ["scaling", "table", "data"]);
    let scaling = table.elements().next().unwrap_or_else(|| unreachable!());
    assert_eq!(scaling.attr("units"), Some("°C"));
    // Columns count characters: `°` is two bytes but one column.
    let expr = scaling
        .attribute("expression")
        .unwrap_or_else(|| unreachable!());
    assert_eq!(expr.span(), lc(11, 27));
    assert_eq!(expr.value_span(), lc(11, 39));
    assert!(scaling.children().is_empty());
    let data = table
        .elements_named("data")
        .next()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(data.text(), "1 2 3 &4");
    assert_eq!(data.children().len(), 1, "references join one text run");
}

#[test]
fn text_runs_join_references_cdata_and_skip_comments() {
    let doc = read("<a>x &lt; <!-- c --><![CDATA[<y>&amp;]]> z<b/>w</a>");
    let a = doc.root();
    let texts: Vec<(&str, Span)> = a
        .children()
        .iter()
        .filter_map(Node::as_text)
        .map(|t| (t.value(), t.span()))
        .collect();
    assert_eq!(texts, [("x < <y>&amp; z", lc(1, 4)), ("w", lc(1, 47))]);
    assert_eq!(a.text(), "x < <y>&amp; zw");
    assert_eq!(a.children().len(), 3);
}

#[test]
fn whitespace_only_text_is_not_kept() {
    let doc = read("<a>\n  <b> </b>\r\n\t<c/>\n</a>");
    assert_eq!(doc.root().children().len(), 2);
    assert!(doc.root().elements().all(|e| e.children().is_empty()));
    // Whitespace around real text is kept.
    assert_eq!(read("<a> x </a>").root().text(), " x ");
}

#[test]
fn empty_and_start_end_elements_are_the_same() {
    let a = read("<a><b x='1'/></a>").into_root();
    let b = read("<a><b x='1'></b></a>").into_root();
    assert_eq!(a, b);
}

#[test]
fn attribute_values_are_normalised() {
    let doc = read("<a v=\"1\t2\r\n3\n4\r5&#10;6 &quot;&apos;\"/>");
    assert_eq!(doc.root().attr("v"), Some("1 2 3 4 5\n6 \"'"));
}

#[test]
fn crlf_bom_and_lf_variants_read_identically() {
    let lf = DEFINITION.as_bytes().to_vec();
    let crlf = DEFINITION.replace('\n', "\r\n").into_bytes();
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(&crlf);
    let reference = parse(SRC, &lf);
    for variant in [&crlf, &bom] {
        let loaded = parse(SRC, variant);
        assert_eq!(loaded, reference);
    }
    // Text keeps its line breaks, as LF.
    let doc = read("<a>1\r\n2\r3</a>");
    assert_eq!(doc.root().text(), "1\n2\n3");
}

/// `text` in ISO-8859-1: each character (all at most U+00FF) one byte.
fn to_latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or_else(|_| unreachable!("{c:?}")))
        .collect()
}

/// An ECU definition with the non-ASCII characters real ones use (`°`,
/// `µ`, `²`, `é`, `ë`), in element text and attribute values, with line
/// breaks inside text, between attributes and inside an attribute value, so
/// ISO-8859-1 transcoding and every line-end rule are exercised.
const UNITS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<roms>
  <rom>
    <romid>
      <xmlid>A2WC522N</xmlid>
      <make>Citro\u{eb}n</make>
      <model>Caf\u{e9} 2\u{b2}</model>
    </romid>
    <table name=\"Timing \u{b0}\" scaling=\"Inj \u{b5}s\" storageaddress=\"0x1234\"
           category=\"Fuel &amp; Timing\">
      <scaling name=\"Inj \u{b5}s\" units=\"\u{b0}C\" expression=\"x-40\"
               description=\"Accel m/s\u{b2},
 r\u{e9}sum\u{e9}\" format=\"0.0\"/>
      degrees \u{b0}BTDC
      over two lines
    </table>
  </rom>
</roms>
";

#[test]
fn encoding_bom_and_line_end_variants_read_identically() {
    let latin1_decl = UNITS.replace("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\"");
    let reference = parse(SRC, UNITS.as_bytes());
    assert_eq!(reference.diagnostics(), &[]);
    let doc = reference
        .value()
        .cloned()
        .flatten()
        .unwrap_or_else(|| unreachable!());
    let rom = doc
        .root()
        .elements()
        .next()
        .unwrap_or_else(|| unreachable!());
    let romid = rom
        .elements_named("romid")
        .next()
        .unwrap_or_else(|| unreachable!());
    let fields: Vec<String> = romid.elements().map(|e| e.text().into_owned()).collect();
    assert_eq!(fields, ["A2WC522N", "Citroën", "Café 2²"]);
    let table = rom
        .elements_named("table")
        .next()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(table.attr("name"), Some("Timing °"));
    assert_eq!(table.attr("scaling"), Some("Inj µs"));
    assert_eq!(table.attr("category"), Some("Fuel & Timing"));
    let scaling = table
        .elements_named("scaling")
        .next()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(scaling.attr("name"), Some("Inj µs"));
    assert_eq!(scaling.attr("units"), Some("°C"));
    // §3.3.3: the line end inside the value becomes one space, whichever
    // line end the file uses.
    assert_eq!(scaling.attr("description"), Some("Accel m/s²,  résumé"));
    // §2.11: text keeps its line breaks, as LF.
    assert_eq!(
        table.text(),
        "\n      degrees °BTDC\n      over two lines\n    "
    );

    for eol in ["\n", "\r\n", "\r"] {
        let utf8 = UNITS.replace('\n', eol);
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(utf8.as_bytes());
        let latin1 = to_latin1(&latin1_decl.replace('\n', eol));
        let lower = to_latin1(
            &UNITS
                .replace("encoding=\"UTF-8\"", "encoding='latin1'")
                .replace('\n', eol),
        );
        for (what, variant) in [
            ("UTF-8", utf8.into_bytes()),
            ("UTF-8 with BOM", bom),
            ("ISO-8859-1", latin1),
            ("latin1", lower),
        ] {
            // The whole result, tree and diagnostics, spans included: lines
            // break alike, and columns count characters, so `°` is one
            // column in either encoding.
            assert_eq!(parse(SRC, &variant), reference, "{what} with {eol:?}");
        }
    }
}

#[test]
fn same_content_ignores_spans_only() {
    // Without a declaration every line moves up one: spans differ, content
    // does not.
    let bare = UNITS
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    let latin1 = to_latin1(&UNITS.replace("UTF-8", "ISO-8859-1").replace('\n', "\r\n"));
    let a = read(bare);
    let b = parse(SRC, &latin1)
        .value()
        .cloned()
        .flatten()
        .unwrap_or_else(|| unreachable!());
    assert_ne!(a, b);
    assert!(a.same_content(&b));
    assert!(b.same_content(&a));
    // Any difference in name, attribute or text is a difference.
    for other in [
        bare.replace("units=", "unit="),
        bare.replace("\u{b0}C", "\u{b0}F"),
        bare.replace("over two", "over 2"),
        bare.replace("<table", "<tablex")
            .replace("</table>", "</tablex>"),
        bare.replace("format=\"0.0\"/>", "format=\"0.0\"><x/></scaling>"),
    ] {
        assert!(!a.same_content(&read(&other)), "{other}");
    }
}

#[test]
fn positions_in_a_latin1_file_are_its_own() {
    let xml = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\r\n<a u=\"\u{b0}\u{b0}\u{b5}\" v=\"x\">\u{b0}&bad;</a>";
    let latin1 = to_latin1(xml);
    // `v` is the 12th character (and byte) of line 2; in UTF-8 it is the
    // 15th byte.
    let loaded = parse(SRC, &to_latin1(&xml.replace("&bad;", "")));
    let doc = loaded
        .value()
        .cloned()
        .flatten()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(
        doc.root().attribute("v").map(Attribute::span),
        Some(lc(2, 12))
    );
    assert_eq!(doc.root().attr("u"), Some("°°µ"));
    let (code, at, _) = refused_with(&latin1, &Limits::DEFAULT);
    assert_eq!((code, at.as_str()), (codes::XML_UNKNOWN_ENTITY, "2:19"));
    // The same file in UTF-8 reports the same position.
    let utf8 = xml.replace("ISO-8859-1", "UTF-8");
    assert_refused(&utf8, codes::XML_UNKNOWN_ENTITY, "2:19");
}

#[test]
fn unsupported_encodings_are_refused() {
    let (code, at, message) = refused("<?xml version=\"1.0\" encoding=\"windows-1252\"?>\n<a/>");
    assert_eq!((code, at.as_str()), (codes::XML_ENCODING, "1:31"));
    assert!(message.contains("windows-1252"), "{message}");
    let (code, at, message) = refused_with(b"\xFF\xFE<\x00a\x00/\x00>\x00", &Limits::DEFAULT);
    assert_eq!((code, at.as_str()), (codes::XML_ENCODING, "1:1"));
    assert!(message.contains("UTF-16"), "{message}");
    // US-ASCII declared, then a non-ASCII byte.
    let (code, at, _) = refused_with(
        b"<?xml version=\"1.0\" encoding=\"us-ascii\"?>\n<a>\xB0</a>",
        &Limits::DEFAULT,
    );
    assert_eq!((code, at.as_str()), (codes::XML_ENCODING, "2:4"));
    // An encoding pseudo-attribute that is not well-formed is never
    // ignored.
    assert_refused(
        "<?xml version=\"1.0\" encoding=latin1?><a/>",
        codes::XML_MALFORMED,
        "1:30",
    );
}

#[test]
fn a_bom_contradicting_the_declaration_is_read_as_utf8_with_a_warning() {
    let mut xml = b"\xEF\xBB\xBF".to_vec();
    xml.extend_from_slice(
        "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\n<a u=\"\u{b0}C\"/>".as_bytes(),
    );
    let loaded = parse(SRC, &xml);
    let diags = loaded.diagnostics();
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(diags[0].code(), codes::XML_ENCODING_MISMATCH);
    assert_eq!(diags[0].severity(), Severity::Warning);
    assert_eq!(diags[0].provenance().to_string(), "t.xml:1:31");
    let doc = loaded
        .value()
        .cloned()
        .flatten()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(doc.root().attr("u"), Some("°C"));
}

#[test]
fn real_doctypes_are_read_past() {
    for xml in [
        // Logger definitions.
        "<?xml version=\"1.0\"?>\n<!DOCTYPE logger SYSTEM \"logger.dtd\"[]>\n<logger/>",
        // Dyno definitions.
        "<!DOCTYPE dyno SYSTEM \"cars_def.dtd\">\n<dyno/>",
        // An ECU definition with an internal subset of declarations.
        "<!DOCTYPE roms [\n<!ELEMENT roms ( rom+ ) >\n\
         <!ATTLIST table storagetype (uint8|uint16|float) #IMPLIED >\n\
         <!ATTLIST table name CDATA #REQUIRED >\n]>\n<roms/>",
    ] {
        assert_eq!(read(xml).root().children(), &[], "{xml}");
    }
}

#[test]
fn an_attlist_default_is_warned_about_but_the_file_is_read() {
    let xml =
        "<!DOCTYPE roms [\n  <!ATTLIST rom abstract CDATA \"false\" >\n]>\n<roms><rom/></roms>";
    let loaded = parse(SRC, xml.as_bytes());
    let diags = loaded.diagnostics();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code(), codes::XML_DTD_DEFAULT_IGNORED);
    assert_eq!(diags[0].severity(), Severity::Warning);
    assert_eq!(diags[0].provenance().to_string(), "t.xml:2:3");
    assert!(diags[0].message().contains("<rom>"), "{}", diags[0]);
    let doc = loaded
        .into_result()
        .ok()
        .and_then(|(d, _)| d)
        .unwrap_or_else(|| unreachable!());
    // Not applied: the element is read as written.
    let rom = doc
        .root()
        .elements()
        .next()
        .unwrap_or_else(|| unreachable!());
    assert_eq!(rom.attr("abstract"), None);
}

#[test]
fn processing_instructions_and_comments_anywhere_are_ignored() {
    let doc = read("<!-- x --><?pi a?><a><?pi b?><!-- y --></a><!-- z --><?pi c?>\n");
    assert_eq!(doc.root().children(), &[]);
}

// ---------------------------------------------------------------------------
// Entities and DTDs
// ---------------------------------------------------------------------------

#[test]
fn billion_laughs_is_refused_at_the_first_entity() {
    let mut xml = String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n");
    xml.push_str(" <!ENTITY lol \"lol\">\n");
    for i in 1..10 {
        let prev = if i == 1 {
            "lol".to_owned()
        } else {
            format!("lol{}", i - 1)
        };
        let refs = format!("&{prev};").repeat(10);
        xml.push_str(&format!(" <!ENTITY lol{i} \"{refs}\">\n"));
    }
    xml.push_str("]>\n<lolz>&lol9;</lolz>\n");
    let (code, at, message) = refused(&xml);
    assert_eq!((code, at.as_str()), (codes::XML_ENTITY_DECLARED, "3:2"));
    assert!(message.contains("`lol`"), "{message}");
}

#[test]
fn quadratic_blowup_is_refused() {
    let big = "a".repeat(100_000);
    let xml = format!(
        "<!DOCTYPE k [<!ENTITY a \"{big}\">]><k>{}</k>",
        "&a;".repeat(10_000)
    );
    assert_refused(&xml, codes::XML_ENTITY_DECLARED, "1:14");
}

#[test]
fn external_entities_are_refused() {
    // Classic XXE.
    assert_refused(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE foo [\n  <!ENTITY xxe SYSTEM \"file:///etc/passwd\">\n]>\n<foo>&xxe;</foo>",
        codes::XML_ENTITY_DECLARED,
        "3:3",
    );
    // PUBLIC, and unparsed (NDATA).
    assert_refused(
        "<!DOCTYPE foo [<!ENTITY x PUBLIC \"-//X//EN\" \"http://evil/x\">]><foo/>",
        codes::XML_ENTITY_DECLARED,
        "1:16",
    );
    assert_refused(
        "<!DOCTYPE foo [<!NOTATION n SYSTEM 'n'><!ENTITY x SYSTEM 'x.gif' NDATA n>]><foo/>",
        codes::XML_ENTITY_DECLARED,
        "1:40",
    );
}

#[test]
fn parameter_entities_are_refused() {
    assert_refused(
        "<!DOCTYPE foo [\n<!ENTITY % dtd SYSTEM \"http://evil/x.dtd\">\n%dtd;\n]>\n<foo/>",
        codes::XML_ENTITY_DECLARED,
        "2:1",
    );
    assert_refused(
        "<!DOCTYPE foo [\n  %dtd;\n]>\n<foo/>",
        codes::XML_ENTITY_DECLARED,
        "2:3",
    );
}

#[test]
fn unknown_entities_are_errors_never_expanded() {
    assert_refused("<a>\n  x &xxe; y</a>", codes::XML_UNKNOWN_ENTITY, "2:5");
    assert_refused("<a b=\"x&nbsp;\"/>", codes::XML_UNKNOWN_ENTITY, "1:8");
    assert_refused(
        "<a>\n<b c='&lt;&ent;'/></a>",
        codes::XML_UNKNOWN_ENTITY,
        "2:11",
    );
    let (_, _, message) = refused("<a>&copy;</a>");
    assert!(message.contains("`&copy;`"), "{message}");
}

#[test]
fn malformed_doctypes_are_refused() {
    assert_refused("<!DOCTYPE x [ junk ]><x/>", codes::XML_MALFORMED, "1:15");
    assert_refused("<a/><!DOCTYPE a><b/>", codes::XML_MALFORMED, "1:5");
    assert_refused("<!DOCTYPE a><!DOCTYPE a><a/>", codes::XML_MALFORMED, "1:13");
    assert_refused("<a><!DOCTYPE a></a>", codes::XML_MALFORMED, "1:4");
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

#[test]
fn deep_nesting_is_refused_at_the_level_past_the_limit() {
    let deep = "<a>".repeat(100_000);
    // 64 `<a>` fit; the 65th, at column 64 * 3 + 1, does not.
    assert_refused(&deep, codes::XML_TOO_DEEP, "1:193");
    let closed = format!("{deep}{}", "</a>".repeat(100_000));
    assert_refused(&closed, codes::XML_TOO_DEEP, "1:193");
    // The limit holds at exactly 64.
    let ok = format!("{}{}", "<a>".repeat(64), "</a>".repeat(64));
    assert_eq!(read(&ok).root().name(), "a");
}

#[test]
fn no_limits_raise_the_depth_past_the_ceiling() {
    let mut limits = Limits::DEFAULT;
    limits.max_depth = u32::MAX;
    assert_eq!(limits.effective_depth(), MAX_DEPTH);
    let deep = "<a>\n".repeat(2000);
    let (code, at, _) = refused_with(deep.as_bytes(), &limits);
    assert_eq!((code, at.as_str()), (codes::XML_TOO_DEEP, "1025:1"));
}

#[test]
fn huge_attributes_are_refused_before_they_are_copied() {
    let xml = format!("<a\n  b=\"{}\"/>", "x".repeat(1 << 20));
    assert_refused(&xml, codes::XML_VALUE_TOO_LONG, "2:6");
    // At the limit is fine.
    let xml = format!("<a b=\"{}\"/>", "&lt;".repeat(16 << 10));
    assert_eq!(read(&xml).root().attr("b").map(str::len), Some(16 << 10));
    // Names are limited too.
    let long = "n".repeat((64 << 10) + 1);
    assert_refused(&format!("<{long}/>"), codes::XML_VALUE_TOO_LONG, "1:2");
    assert_refused(
        &format!("<a {long}='1'/>"),
        codes::XML_VALUE_TOO_LONG,
        "1:4",
    );
}

#[test]
fn too_many_attributes_are_refused_at_the_first_one_over() {
    let attrs: String = (0..100).map(|i| format!(" a{i:02}='1'")).collect();
    // 64 attributes of 8 characters after `<e`, then the space before the 65th.
    assert_refused(
        &format!("<e{attrs}/>"),
        codes::XML_TOO_MANY_ATTRIBUTES,
        "1:516",
    );
    let attrs: String = (0..64).map(|i| format!(" a{i:02}='1'")).collect();
    assert_eq!(read(&format!("<e{attrs}/>")).root().attributes().len(), 64);
}

#[test]
fn too_many_attributes_in_the_file_are_refused_at_the_first_one_over() {
    let mut limits = Limits::DEFAULT;
    limits.max_total_attributes = 5;
    let xml = "<r a='1'>\n<e b='1' c='2'/>\n<e d='1' e='2'/>\n<e f='1'/></r>";
    let (code, at, message) = refused_with(xml.as_bytes(), &limits);
    assert_eq!((code, at.as_str()), (codes::XML_TOO_MANY_ATTRIBUTES, "4:4"));
    assert!(message.contains("the file has more than 5"), "{message}");
    limits.max_total_attributes = 6;
    assert!(!parse_with(SRC, xml.as_bytes(), &limits).has_errors());
}

#[test]
fn too_many_elements_are_refused_at_the_first_one_over() {
    let mut limits = Limits::DEFAULT;
    limits.max_elements = 10;
    let xml = format!("<r>\n{}</r>", "<e/>\n".repeat(20));
    let (code, at, _) = refused_with(xml.as_bytes(), &limits);
    // The root and 9 children fit; the 10th child, on line 11, does not.
    assert_eq!((code, at.as_str()), (codes::XML_TOO_MANY_ELEMENTS, "11:1"));
    let xml = format!("<r>{}</r>", "<e/>".repeat(9));
    assert!(!parse_with(SRC, xml.as_bytes(), &limits).has_errors());
}

#[test]
fn oversized_files_are_refused_unread() {
    let mut limits = Limits::DEFAULT;
    limits.max_input_len = 16;
    // Not even well-formed: the size is checked before anything else.
    let (code, at, message) = refused_with(b"<a>\n\xFF\xFF this is too long", &limits);
    assert_eq!((code, at.as_str()), (codes::XML_TOO_LARGE, "1:1"));
    assert!(message.contains("23 bytes"), "{message}");
    assert!(
        !parse_with(SRC, b"<a>0123456789</a>", &{
            let mut l = limits;
            l.max_input_len = 17;
            l
        })
        .has_errors()
    );
}

#[test]
fn total_text_is_limited() {
    let mut limits = Limits::DEFAULT;
    limits.max_text_len = 10;
    let (code, at, _) = refused_with(b"<a x='12345'>\n678 <b>9 1</b></a>", &limits);
    assert_eq!((code, at.as_str()), (codes::XML_TEXT_TOO_LONG, "2:8"));
    // Whitespace between elements does not count.
    let xml = format!("<a x='12345'>{}<b>67890</b></a>", " \n".repeat(100));
    assert!(!parse_with(SRC, xml.as_bytes(), &limits).has_errors());
}

#[test]
fn default_limits_are_the_documented_ones() {
    assert_eq!(Limits::default(), Limits::DEFAULT);
    let l = Limits::DEFAULT;
    assert_eq!(l.max_input_len, 64 << 20);
    assert_eq!(l.max_depth, 64);
    assert_eq!(l.max_elements, 1_000_000);
    assert_eq!(l.max_attributes, 64);
    assert_eq!(l.max_total_attributes, 2_000_000);
    assert_eq!(l.max_value_len, 64 << 10);
    assert_eq!(l.max_text_len, 32 << 20);
}

// ---------------------------------------------------------------------------
// Well-formedness
// ---------------------------------------------------------------------------

#[test]
fn malformed_xml_is_refused_where_it_goes_wrong() {
    for (xml, at) in [
        // Mismatched end tag: quick-xml points at the end tag.
        ("<a>\n  <b></c>\n</a>", "2:6"),
        // An end tag with nothing open.
        ("<a/></a>", "1:5"),
        // Unclosed elements: the end of the file, naming the element.
        ("<a>\n  <b>\n", "3:1"),
        // An unclosed tag.
        ("<a>\n<b", "2:1"),
        // Attribute problems: duplicate, unquoted, no value.
        ("<a x='1' x='2'/>", "1:10"),
        ("<a x=1/>", "1:6"),
        ("<a x/>", "1:5"),
        // A bare ampersand, and a bad character reference.
        ("<a>fish & chips</a>", "1:9"),
        ("<a>&#xZZ;</a>", "1:4"),
        ("<a>&#0;</a>", "1:4"),
        ("<a b='&#xD800;'/>", "1:7"),
        // Document structure.
        ("", "1:1"),
        ("<!-- only a comment -->\n", "2:1"),
        ("<a/>\n<b/>", "2:1"),
        ("<a/>\ntrailing", "2:1"),
        ("leading\n<a/>", "1:1"),
        ("<a/>&amp;", "1:5"),
        ("<a/><![CDATA[x]]>", "1:5"),
        // An XML declaration that is not first.
        ("\n<?xml version=\"1.0\"?><a/>", "2:1"),
        ("<a/><?xml version=\"1.0\"?>", "1:5"),
        // An unclosed comment.
        ("<a><!-- </a>", "1:4"),
    ] {
        let (code, span, message) = refused(xml);
        assert_eq!(
            (code, span.as_str()),
            (codes::XML_MALFORMED, at),
            "{xml:?}: {message}"
        );
    }
}

#[test]
fn unclosed_elements_are_named() {
    let (_, _, message) = refused("<roms>\n  <rom>\n");
    assert!(message.contains("<rom> opened at 2:3"), "{message}");
}

#[test]
fn invalid_utf8_is_refused_at_the_bad_byte() {
    // ISO-8859-1 `°` (0xB0) on line 2, after `<a>` and a BOM.
    let (code, at, message) = refused_with(b"\xEF\xBB\xBF<r>\n<a>\xB0C</a></r>", &Limits::DEFAULT);
    assert_eq!((code, at.as_str()), (codes::XML_ENCODING, "2:4"));
    assert!(message.contains("UTF-8"), "{message}");
}

#[test]
fn every_xml_code_is_registered() {
    for code in [
        codes::XML_MALFORMED,
        codes::XML_ENCODING,
        codes::XML_ENTITY_DECLARED,
        codes::XML_UNKNOWN_ENTITY,
        codes::XML_DTD_DEFAULT_IGNORED,
        codes::XML_TOO_LARGE,
        codes::XML_TOO_DEEP,
        codes::XML_TOO_MANY_ELEMENTS,
        codes::XML_TOO_MANY_ATTRIBUTES,
        codes::XML_VALUE_TOO_LONG,
        codes::XML_TEXT_TOO_LONG,
        codes::XML_ENCODING_MISMATCH,
    ] {
        let info = codes::lookup(code).unwrap_or_else(|| unreachable!("{code}"));
        let expected =
            if code == codes::XML_DTD_DEFAULT_IGNORED || code == codes::XML_ENCODING_MISMATCH {
                Severity::Warning
            } else {
                Severity::Error
            };
        assert_eq!(info.severity, expected, "{code}");
    }
}

// ---------------------------------------------------------------------------
// Arbitrary input
// ---------------------------------------------------------------------------

mod arbitrary {
    use proptest::prelude::*;

    use super::*;

    /// Fragments real and hostile files are made of, so random documents
    /// reach deep into the reader rather than failing at the first byte.
    const PIECES: &[&str] = &[
        "<a>",
        "</a>",
        "<b x='1'>",
        "</b>",
        "<c/>",
        "text",
        " ",
        "\r\n",
        "&amp;",
        "&lt;",
        "&#176;",
        "&#xZZ;",
        "&e;",
        "<![CDATA[x]]>",
        "<!-- c -->",
        "<?pi?>",
        "<?xml version=\"1.0\"?>",
        "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>",
        "<?xml version='1.0' encoding='us-ascii'?>",
        "<?xml version=\"1.0\" encoding=\"x\"?>",
        "<?xml encoding=?>",
        "\r",
        "<!DOCTYPE a [<!ELEMENT a ANY>]>",
        "<!DOCTYPE a [<!ENTITY e 'x'>]>",
        "<!DOCTYPE a SYSTEM 'a.dtd'>",
        "<d y=\"&q;\" z='2'>",
        "</d>",
        "°",
        "\u{feff}",
        "&",
        "<",
        ">",
        "\"",
        "'",
        "=",
    ];

    fn document() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            proptest::collection::vec(proptest::sample::select(PIECES), 0..40)
                .prop_map(|p| p.concat().into_bytes()),
            proptest::collection::vec(any::<u8>(), 0..200),
        ]
    }

    proptest! {
        /// Any input reads without panicking; a refused one has exactly one
        /// error, with a `line:col` inside the input, and no document; a read
        /// one has no errors.
        #[test]
        fn any_input_reads_or_is_refused_with_a_located_error(input in document()) {
            let loaded = parse(SRC, &input);
            let errors = loaded.diagnostics().iter().filter(|d| d.is_error()).count();
            let lines = u32::try_from(input.len() + 1).unwrap_or(u32::MAX);
            for d in loaded.diagnostics() {
                match d.provenance().span() {
                    Span::LineCol { line, col } => {
                        prop_assert!(line >= 1 && line <= lines && col >= 1, "{}", d);
                    }
                    Span::ByteOffset(_) => prop_assert!(false, "{}", d),
                }
            }
            prop_assert!(errors <= 1);
            let (value, _) = loaded.accept_errors();
            prop_assert_eq!(value.is_some(), errors == 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding and line-end variants of arbitrary documents
// ---------------------------------------------------------------------------

mod variants {
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::*;

    /// A generated element: what is written, from which the tree XML 1.0
    /// requires is worked out independently of the reader.
    #[derive(Debug, Clone)]
    struct Model {
        name: String,
        /// Name, raw value, and whether a line break (rather than a space)
        /// comes before it in the tag.
        attributes: Vec<(String, String, bool)>,
        children: Vec<Item>,
    }

    #[derive(Debug, Clone)]
    enum Item {
        Text(String),
        Element(Model),
    }

    /// Characters ISO-8859-1 can write, as content: printable ASCII,
    /// U+00A0..=U+00FF, tab and (often) LF; the characters units use are
    /// weighted up. Never CR: a generated LF is what each variant turns
    /// into its own line end.
    fn content_chars() -> Vec<char> {
        let mut chars: Vec<char> = (0x20_u8..=0x7E)
            .chain(0xA0..=0xFF)
            .map(char::from)
            .collect();
        chars.extend(['\t', ' ', ' '].repeat(2));
        chars.extend(['\n'].repeat(12));
        chars.extend(['°', 'µ', '²', 'é'].repeat(4));
        chars
    }

    fn content() -> impl Strategy<Value = String> {
        vec(select(content_chars()), 0..16).prop_map(|c| c.into_iter().collect())
    }

    /// An XML name of ASCII and ISO-8859-1 letters.
    fn name() -> impl Strategy<Value = String> {
        let start: Vec<char> = ('A'..='Z')
            .chain('a'..='z')
            .chain(['_'])
            .chain('\u{c0}'..='\u{d6}')
            .chain('\u{d8}'..='\u{f6}')
            .chain('\u{f8}'..='\u{ff}')
            .collect();
        let mut rest = start.clone();
        rest.extend('0'..='9');
        rest.extend(['-', '.', '\u{b7}']);
        (select(start), vec(select(rest), 0..6))
            .prop_map(|(first, rest)| std::iter::once(first).chain(rest).collect())
    }

    fn attributes() -> impl Strategy<Value = Vec<(String, String, bool)>> {
        vec((name(), content(), any::<bool>()), 0..4).prop_map(|mut attrs| {
            let mut seen = std::collections::BTreeSet::new();
            attrs.retain(|(n, _, _)| seen.insert(n.clone()));
            attrs
        })
    }

    fn model() -> impl Strategy<Value = Model> {
        let leaf =
            (name(), attributes(), vec(content(), 0..2)).prop_map(|(name, attributes, texts)| {
                Model {
                    name,
                    attributes,
                    children: texts.into_iter().map(Item::Text).collect(),
                }
            });
        leaf.prop_recursive(4, 32, 5, |inner| {
            (
                name(),
                attributes(),
                vec(
                    prop_oneof![
                        content().prop_map(Item::Text),
                        inner.prop_map(Item::Element)
                    ],
                    0..5,
                ),
            )
                .prop_map(|(name, attributes, children)| Model {
                    name,
                    attributes,
                    children,
                })
        })
    }

    fn escape(s: &str, in_attribute: bool, out: &mut String) {
        for c in s.chars() {
            match c {
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '&' => out.push_str("&amp;"),
                '"' if in_attribute => out.push_str("&quot;"),
                c => out.push(c),
            }
        }
    }

    /// `m` as XML, LF line ends.
    fn write(m: &Model, out: &mut String) {
        out.push('<');
        out.push_str(&m.name);
        for (name, value, line_break) in &m.attributes {
            out.push_str(if *line_break { "\n    " } else { " " });
            out.push_str(name);
            out.push_str("=\"");
            escape(value, true, out);
            out.push('"');
        }
        if m.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for item in &m.children {
            match item {
                Item::Text(t) => escape(t, false, out),
                Item::Element(e) => write(e, out),
            }
        }
        out.push_str("</");
        out.push_str(&m.name);
        out.push('>');
    }

    /// The element XML 1.0 says `m` is, spans aside: attribute values with
    /// each tab and line end a space (§3.3.3), text with line ends as LF
    /// (§2.11, generated as LF already), adjacent text one run, and
    /// whitespace-only runs not kept.
    fn expected(m: &Model) -> Element {
        let span = Span::line_col(1, 1);
        let mut children = Vec::new();
        let mut run = String::new();
        let flush = |run: &mut String, children: &mut Vec<Node>| {
            let value = std::mem::take(run);
            if !value.bytes().all(is_space) {
                children.push(Node::Text(Text { value, span }));
            }
        };
        for item in &m.children {
            match item {
                Item::Text(t) => run.push_str(t),
                Item::Element(e) => {
                    flush(&mut run, &mut children);
                    children.push(Node::Element(expected(e)));
                }
            }
        }
        flush(&mut run, &mut children);
        Element {
            name: m.name.clone(),
            span,
            attributes: m
                .attributes
                .iter()
                .map(|(name, value, _)| Attribute {
                    name: name.clone(),
                    value: value.replace(['\t', '\n'], " "),
                    span,
                    value_span: span,
                })
                .collect(),
            children,
        }
    }

    proptest! {
        /// The same document in UTF-8, UTF-8 with a BOM and ISO-8859-1, with
        /// LF, CRLF or CR line ends, reads with no diagnostic to the tree
        /// XML 1.0 requires, and every variant with a declaration to the
        /// identical result, spans included; without a declaration (lines
        /// unchanged after the first), to the same content.
        #[test]
        fn every_encoding_and_line_end_variant_reads_identically(m in model()) {
            let mut body = String::new();
            write(&m, &mut body);
            let declared = |encoding: &str, eol: &str| {
                format!("<?xml version=\"1.0\" encoding=\"{encoding}\"?>\n{body}\n")
                    .replace('\n', eol)
            };
            let want = Document { root: expected(&m) };
            let reference = parse(SRC, declared("UTF-8", "\n").as_bytes());
            prop_assert_eq!(reference.diagnostics(), &[]);
            let doc = reference.value().cloned().flatten();
            prop_assert!(
                doc.as_ref().is_some_and(|d| d.same_content(&want)),
                "{body:?}\nread {doc:#?}\nwant {want:#?}"
            );
            for eol in ["\n", "\r\n", "\r"] {
                let utf8 = declared("UTF-8", eol).into_bytes();
                let mut bom = b"\xEF\xBB\xBF".to_vec();
                bom.extend_from_slice(&utf8);
                let latin1 = to_latin1(&declared("ISO-8859-1", eol));
                for (what, variant) in [("UTF-8", utf8), ("UTF-8 with BOM", bom), ("ISO-8859-1", latin1)] {
                    prop_assert_eq!(&parse(SRC, &variant), &reference, "{} with {:?}", what, eol);
                }
                let bare = format!("{body}\n").replace('\n', eol);
                let mut bare_bom = b"\xEF\xBB\xBF".to_vec();
                bare_bom.extend_from_slice(bare.as_bytes());
                for (what, variant) in [("bare UTF-8", bare.into_bytes()), ("bare UTF-8 with BOM", bare_bom)] {
                    let loaded = parse(SRC, &variant);
                    prop_assert_eq!(loaded.diagnostics(), &[], "{} with {:?}", what, eol);
                    let doc = loaded.value().cloned().flatten();
                    prop_assert!(
                        doc.is_some_and(|d| d.same_content(&want)),
                        "{} with {:?}", what, eol
                    );
                }
            }
        }
    }
}
