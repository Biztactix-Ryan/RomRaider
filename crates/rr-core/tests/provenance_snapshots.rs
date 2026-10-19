//! Golden snapshots of how a [`Provenance`] (and the [`Diagnostic`] line that
//! carries it) renders — US-RR-17-2.
//!
//! Text sources render as `file:line:col`; binary sources render as
//! `file@0xOFFSET`. Each snapshot is a table of `case => rendering` so a
//! formatting change shows up as one readable diff covering every edge case.
//!
//! The settings mirror `rr_testkit::snapshot::settings()` (module path
//! prepended, expression kept); rr-testkit is not used directly because it
//! depends on rr-core.

use rr_core::diag::{Code, Diagnostic, Provenance, Span};

fn settings() -> insta::Settings {
    let mut settings = insta::Settings::clone_current();
    settings.set_sort_maps(true);
    settings.set_prepend_module_to_snapshot(true);
    settings.set_omit_expression(false);
    settings
}

/// One `label => rendering` line per case, in the order given.
fn table(cases: &[(&str, Provenance)]) -> String {
    cases
        .iter()
        .map(|(label, provenance)| format!("{label:<28} => {provenance}\n"))
        .collect()
}

#[test]
fn text_sources_render_file_line_col() {
    let cases = [
        (
            "typical",
            Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7)),
        ),
        (
            "first position",
            Provenance::new("a.xml", Span::line_col(1, 1)),
        ),
        (
            "large line and column",
            Provenance::new("a.xml", Span::line_col(123_456, 789)),
        ),
        (
            "u32::MAX line and column",
            Provenance::new("a.xml", Span::line_col(u32::MAX, u32::MAX)),
        ),
        (
            "windows path",
            Provenance::new(r"C:\RomRaider\defs\ecu_defs.xml", Span::line_col(9, 3)),
        ),
        (
            "path with spaces",
            Provenance::new("my defs/user overrides.toml", Span::line_col(2, 14)),
        ),
        (
            "non-file label",
            Provenance::new("<clipboard>", Span::line_col(1, 5)),
        ),
        (
            "empty source name",
            Provenance::new("", Span::line_col(3, 4)),
        ),
        (
            "non-ascii source",
            Provenance::new("défs/ß.xml", Span::line_col(3, 4)),
        ),
        (
            "control chars escaped",
            Provenance::new("evil\n\t\u{1b}[31m.xml", Span::line_col(1, 2)),
        ),
    ];
    let rendered = table(&cases);
    settings().bind(|| insta::assert_snapshot!("text_sources", rendered));
}

#[test]
fn binary_sources_render_hex_offset() {
    let cases = [
        (
            "typical",
            Provenance::new("A2WC522N.bin", Span::byte_offset(0x1F3A0)),
        ),
        (
            "offset zero",
            Provenance::new("rom.bin", Span::byte_offset(0)),
        ),
        (
            "single digit",
            Provenance::new("rom.bin", Span::byte_offset(0xF)),
        ),
        (
            "end of 1 MiB image",
            Provenance::new("rom.bin", Span::byte_offset(0xF_FFFF)),
        ),
        (
            "past u32 range",
            Provenance::new("trace.rrlog", Span::byte_offset(0x1_0000_0000)),
        ),
        (
            "u64::MAX",
            Provenance::new("rom.bin", Span::byte_offset(u64::MAX)),
        ),
        (
            "windows path",
            Provenance::new(r"C:\roms\stock image.hex", Span::byte_offset(0x400)),
        ),
        (
            "non-file label",
            Provenance::new("<ecu-read>", Span::byte_offset(0x8000)),
        ),
        (
            "empty source name",
            Provenance::new("", Span::byte_offset(0x10)),
        ),
        (
            "control chars escaped",
            Provenance::new("rom\r\n.bin", Span::byte_offset(0x20)),
        ),
    ];
    let rendered = table(&cases);
    settings().bind(|| insta::assert_snapshot!("binary_sources", rendered));
}

#[test]
fn layer_and_rule_suffixes_follow_either_span_kind() {
    let text = || Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7));
    let binary = || Provenance::new("A2WC522N.bin", Span::byte_offset(0x1F3A0));
    let cases = [
        ("text, layer", text().with_layer("base")),
        ("text, rule", text().with_rule("quirk.ssm-delay")),
        (
            "text, layer and rule",
            text().with_layer("user").with_rule("quirk.ssm-delay"),
        ),
        ("binary, layer", binary().with_layer("vendor")),
        ("binary, rule", binary().with_rule("checksum.subaru-dbw")),
        (
            "binary, layer and rule",
            binary().with_layer("user").with_rule("checksum.subaru-dbw"),
        ),
        (
            "control chars in suffixes",
            text().with_layer("l\r").with_rule("r\u{1b}[0m"),
        ),
    ];
    let rendered = table(&cases);
    settings().bind(|| insta::assert_snapshot!("layer_and_rule", rendered));
}

#[test]
fn diagnostic_lines_lead_with_provenance() {
    let diags = [
        Diagnostic::error(
            Code::new("DEF0001"),
            "unknown scaling \"psi\"",
            Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7)).with_layer("base"),
        ),
        Diagnostic::warning(
            Code::new("ROM0003"),
            "table runs past image end",
            Provenance::new("A2WC522N.bin", Span::byte_offset(0x1F3A0)),
        ),
        Diagnostic::info(
            Code::new("DEF0100"),
            "deprecated attribute",
            Provenance::new("<clipboard>", Span::line_col(1, 1)).with_rule("compat.legacy"),
        ),
    ];
    let rendered: String = diags.iter().map(|d| format!("{d}\n")).collect();
    settings().bind(|| insta::assert_snapshot!("diagnostic_lines", rendered));
}
