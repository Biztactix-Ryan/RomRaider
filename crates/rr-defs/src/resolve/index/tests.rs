//! Unit tests of the ROM-ID index (US-RR-34-7): unique, none and
//! ambiguous matches (EC-03), IDs at the end of or past the image, what is
//! left out of the index, and agreement with matching each definition one
//! by one.

use proptest::prelude::*;
use rr_core::diag::{Code, Diagnostic, Span, codes};

use super::{Candidate, RomIdIndex, RomMatch};
use crate::resolve::DefinitionResolver;
use crate::romraider::DefinitionSet;

/// The index can be shared across threads: one per set version serves
/// every open.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<RomIdIndex>();
    send_sync::<Candidate>();
    send_sync::<RomMatch<'static>>();
};

/// A `<rom>` named `xmlid`, identified by `id` at `address` (both as
/// written in the XML; `None` for no `<internalid...>` at all).
fn rom(xmlid: &str, id: Option<(&str, &str)>) -> String {
    let id = id.map_or_else(String::new, |(address, id)| {
        format!("<internalidaddress>{address}</internalidaddress><internalidstring>{id}</internalidstring>")
    });
    format!("<rom><romid><xmlid>{xmlid}</xmlid>{id}</romid></rom>")
}

/// A file of `roms`.
fn file(roms: &[String]) -> String {
    format!("<roms>{}</roms>", roms.concat())
}

/// The set of `files` (`(source, xml)`), accepting errors.
fn set_of(files: &[(&str, String)]) -> DefinitionSet {
    let (set, _) =
        DefinitionSet::read_all(files.iter().map(|(s, x)| (*s, x.as_bytes()))).accept_errors();
    set
}

/// The set of one file `defs.xml` of `roms`.
fn one_file(roms: &[String]) -> DefinitionSet {
    set_of(&[("defs.xml", file(roms))])
}

/// The `xmlid`s `image` matches in `index`, in the order returned, and
/// whether the match is ambiguous.
fn hits(index: &RomIdIndex, image: &[u8]) -> Vec<String> {
    match index.lookup(image) {
        RomMatch::None => vec![],
        RomMatch::Unique(c) => vec![c.xmlid().to_owned()],
        RomMatch::Ambiguous(all) => {
            assert!(all.len() >= 2);
            all.iter().map(|c| c.xmlid().to_owned()).collect()
        }
    }
}

/// The code of each diagnostic.
fn codes_of(diags: &[Diagnostic]) -> Vec<Code> {
    diags.iter().map(Diagnostic::code).collect()
}

/// `len` bytes of `.` with `id` written at `at`.
fn image_with(len: usize, at: usize, id: &[u8]) -> Vec<u8> {
    let mut image = vec![b'.'; len];
    image[at..at + id.len()].copy_from_slice(id);
    image
}

#[test]
fn a_unique_match_names_the_rom_to_resolve() {
    let set = one_file(&[
        rom("BASE", None),
        rom("ROM A", Some(("10", "ROMA"))),
        rom("ROM B", Some(("10", "ROMB"))),
    ]);
    let index = RomIdIndex::new(&set);
    let image = image_with(0x40, 0x10, b"ROMB");

    let RomMatch::Unique(c) = index.lookup(&image) else {
        panic!("{:?}", index.lookup(&image));
    };
    assert_eq!(c.xmlid(), "ROM B");
    assert_eq!(c.rom(), set.find("ROM B").unwrap());
    assert_eq!(c.at().source(), "defs.xml");
    assert_eq!(c.at().layer(), Some("ROM B"));
    assert!(c.identification().matches(&image));
    assert_eq!(index.lookup(&image).unique(), Some(c));

    // The match resolves to the definition of that name.
    let def = DefinitionResolver::new(&set)
        .resolve(c.rom())
        .into_result()
        .unwrap()
        .0
        .unwrap();
    assert_eq!(def.xmlid(), "ROM B");

    let (found, diags) = index.identify(&image, "rom.bin").into_result().unwrap();
    assert_eq!(found, Some(c));
    assert_eq!(codes_of(&diags), []);
}

#[test]
fn text_ids_ignore_ascii_case_and_hex_ids_compare_exactly() {
    let set = one_file(&[
        rom("TEXT", Some(("0", "AbC1"))),
        rom("HEX", Some(("8", "0x0A0B"))),
    ]);
    let index = RomIdIndex::new(&set);
    assert_eq!(hits(&index, b"aBc1......"), ["TEXT"]);
    assert_eq!(hits(&index, b"ABC1......"), ["TEXT"]);
    assert_eq!(hits(&index, b"ABC2......"), Vec::<String>::new());
    assert_eq!(hits(&index, b"........\x0A\x0B"), ["HEX"]);
    // 0x2A differs from 0x0A only in the bit a text letter ignores.
    assert_eq!(hits(&index, b"........\x2A\x0B"), Vec::<String>::new());
}

