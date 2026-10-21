//! Unknown elements and attributes in a RomRaider ECU definition produce
//! Warning diagnostics naming them at their `line:col` (US-RR-29-3; MF-23:
//! never a silent drop, where legacy RomRaider skipped them without a word).
//!
//! One minimal definition per element context the reader reads: `<roms>`, a
//! lone `<rom>` root, `<rom>`, `<romid>` and its value elements, `<table>` of
//! every kind, axis tables (`X Axis`, `Y Axis`, `Static Y Axis`), axes the
//! reader ignores, `<scaling>`, `<scalingbase>`, `<state>`, `<bit>`, `<data>`,
//! `<description>`, an omitted table, unknown attributes whose value is
//! `NaN` (legacy read a `NaN` value as an absent attribute), and many
//! unknowns in one element (every one listed, not only the first).
//!
//! Two checks over the same cases:
//!
//! * [`warnings_snapshot`]: one golden snapshot of every case: its XML, line
//!   numbered, then each diagnostic as rendered (`file:line:col:
//!   severity[code]: message`) with its source line and a caret under the
//!   `line:col`.
//! * [`every_unknown_is_named_where_it_is`]: a sweep that does not trust the
//!   snapshot: for every case, no Error, exactly the expected Warnings, each
//!   with its exact message, and each `line:col` sliced out of the source
//!   text must be the unknown attribute's name (`name=`) or element's start
//!   tag (`<name`), so a snapshot blessed with a wrong position fails here.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use rr_core::diag::{Code, Diagnostic, Severity, Span, codes};
    use rr_defs::romraider;

    /// The source name every case is read as.
    const SOURCE: &str = "defs.xml";

    /// One warning a case must give.
    struct Expect {
        code: Code,
        /// The attribute or element name the warning's `line:col` points at.
        at: At,
        /// The exact message, when the warning is one this test is about.
        message: Option<String>,
    }

    /// What a warning's `line:col` points at.
    enum At {
        /// An attribute's name, as `name=`.
        Attribute(&'static str),
        /// An element's start tag, as `<name`.
        Element(&'static str),
    }

    /// `DEF0005`: unknown attribute `name` on the element described as
    /// `on`.
    fn attr(name: &'static str, on: &str) -> Expect {
        Expect {
            code: codes::UNKNOWN_ATTRIBUTE,
            at: At::Attribute(name),
            message: Some(format!("unknown attribute `{name}` on {on}; ignored")),
        }
    }

    /// `DEF0004`: unknown element `<name>` in the element described as `in_`.
    fn elem(name: &'static str, in_: &str) -> Expect {
        Expect {
            code: codes::UNKNOWN_ELEMENT,
            at: At::Element(name),
            message: Some(format!("unknown element <{name}> in {in_}; ignored")),
        }
    }

    /// Another warning a case gives as well (its message is not what this
    /// test is about), at the start tag of `<name>`.
    const fn other(code: Code, name: &'static str) -> Expect {
        Expect {
            code,
            at: At::Element(name),
            message: None,
        }
    }

    /// One minimal definition and the warnings it must give, in document
    /// order.
    struct Case {
        label: &'static str,
        xml: &'static str,
        expect: Vec<Expect>,
    }

    const AXIS: &str = "an axis <table>";

    #[expect(clippy::too_many_lines, reason = "one flat list of cases")]
    fn cases() -> Vec<Case> {
        vec![
            Case {
                label: "clean definition: no diagnostics at all",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="1D" name="Idle" storageaddress="100" storagetype="uint8" category="NaN"/>
  </rom>
</roms>"#,
                expect: vec![],
            },
            Case {
                label: "<roms>: unknown attribute and child element",
                xml: r#"<roms version="2">
  <frobnicate/>
  <rom><romid><xmlid>A</xmlid></romid></rom>
</roms>"#,
                expect: vec![attr("version", "<roms>"), elem("frobnicate", "<roms>")],
            },
            Case {
                label: "lone <rom> root: unknown attribute and child element",
                xml: r#"<rom colour="red">
  <romid><xmlid>A</xmlid></romid>
  <patch/>
</rom>"#,
                expect: vec![
                    other(codes::SINGLE_ROM_ROOT, "rom"),
                    attr("colour", "<rom>"),
                    elem("patch", "<rom>"),
                ],
            },
            Case {
                label: "<rom>: unknown attribute and child element",
                xml: r#"<roms>
  <rom base="none" colour="red">
    <romid><xmlid>A</xmlid></romid>
    <patch/>
  </rom>
</roms>"#,
                expect: vec![attr("colour", "<rom>"), elem("patch", "<rom>")],
            },
            Case {
                label: "<romid>: unknown attribute and child element",
                xml: r#"<roms>
  <rom>
    <romid region="eu">
      <xmlid>A</xmlid>
      <colour>red</colour>
    </romid>
  </rom>
</roms>"#,
                expect: vec![attr("region", "<romid>"), elem("colour", "<romid>")],
            },
            Case {
                label: "<romid> value elements: unknown attributes and child elements",
                xml: r#"<roms>
  <rom>
    <romid>
      <xmlid lang="en">A</xmlid>
      <make>Subaru<badge/></make>
      <memmodel endian="big" width="32">SH7058</memmodel>
      <noramoffset unit="b"/>
    </romid>
  </rom>
</roms>"#,
                expect: vec![
                    attr("lang", "<xmlid>"),
                    elem("badge", "<make>"),
                    attr("width", "<memmodel>"),
                    attr("unit", "<noramoffset>"),
                ],
            },
            Case {
                label: "1D <table> and its <scaling>: unknown attributes and child elements",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="1D" name="Idle" storageaddress="100" storagetype="uint8" colour="red">
      <scaling units="rpm" expression="x" format="0" colour="blue"><note/></scaling>
      <graph/>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("colour", "<table>"),
                    attr("colour", "<scaling>"),
                    elem("note", "<scaling>"),
                    elem("graph", "<table>"),
                ],
            },
            Case {
                label: "<data> and <description> of a table: unknown attributes and child elements",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="1D" name="Gear" storageaddress="100" storagetype="uint8">
      <data index="1">1<sub/></data>
      <description lang="en">Gear<b/></description>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("index", "<data>"),
                    elem("sub", "<data>"),
                    attr("lang", "<description>"),
                    elem("b", "<description>"),
                ],
            },
            Case {
                label: "2D <table> and its Y Axis: unknown attributes and child elements",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="2D" name="Boost" storageaddress="100" storagetype="uint8" sizey="2" colour="red">
      <table type="Y Axis" name="RPM" storageaddress="200" storagetype="uint8" colour="blue">
        <scaling units="rpm" expression="x" smooth="1"/>
        <ticks/>
      </table>
      <graph/>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("colour", "<table>"),
                    attr("colour", AXIS),
                    attr("smooth", "<scaling>"),
                    elem("ticks", AXIS),
                    elem("graph", "<table>"),
                ],
            },
            Case {
                label: "3D <table>, its X Axis and Static Y Axis, and the axis's <data> and <description>",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="3D" name="Fuel" storageaddress="100" storagetype="uint8" sizex="2" sizey="2" colour="red">
      <table type="X Axis" name="Load" storageaddress="200" storagetype="uint8" colour="blue"><ticks/></table>
      <table type="Static Y Axis" name="Gear" sizey="2" colour="green">
        <data index="0">1</data>
        <data>2<sub/></data>
        <description lang="en">gears<b/></description>
      </table>
      <graph/>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("colour", "<table>"),
                    attr("colour", AXIS),
                    elem("ticks", AXIS),
                    attr("colour", AXIS),
                    attr("index", "<data>"),
                    elem("sub", "<data>"),
                    attr("lang", "<description>"),
                    elem("b", "<description>"),
                    elem("graph", "<table>"),
                ],
            },
            Case {
                label: "Switch <table> and its <state>: unknown attributes and child elements",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="Switch" name="Flag" storageaddress="100" sizey="1" frob="1">
      <state name="on" data="01" colour="red"><note/></state>
      <state name="off" data="00"/>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("frob", "<table>"),
                    attr("colour", "<state>"),
                    elem("note", "<state>"),
                ],
            },
            Case {
                label: "BitwiseSwitch <table> and its <bit>: unknown attributes and child elements",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="BitwiseSwitch" name="Bits" storageaddress="100" storagetype="uint8" frob="1">
      <bit position="0" name="A" colour="red"><note/></bit>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("frob", "<table>"),
                    attr("colour", "<bit>"),
                    elem("note", "<bit>"),
                ],
            },
            Case {
                label: "<scalingbase>: unknown attribute and child element",
                xml: r#"<roms>
  <scalingbase name="rpm" units="rpm" expression="x" colour="red"><note/></scalingbase>
  <rom><romid><xmlid>A</xmlid></romid></rom>
</roms>"#,
                expect: vec![
                    attr("colour", "<scalingbase>"),
                    elem("note", "<scalingbase>"),
                ],
            },
            Case {
                label: "omitted <table>: its unknown attribute and child element are still named",
                xml: r#"<roms>
  <rom><romid><xmlid>B</xmlid></romid><table type="1D" name="Idle" storageaddress="100"/></rom>
  <rom base="B">
    <romid><xmlid>A</xmlid></romid>
    <table name="Idle" omit="true" colour="red">
      <graph/>
      <scaling units="x"/>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    other(codes::CONTENT_IGNORED, "table"),
                    attr("colour", "<table>"),
                    elem("graph", "<table>"),
                ],
            },
            Case {
                label: "unknown attributes valued NaN (legacy's absent value) are still named",
                xml: r#"<roms>
  <rom><romid><xmlid>B</xmlid></romid><table type="1D" name="Idle" storageaddress="100"/></rom>
  <rom base="B" colour="NaN">
    <romid><xmlid>A</xmlid></romid>
    <table name="Idle" omit="true" colour="NaN"/>
    <table type="2D" name="Boost" storageaddress="100" sizey="2" category="NaN" colour="NaN">
      <table type="Y Axis" name="RPM" storageaddress="200" logparam="NaN" colour="NaN">
        <scaling units="rpm" expression="x" min="NaN" colour="NaN"/>
      </table>
      <state name="on" data="01" colour="NaN"/>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    attr("colour", "<rom>"),
                    attr("colour", "<table>"),
                    attr("colour", "<table>"),
                    attr("colour", AXIS),
                    attr("colour", "<scaling>"),
                    attr("colour", "<state>"),
                ],
            },
            Case {
                label: "many unknowns in one element: every one is listed",
                xml: r#"<roms a="1" b="2">
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="1D" name="Idle" storageaddress="100" x1="1" x2="2" x3="3">
      <one/>
      <two/>
      <three/>
    </table>
  </rom>
  <first/>
  <second/>
</roms>"#,
                expect: vec![
                    attr("a", "<roms>"),
                    attr("b", "<roms>"),
                    attr("x1", "<table>"),
                    attr("x2", "<table>"),
                    attr("x3", "<table>"),
                    elem("one", "<table>"),
                    elem("two", "<table>"),
                    elem("three", "<table>"),
                    elem("first", "<roms>"),
                    elem("second", "<roms>"),
                ],
            },
            Case {
                label: "element names ignore case; attribute names do not (except a scaling's caseless ones)",
                xml: r#"<ROMS>
  <Rom>
    <ROMID><XMLID>A</XMLID></ROMID>
    <TABLE type="1D" name="Idle" storageaddress="100" Category="fuel">
      <Scaling Units="rpm" EXPRESSION="x" To_Byte="x"/>
    </TABLE>
  </Rom>
</ROMS>"#,
                expect: vec![attr("Category", "<TABLE>"), attr("To_Byte", "<Scaling>")],
            },
            Case {
                label: "axes the reader ignores: their unknown attributes and child elements are still named",
                xml: r#"<roms>
  <rom>
    <romid><xmlid>A</xmlid></romid>
    <table type="1D" name="Idle" storageaddress="100">
      <table type="X Axis" name="RPM" colour="red"><ticks/></table>
    </table>
    <table type="2D" name="Boost" storageaddress="100" sizey="2">
      <table type="Y Axis" name="RPM" storageaddress="200">
        <table type="X Axis" name="Inner" colour="green"><ticks/></table>
      </table>
      <table type="Z Axis" name="Gear" colour="blue"><ticks/></table>
    </table>
  </rom>
</roms>"#,
                expect: vec![
                    other(codes::CONTENT_IGNORED, "table"),
                    attr("colour", AXIS),
                    elem("ticks", AXIS),
                    other(codes::CONTENT_IGNORED, "table"),
                    attr("colour", AXIS),
                    elem("ticks", AXIS),
                    other(codes::CONTENT_IGNORED, "table"),
                    attr("colour", AXIS),
                    elem("ticks", AXIS),
                ],
            },
        ]
    }

    /// Reads `xml`; panics if the XML layer refuses it (every case is
    /// well-formed).
    fn read(xml: &str) -> Vec<Diagnostic> {
        romraider::read(SOURCE, xml.as_bytes()).into_diagnostics()
    }

    /// `(line, col)` of a text diagnostic.
    fn line_col(d: &Diagnostic) -> (u32, u32) {
        match d.provenance().span() {
            Span::LineCol { line, col } => (line, col),
            Span::ByteOffset(_) => panic!("a text diagnostic has a byte offset: {d}"),
        }
    }

    /// The text of `xml` from `line:col` (1-based; `col` counts characters)
    /// to the end of that line.
    fn text_at(xml: &str, line: u32, col: u32) -> String {
        let line = usize::try_from(line).unwrap();
        let col = usize::try_from(col).unwrap();
        let text = xml.split('\n').nth(line - 1).unwrap_or("");
        text.chars().skip(col - 1).collect()
    }

    /// One case, as the snapshot shows it.
    fn render(case: &Case) -> String {
        let mut out = String::new();
        writeln!(out, "=== {}", case.label).unwrap();
        for (n, line) in case.xml.split('\n').enumerate() {
            writeln!(out, "{:>4} | {line}", n + 1).unwrap();
        }
        let diags = read(case.xml);
        if diags.is_empty() {
            writeln!(out, "(no diagnostics)").unwrap();
        }
        for d in &diags {
            writeln!(out, "{d}").unwrap();
            let (line, col) = line_col(d);
            let source_line = case
                .xml
                .split('\n')
                .nth(usize::try_from(line).unwrap() - 1)
                .unwrap_or("");
            // The caret runs under the name: an attribute's up to `=`, an
            // element's from `<` to the end of its name.
            let rest = text_at(case.xml, line, col);
            let width = rest
                .char_indices()
                .find(|&(i, c)| c == '=' || c == '/' || c == '>' || (c.is_whitespace() && i > 0))
                .map_or(rest.chars().count(), |(i, _)| rest[..i].chars().count())
                .max(1);
            writeln!(out, "{line:>4} | {source_line}").unwrap();
            let pad = usize::try_from(col).unwrap() - 1;
            writeln!(out, "     | {}{}", " ".repeat(pad), "^".repeat(width)).unwrap();
        }
        out.push('\n');
        out
    }

    #[test]
    fn warnings_snapshot() {
        let rendered: String = cases().iter().map(render).collect();
        rr_testkit::snapshot::settings()
            .bind(|| insta::assert_snapshot!("unknown_warnings", rendered));
    }

    #[test]
    fn every_unknown_is_named_where_it_is() {
        let mut failures = Vec::new();
        for case in cases() {
            let diags = read(case.xml);
            let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
            if !errors.is_empty() {
                failures.push(format!("{}: errors: {errors:#?}", case.label));
                continue;
            }
            let mut warnings: Vec<&Diagnostic> = diags.iter().collect();
            assert!(
                warnings.iter().all(|d| d.severity() == Severity::Warning),
                "{}: a diagnostic that is not a warning: {diags:#?}",
                case.label
            );
            // Document order; the reader reports an element's attributes
            // before what it says about the element as a whole.
            warnings.sort_by_key(|d| line_col(d));
            if warnings.len() != case.expect.len() {
                let got: Vec<String> = warnings.iter().map(ToString::to_string).collect();
                failures.push(format!(
                    "{}: {} warning(s), expected {}: {got:#?}",
                    case.label,
                    warnings.len(),
                    case.expect.len()
                ));
                continue;
            }
            for (d, want) in warnings.iter().zip(&case.expect) {
                let (line, col) = line_col(d);
                let text = text_at(case.xml, line, col);
                let before = text_at(case.xml, line, col.saturating_sub(1).max(1));
                let points_at_name = match want.at {
                    At::Attribute(name) => {
                        // The name itself, whole: preceded by whitespace and
                        // followed by `=`.
                        col > 1
                            && before.starts_with(char::is_whitespace)
                            && text
                                .strip_prefix(name)
                                .is_some_and(|r| r.trim_start().starts_with('='))
                    }
                    At::Element(name) => text
                        .strip_prefix('<')
                        .and_then(|r| r.strip_prefix(name))
                        .is_some_and(|r| r.starts_with([' ', '/', '>'])),
                };
                if d.code() != want.code
                    || !points_at_name
                    || want.message.as_ref().is_some_and(|m| m != d.message())
                {
                    let at = match want.at {
                        At::Attribute(n) => format!("{n}="),
                        At::Element(n) => format!("<{n}"),
                    };
                    failures.push(format!(
                        "{}: got `{d}` (source there: {text:?}); expected {} at `{at}` \
                         with message {:?}",
                        case.label, want.code, want.message
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
