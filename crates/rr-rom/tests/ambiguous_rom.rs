//! US-RR-34-2: a ROM image matching two definitions is refused with a
//! diagnostic listing both candidates, and no definition is applied
//! (US-RR-34, acceptance criterion 2; PROJECT.md EC-03).
//!
//! # What this prevents
//!
//! Legacy RomRaider took the first `<rom>` of the first definition file
//! whose `<romid>` matched the image (`editor/ecu/OpenImageWorker.java:219-250`;
//! within a file, `xml/DOMRomUnmarshaller.java:118-124`). Two definitions
//! matching one image (the same ROM in two files, as RomRaider's standard
//! and metric `ecu_defs.xml`; or two `<rom>`s of one file with one ID) went
//! unnoticed, and the image opened through whichever came first, possibly
//! the wrong one, its tables read and written at the wrong addresses.
//!
//! # How
//!
//! Exercised only through the public APIs, the way the editor opens a ROM
//! ([`open_like_the_editor`]): read the definition files into one
//! [`DefinitionSet`], index it ([`RomIdIndex`]), identify the image
//! ([`RomIdIndex::identify`]), resolve the one definition found
//! ([`DefinitionResolver`]) and open the image through it
//! ([`OpenRom::open`]). An ambiguous image must stop at the identify step:
//! no [`OpenRom`] is ever built, not even with [`Loaded::accept_errors`],
//! and the only diagnostic is one [`AMBIGUOUS_ROM_ID`](codes::AMBIGUOUS_ROM_ID)
//! Error naming every candidate's `xmlid` and where its `<rom>` is
//! (`file:line:col [layer id]`). Covered: two `<rom>`s in one file, the
//! same ROM in two files, three candidates (all listed), and the
//! non-ambiguous neighbours: a near miss (same address, different bytes)
//! and a `<rom>` left out of the index (no identification, or ignored as a
//! repeat) open the one real match normally.
//!
//! The rendered diagnostics are pinned in one insta snapshot
//! ([`ambiguity_snapshot`]); the per-case tests assert the same facts
//! without trusting it. The index's own unit tests
//! (`rr-defs/src/resolve/index/tests.rs`, US-RR-34-7) cover the matching
//! itself (prefix IDs, overlapping IDs, `force`, IDs past the image end).

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod ambiguous_rom {
    use std::fmt::Write as _;

    use rr_core::diag::{Code, Diagnostic, Loaded, Span, codes};
    use rr_defs::resolve::{DefinitionResolver, RomIdIndex, RomMatch};
    use rr_defs::romraider::DefinitionSet;
    use rr_rom::image::Image;
    use rr_rom::open::OpenRom;

    /// The name every image is opened as.
    const IMAGE: &str = "rom.bin";

    /// The size every definition declares and every image has.
    const SIZE: usize = 0x400;

    /// One `<rom>` on its own line (at column 3), identified by `id` at
    /// address 0x100 and declaring a 1 KiB image.
    fn rom(xmlid: &str, id: &str) -> String {
        format!(
            "  <rom><romid><xmlid>{xmlid}</xmlid>\
             <internalidaddress>100</internalidaddress>\
             <internalidstring>{id}</internalidstring>\
             <filesize>1kb</filesize></romid></rom>"
        )
    }

    /// A definition file of `roms`, one per line: the `n`th (from 0) is at
    /// line `n + 2`, column 3.
    fn file(roms: &[String]) -> String {
        let mut xml = String::from("<roms>\n");
        for r in roms {
            xml.push_str(r);
            xml.push('\n');
        }
        xml.push_str("</roms>\n");
        xml
    }

    /// A 1 KiB image holding `id` at 0x100.
    fn image(id: &[u8]) -> Vec<u8> {
        let mut image = vec![0; SIZE];
        image[0x100..0x100 + id.len()].copy_from_slice(id);
        image
    }

    /// Reads `files` (`(source, xml)`) into one set, as a user's list of
    /// definition files is loaded. Returns the set and the reader's codes.
    fn set_of(files: &[(&str, String)]) -> (DefinitionSet, Vec<Code>) {
        let (set, diags) =
            DefinitionSet::read_all(files.iter().map(|(s, x)| (*s, x.as_bytes()))).accept_errors();
        (set, codes_of(&diags))
    }

    /// The code of each diagnostic.
    fn codes_of(diags: &[Diagnostic]) -> Vec<Code> {
        diags.iter().map(Diagnostic::code).collect()
    }

    /// Opens `bytes` against `set` the way the editor does: find the
    /// image's definition in the ROM-ID index, refused when ambiguous;
    /// resolve it; open the image through it. `None` when no definition
    /// is applied.
    fn open_like_the_editor(set: &DefinitionSet, bytes: &[u8]) -> Loaded<Option<OpenRom>> {
        let index = RomIdIndex::new(set);
        let (found, mut diags) = index.identify(bytes, IMAGE).accept_errors();
        let Some(candidate) = found else {
            return Loaded::new(None, diags);
        };
        let (definition, resolved) = DefinitionResolver::new(set)
            .resolve(candidate.rom())
            .accept_errors();
        diags.extend(resolved);
        let Some(definition) = definition else {
            return Loaded::new(None, diags);
        };
        let (rom, opened) =
            OpenRom::open(definition, Image::new(bytes.to_vec()), IMAGE).accept_errors();
        diags.extend(opened);
        Loaded::new(Some(rom), diags)
    }

    /// Asserts `bytes` is refused against `set`: no definition applied,
    /// whichever way the caller asks, and exactly one `AMBIGUOUS_ROM_ID`
    /// Error, at the image's ID address, naming each of `expected`
    /// (`` `xmlid` (file:line:col [layer id] ``) in that order. Returns the
    /// diagnostic.
    fn assert_refused(set: &DefinitionSet, bytes: &[u8], expected: &[&str]) -> Diagnostic {
        // The index sees every candidate and picks none.
        let index = RomIdIndex::new(set);
        let RomMatch::Ambiguous(all) = index.lookup(bytes) else {
            panic!("not ambiguous: {:?}", index.lookup(bytes));
        };
        assert_eq!(all.len(), expected.len());
        assert_eq!(index.lookup(bytes).unique(), None);

        // Opened as the editor would: refused, and still nothing when the
        // caller accepts the error.
        let loaded = open_like_the_editor(set, bytes);
        assert!(loaded.has_errors());
        assert_eq!(loaded.error_count(), 1);
        assert!(loaded.value().is_none());
        let (rom, diags) = loaded.accept_errors();
        assert!(rom.is_none(), "a definition was applied: {rom:?}");
        assert_eq!(codes_of(&diags), [codes::AMBIGUOUS_ROM_ID]);

        let diag = diags.into_iter().next().unwrap();
        assert!(diag.is_error());
        assert_eq!(diag.provenance().source(), IMAGE);
        assert_eq!(diag.provenance().span(), Span::byte_offset(0x100));
        let message = diag.message();
        assert!(
            message.contains(&format!(
                "matches {} definitions, so none is applied",
                expected.len()
            )),
            "{message}"
        );
        // Every candidate, with where it is, in set order.
        let mut from = 0;
        for want in expected {
            let at = message[from..]
                .find(want)
                .unwrap_or_else(|| panic!("{want} not (in order) in: {message}"));
            from += at + want.len();
        }
        diag
    }

    /// Asserts `bytes` opens against `set` through `xmlid` alone, editable,
    /// with no diagnostic.
    fn assert_opens_through(set: &DefinitionSet, bytes: &[u8], xmlid: &str) {
        let index = RomIdIndex::new(set);
        let RomMatch::Unique(c) = index.lookup(bytes) else {
            panic!("not unique: {:?}", index.lookup(bytes));
        };
        assert_eq!(c.xmlid(), xmlid);
        let (rom, diags) = open_like_the_editor(set, bytes).into_result().unwrap();
        assert_eq!(codes_of(&diags), []);
        let mut rom = rom.unwrap();
        assert_eq!(rom.definition().xmlid(), xmlid);
        assert!(rom.edit().is_ok());
    }

    /// Two `<rom>`s of one file with one ROM ID.
    fn two_in_one_file() -> Vec<(&'static str, String)> {
        vec![(
            "defs.xml",
            file(&[rom("ROM A", "A2WC522N"), rom("ROM A REV2", "A2WC522N")]),
        )]
    }

    /// The same ROM (one `xmlid`, one ID) in two files, as RomRaider's
    /// standard and metric definitions.
    fn same_rom_in_two_files() -> Vec<(&'static str, String)> {
        vec![
            ("standard.xml", file(&[rom("A2WC522N", "A2WC522N")])),
            ("metric.xml", file(&[rom("A2WC522N", "A2WC522N")])),
        ]
    }

    /// Three candidates: two `<rom>`s of one file and a third in another.
    fn three_candidates() -> Vec<(&'static str, String)> {
        vec![
            (
                "standard.xml",
                file(&[
                    rom("OTHER", "B3XD100A"),
                    rom("A2WC522N", "A2WC522N"),
                    rom("A2WC522N TEST", "A2WC522N"),
                ]),
            ),
            ("metric.xml", file(&[rom("A2WC522N", "A2WC522N")])),
        ]
    }

    #[test]
    fn two_roms_of_one_file_matching_one_image_are_refused() {
        let (set, read) = set_of(&two_in_one_file());
        assert_eq!(read, []);
        assert_refused(
            &set,
            &image(b"A2WC522N"),
            &[
                "`ROM A` (defs.xml:2:3 [layer ROM A], ID at 0x100)",
                "`ROM A REV2` (defs.xml:3:3 [layer ROM A REV2], ID at 0x100)",
            ],
        );
    }

    #[test]
    fn the_same_rom_in_two_files_is_refused() {
        // Legacy opened it through the first file listed. Reading the two
        // only warns: both are kept, each serving its own file.
        let (set, read) = set_of(&same_rom_in_two_files());
        assert_eq!(read, [codes::DUPLICATE_XMLID]);
        assert_refused(
            &set,
            &image(b"A2WC522N"),
            &[
                "`A2WC522N` (standard.xml:2:3 [layer A2WC522N], ID at 0x100)",
                "`A2WC522N` (metric.xml:2:3 [layer A2WC522N@metric.xml], ID at 0x100)",
            ],
        );
        // Either file alone opens it.
        for (source, xml) in same_rom_in_two_files() {
            let (set, _) = set_of(&[(source, xml)]);
            assert_opens_through(&set, &image(b"A2WC522N"), "A2WC522N");
        }
    }

    #[test]
    fn three_candidates_are_all_listed() {
        let (set, read) = set_of(&three_candidates());
        // `A2WC522N` again in metric.xml: a Warning, both kept.
        assert_eq!(read, [codes::DUPLICATE_XMLID]);
        assert_refused(
            &set,
            &image(b"A2WC522N"),
            &[
                "`A2WC522N` (standard.xml:3:3 [layer A2WC522N], ID at 0x100)",
                "`A2WC522N TEST` (standard.xml:4:3 [layer A2WC522N TEST], ID at 0x100)",
                "`A2WC522N` (metric.xml:2:3 [layer A2WC522N@metric.xml], ID at 0x100)",
            ],
        );
        // The definition with another ID is no candidate, and is the one
        // match of its own image.
        assert_opens_through(&set, &image(b"B3XD100A"), "OTHER");
    }

    #[test]
    fn a_near_miss_is_not_ambiguous() {
        // Same address, IDs differing in one byte: each image is its own.
        let (set, read) = set_of(&[(
            "defs.xml",
            file(&[rom("ROM A", "A2WC522N"), rom("ROM B", "A2WC522P")]),
        )]);
        assert_eq!(read, []);
        assert_opens_through(&set, &image(b"A2WC522N"), "ROM A");
        assert_opens_through(&set, &image(b"A2WC522P"), "ROM B");
        // Neither: nothing applied, and nothing ambiguous about it.
        let (rom, diags) = open_like_the_editor(&set, &image(b"A2WC522X"))
            .into_result()
            .unwrap();
        assert!(rom.is_none());
        assert_eq!(codes_of(&diags), []);
    }

    #[test]
    fn a_rom_left_out_of_the_index_causes_no_ambiguity() {
        let xml = file(&[
            // A base with no ID: never a candidate.
            "  <rom><romid><xmlid>BASE</xmlid><filesize>1kb</filesize></romid></rom>".to_owned(),
            // An address but no string (DEF0013): never a candidate.
            "  <rom><romid><xmlid>NO STRING</xmlid><internalidaddress>100</internalidaddress></romid></rom>"
                .to_owned(),
            r#"  <rom base="BASE"><romid><xmlid>ROM A</xmlid><internalidaddress>100</internalidaddress><internalidstring>A2WC522N</internalidstring></romid></rom>"#
                .to_owned(),
            // A repeat of a name in one file, same ID: ignored (DEF0026).
            rom("ROM A", "A2WC522N"),
        ]);
        let (set, read) = set_of(&[("defs.xml", xml)]);
        assert_eq!(
            read,
            [codes::ROM_ID_INCOMPLETE, codes::DUPLICATE_XMLID],
            "the reader reports what it leaves out"
        );
        let index = RomIdIndex::new(&set);
        let names: Vec<&str> = index.candidates().iter().map(|c| c.xmlid()).collect();
        assert_eq!(names, ["ROM A"]);
        assert_opens_through(&set, &image(b"A2WC522N"), "ROM A");
    }

    /// One case, as the snapshot shows it: each file, line numbered, the
    /// reader's diagnostics, then those of opening the image.
    fn render(label: &str, files: &[(&str, String)], id: &[u8]) -> String {
        let mut out = String::new();
        writeln!(out, "=== {label}").unwrap();
        for (source, xml) in files {
            writeln!(out, "--- {source}").unwrap();
            for (n, line) in xml.lines().enumerate() {
                writeln!(out, "{:>4} | {line}", n + 1).unwrap();
            }
        }
        let (set, read) =
            DefinitionSet::read_all(files.iter().map(|(s, x)| (*s, x.as_bytes()))).accept_errors();
        writeln!(out, "--- read").unwrap();
        for d in &read {
            writeln!(out, "{d}").unwrap();
        }
        let (rom, diags) = open_like_the_editor(&set, &image(id)).accept_errors();
        writeln!(
            out,
            "--- {IMAGE} (ID {:?} at 0x100): {}",
            String::from_utf8_lossy(id),
            rom.map_or_else(
                || "no definition applied".to_owned(),
                |r| format!("opened through `{}`", r.definition().xmlid())
            ),
        )
        .unwrap();
        for d in &diags {
            writeln!(out, "{d}").unwrap();
        }
        out.push('\n');
        out
    }

    #[test]
    fn ambiguity_snapshot() {
        let rendered = [
            render("two <rom>s of one file", &two_in_one_file(), b"A2WC522N"),
            render(
                "the same ROM in two files",
                &same_rom_in_two_files(),
                b"A2WC522N",
            ),
            render("three candidates", &three_candidates(), b"A2WC522N"),
        ]
        .concat();
        rr_testkit::snapshot::settings()
            .bind(|| insta::assert_snapshot!("ambiguous_rom", rendered));
    }
}