#[test]
fn an_image_no_definition_matches_finds_none_without_a_diagnostic() {
    let set = one_file(&[rom("ROM A", Some(("4", "ROMA")))]);
    let index = RomIdIndex::new(&set);
    let image = image_with(16, 4, b"ROMX");
    assert_eq!(index.lookup(&image), RomMatch::None);
    assert_eq!(index.lookup(&image).unique(), None);

    let (found, diags) = index.identify(&image, "rom.bin").into_result().unwrap();
    assert_eq!(found, None);
    assert_eq!(codes_of(&diags), []);
}

#[test]
fn two_definitions_with_one_id_are_ambiguous_and_none_is_applied() {
    // The same ROM in two files (as RomRaider's standard and metric
    // files), and a second name with the same ID in the first file.
    let set = set_of(&[
        (
            "standard.xml",
            file(&[
                rom("A2WC522N", Some(("2000", "A2WC522N"))),
                rom("A2WC522N copy", Some(("0x2000", "a2wc522n"))),
            ]),
        ),
        (
            "metric.xml",
            file(&[rom("A2WC522N", Some(("2000", "A2WC522N")))]),
        ),
    ]);
    let index = RomIdIndex::new(&set);
    assert_eq!(index.candidates().len(), 3);
    let image = image_with(0x3000, 0x2000, b"A2WC522N");

    let RomMatch::Ambiguous(all) = index.lookup(&image) else {
        panic!("{:?}", index.lookup(&image));
    };
    let places: Vec<(&str, &str, Option<&str>)> = all
        .iter()
        .map(|c| (c.xmlid(), c.at().source(), c.at().layer()))
        .collect();
    assert_eq!(
        places,
        [
            ("A2WC522N", "standard.xml", Some("A2WC522N")),
            ("A2WC522N copy", "standard.xml", Some("A2WC522N COPY")),
            ("A2WC522N", "metric.xml", Some("A2WC522N@metric.xml")),
        ]
    );
    assert_eq!(index.lookup(&image).unique(), None);

    // Refused: an Error naming every candidate and where it is, no value.
    let loaded = index.identify(&image, "rom.bin");
    assert!(loaded.has_errors());
    assert_eq!(loaded.value(), None);
    let (found, diags) = loaded.accept_errors();
    assert_eq!(found, None);
    assert_eq!(codes_of(&diags), [codes::AMBIGUOUS_ROM_ID]);
    let diag = &diags[0];
    assert!(diag.is_error());
    assert_eq!(diag.provenance().source(), "rom.bin");
    assert_eq!(diag.provenance().span(), Span::byte_offset(0x2000));
    let message = diag.message();
    assert!(message.contains("matches 3 definitions"), "{message}");
    for (xmlid, source) in [
        ("`A2WC522N` (standard.xml:1:", "standard.xml"),
        ("`A2WC522N copy` (standard.xml:1:", "standard.xml"),
        ("`A2WC522N` (metric.xml:1:", "metric.xml"),
    ] {
        assert!(message.contains(xmlid), "{source}: {message}");
    }
    assert!(message.contains("ID at 0x2000"), "{message}");
}

#[test]
fn overlapping_ids_at_different_addresses_are_ambiguous_only_when_both_match() {
    // B's ID starts inside A's: one image can hold both.
    let set = one_file(&[rom("A", Some(("0", "ABCD"))), rom("B", Some(("2", "CDEF")))]);
    let index = RomIdIndex::new(&set);
    assert_eq!(hits(&index, b"ABCDEF"), ["A", "B"]);
    assert_eq!(hits(&index, b"ABCDxx"), ["A"]);
    assert_eq!(hits(&index, b"xxCDEF"), ["B"]);
    assert!(index.identify(b"ABCDEF", "rom.bin").has_errors());
}

#[test]
fn an_id_that_is_a_prefix_of_another_at_one_address_is_ambiguous() {
    // The pinned corpus has IDs ending in spaces (`A2WC50  `).
    let set = one_file(&[
        rom("SHORT", Some(("4", "A2WC50"))),
        rom("LONG", Some(("4", "A2WC50  "))),
    ]);
    let index = RomIdIndex::new(&set);
    assert_eq!(hits(&index, b"....A2WC50  ...."), ["SHORT", "LONG"]);
    assert_eq!(hits(&index, b"....A2WC50XX...."), ["SHORT"]);
}

#[test]
fn an_id_ending_at_the_last_byte_matches_and_one_past_the_end_does_not() {
    let set = one_file(&[rom("END", Some(("C", "TAIL")))]);
    let index = RomIdIndex::new(&set);
    // 16 bytes: the ID's last byte is the image's last.
    assert_eq!(hits(&index, b"............TAIL"), ["END"]);
    // One byte short: the ID would end past the image.
    assert_eq!(hits(&index, b"............TAI"), Vec::<String>::new());
    // Starting past the end.
    assert_eq!(hits(&index, b"....."), Vec::<String>::new());
}

#[test]
fn an_id_at_the_largest_address_never_panics() {
    let set = one_file(&[rom("FAR", Some(("FFFFFFF0", "FAR")))]);
    let index = RomIdIndex::new(&set);
    assert_eq!(index.candidates().len(), 1);
    assert_eq!(hits(&index, &[0; 64]), Vec::<String>::new());
    assert_eq!(hits(&index, &[]), Vec::<String>::new());
}

#[test]
fn an_empty_image_matches_nothing_but_force() {
    let set = one_file(&[rom("A", Some(("0", "A")))]);
    let index = RomIdIndex::new(&set);
    assert_eq!(index.lookup(&[]), RomMatch::None);

    let set = one_file(&[
        rom("A", Some(("0", "A"))),
        rom("ANY", Some(("-1", "Force"))),
    ]);
    let index = RomIdIndex::new(&set);
    assert_eq!(hits(&index, &[]), ["ANY"]);
}

#[test]
fn force_is_ambiguous_with_any_other_match() {
    let set = one_file(&[
        rom("ANY", Some(("-1", "force"))),
        rom("A", Some(("0", "A"))),
    ]);
    let index = RomIdIndex::new(&set);
    assert_eq!(hits(&index, b"A..."), ["ANY", "A"]);
    assert_eq!(hits(&index, b"B..."), ["ANY"]);
    let (_, diags) = index.identify(b"A...", "rom.bin").accept_errors();
    let message = diags[0].message();
    assert!(message.contains("force: matches every image"), "{message}");
    // At the address of the one byte ID.
    assert_eq!(diags[0].provenance().span(), Span::byte_offset(0));
}

#[test]
fn definitions_without_an_identification_are_not_indexed() {
    let set = one_file(&[
        // A base: no ID at all.
        rom("BASE", None),
        // An address with no string never matches (DEF0013).
        "<rom><romid><xmlid>NO STRING</xmlid><internalidaddress>0</internalidaddress></romid></rom>"
            .to_owned(),
        // -1 is only for `force` (DEF0013).
        rom("MINUS ONE", Some(("-1", "ABC"))),
        // Not hex bytes (DEF0012).
        rom("BAD HEX", Some(("0", "0xZZ"))),
        rom("A", Some(("0", "A"))),
        // A repeat of a name in one file is ignored (DEF0026).
        rom("A", Some(("0", "B"))),
    ]);
    let index = RomIdIndex::new(&set);
    let names: Vec<&str> = index.candidates().iter().map(Candidate::xmlid).collect();
    assert_eq!(names, ["A"]);
    assert_eq!(hits(&index, b"B"), Vec::<String>::new());
    assert_eq!(hits(&index, b"ABC"), ["A"]);
}

#[test]
fn an_empty_set_matches_nothing() {
    let index = RomIdIndex::new(&set_of(&[]));
    assert_eq!(index.candidates(), []);
    assert_eq!(index.lookup(b"anything"), RomMatch::None);
}

/// A ROM ID: an address and text over a small alphabet, so random IDs
/// and images often collide and overlap.
fn id_strategy() -> impl Strategy<Value = (u8, String)> {
    (0u8..6, "[aAbB0]{1,3}")
}

proptest! {
    /// The index finds exactly the definitions whose identification
    /// matches the image one by one, in set order.
    #[test]
    fn lookup_agrees_with_matching_every_definition(
        ids in proptest::collection::vec(id_strategy(), 0..8),
        image in "[aAbB0.]{0,10}",
    ) {
        let roms: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(i, (address, id))| rom(&format!("R{i}"), Some((&format!("{address:X}"), id))))
            .collect();
        let set = one_file(&roms);
        let index = RomIdIndex::new(&set);
        let image = image.as_bytes();
        let expected: Vec<String> = set
            .roms()
            .filter(|(_, r)| r.identification().is_some_and(|i| i.matches(image)))
            .map(|(_, r)| r.xmlid().to_owned())
            .collect();
        prop_assert_eq!(hits(&index, image), expected.clone());
        let loaded = index.identify(image, "rom.bin");
        prop_assert_eq!(loaded.has_errors(), expected.len() > 1);
    }
}
