//! Unit tests of opening a ROM through its definition (US-RR-34-8): an
//! image of the declared size opens editable; a longer or shorter one is a
//! `ROM_SIZE_MISMATCH` Error naming both sizes and the definition, and
//! every edit is refused; a definition with no declared size is not
//! checked.

use rr_core::diag::{Severity, Span, codes};
use rr_defs::resolve::{Definition, DefinitionResolver};
use rr_defs::romraider::DefinitionSet;

use super::{EditingDisabled, OpenRom, Size};
use crate::image::Image;
use crate::journal::{Journal, Origin, Patch};

/// The definition `ROM A`, with `filesize` as written (`None`: no
/// `<filesize>` element), or inheriting it from a base when `inherited`.
fn definition(filesize: Option<&str>, inherited: bool) -> Definition {
    let size = filesize.map_or_else(String::new, |s| format!("<filesize>{s}</filesize>"));
    let (base_size, own_size) = if inherited {
        (size, String::new())
    } else {
        (String::new(), size)
    };
    let xml = format!(
        "<roms>\
           <rom><romid><xmlid>BASE</xmlid>{base_size}</romid></rom>\
           <rom base=\"BASE\"><romid><xmlid>ROM A</xmlid>\
             <internalidaddress>0</internalidaddress>\
             <internalidstring>ROMA</internalidstring>{own_size}</romid></rom>\
         </roms>"
    );
    let (set, _) = DefinitionSet::read_all([("defs.xml", xml.as_bytes())])
        .into_result()
        .unwrap();
    let rom = set.find("rom a").unwrap();
    DefinitionResolver::new(&set)
        .resolve(rom)
        .into_result()
        .unwrap()
        .0
        .unwrap()
}

/// An image of `len` zero bytes.
fn image(len: usize) -> Image {
    Image::new(vec![0; len])
}

/// A one-byte edit at 0 of `rom`'s image.
fn edit_at_0(rom: &OpenRom) -> Patch {
    Patch::replacing(rom.image(), 0, vec![0xAA], Origin::Edit).unwrap()
}

#[test]
fn matching_size_opens_editable_with_no_diagnostic() {
    let loaded = OpenRom::open(definition(Some("1kb"), false), image(1024), "a.bin");
    assert!(loaded.diagnostics().is_empty());
    let (mut rom, _) = loaded.into_result().unwrap();
    assert!(rom.is_editable());
    assert_eq!(rom.editing_disabled(), None);
    assert_eq!(rom.source(), "a.bin");
    assert_eq!(rom.definition().xmlid(), "ROM A");

    let patch = edit_at_0(&rom);
    let mut editor = rom.edit().unwrap();
    editor.apply(patch).unwrap();
    assert_eq!(editor.applied().len(), 1);
    assert_eq!(editor.image().bytes()[0], 0xAA);
    editor.undo().unwrap();
    assert_eq!(rom.image().bytes()[0], 0);
    rom.edit().unwrap().redo().unwrap();
    assert_eq!(rom.image().bytes()[0], 0xAA);
    rom.edit().unwrap().undo_all().unwrap();
    assert!(!rom.image().is_modified());
    rom.edit().unwrap().redo_all().unwrap();
    assert!(rom.image().is_modified());
    assert_eq!(rom.journal().applied().len(), 1);
}

#[test]
fn inherited_size_is_checked_as_its_own() {
    let ok = OpenRom::open(definition(Some("2kb"), true), image(2048), "a.bin");
    assert!(ok.diagnostics().is_empty());
    let bad = OpenRom::open(definition(Some("2kb"), true), image(1024), "a.bin");
    assert!(bad.has_errors());
}

#[test]
fn size_units_are_compared_in_bytes() {
    for (filesize, len) in [
        ("1kb", 1024),
        ("1KB", 1024),
        ("1mb", 1 << 20),
        ("1000b", 1000),
    ] {
        let ok = OpenRom::open(definition(Some(filesize), false), image(len), "a.bin");
        assert!(ok.diagnostics().is_empty(), "{filesize} = {len} bytes");
    }
    // "1kb" is 1024 bytes, not 1000.
    let bad = OpenRom::open(definition(Some("1kb"), false), image(1000), "a.bin");
    assert!(bad.has_errors());
}

/// A mismatched open: the one Error, then the ROM accepted read-only.
fn open_mismatched(declared: &str, len: usize) -> (OpenRom, rr_core::diag::Diagnostic) {
    let loaded = OpenRom::open(definition(Some(declared), false), image(len), "rom.bin");
    assert!(loaded.has_errors());
    assert_eq!(loaded.error_count(), 1);
    assert!(loaded.value().is_none(), "the Error withholds the ROM");
    let (rom, mut diagnostics) = loaded.accept_errors();
    assert_eq!(diagnostics.len(), 1);
    (rom, diagnostics.remove(0))
}

#[test]
fn larger_image_is_an_error_and_read_only() {
    let (mut rom, d) = open_mismatched("1kb", 1536);
    assert_eq!(d.code(), codes::ROM_SIZE_MISMATCH);
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(d.provenance().source(), "rom.bin");
    assert_eq!(d.provenance().span(), Span::byte_offset(1024));
    assert_eq!(
        d.message(),
        "the image is 1536 bytes but definition `ROM A` declares a file size of \
         1024 bytes (1 KiB) (512 bytes too long), so it may not be this ROM or \
         not all of it: opened read-only, editing is disabled"
    );
    let why = EditingDisabled::SizeMismatch {
        declared: 1024,
        actual: 1536,
    };
    assert!(!rom.is_editable());
    assert_eq!(rom.editing_disabled(), Some(why));
    assert_eq!(rom.edit().err(), Some(why));
}

#[test]
fn smaller_image_is_an_error_and_read_only() {
    let (mut rom, d) = open_mismatched("1kb", 512);
    assert_eq!(d.code(), codes::ROM_SIZE_MISMATCH);
    assert_eq!(d.provenance().span(), Span::byte_offset(512));
    assert!(
        d.message().contains("the image is 512 bytes"),
        "{}",
        d.message()
    );
    assert!(
        d.message().contains("1024 bytes (1 KiB)"),
        "{}",
        d.message()
    );
    assert!(d.message().contains("(512 bytes short)"), "{}", d.message());
    assert!(d.message().contains("`ROM A`"), "{}", d.message());
    assert_eq!(
        rom.edit().err(),
        Some(EditingDisabled::SizeMismatch {
            declared: 1024,
            actual: 512,
        })
    );
    assert!(!rom.image().is_modified());
}

#[test]
fn one_byte_off_either_way_is_a_mismatch() {
    for len in [1023, 1025] {
        let (rom, d) = open_mismatched("1kb", len);
        assert_eq!(d.code(), codes::ROM_SIZE_MISMATCH);
        assert!(!rom.is_editable(), "{len}");
    }
}

#[test]
fn empty_image_is_a_mismatch() {
    let (rom, d) = open_mismatched("1kb", 0);
    assert_eq!(d.provenance().span(), Span::byte_offset(0));
    assert!(
        d.message().contains("the image is 0 bytes"),
        "{}",
        d.message()
    );
    assert!(!rom.is_editable());
}

#[test]
fn no_declared_size_is_not_checked() {
    for len in [0, 1, 1024, 1 << 20] {
        let loaded = OpenRom::open(definition(None, false), image(len), "a.bin");
        assert!(loaded.diagnostics().is_empty(), "{len}");
        let (rom, _) = loaded.into_result().unwrap();
        assert!(rom.is_editable(), "{len}");
    }
}

#[test]
fn a_restored_journal_is_checked_by_its_image_size() {
    let mut journal = Journal::new(image(1024));
    journal
        .apply(Patch::new(0, vec![0], vec![1], Origin::Edit).unwrap())
        .unwrap();
    let ok = OpenRom::open(definition(Some("1kb"), false), journal.clone(), "a.bin");
    let (rom, _) = ok.into_result().unwrap();
    assert_eq!(rom.journal(), &journal);

    let bad = OpenRom::open(definition(Some("2kb"), false), journal, "a.bin");
    let (mut rom, _) = bad.accept_errors();
    // Not even an undo of the restored history.
    assert!(rom.edit().is_err());
    assert!(rom.journal().can_undo());
}

#[test]
fn editing_disabled_displays_both_sizes() {
    let why = EditingDisabled::SizeMismatch {
        declared: 1024,
        actual: 512,
    };
    assert_eq!(
        why.to_string(),
        "editing is disabled: the image is 512 bytes, the definition declares 1024"
    );
}

#[test]
fn sizes_show_kib_only_when_exact() {
    assert_eq!(Size(0).to_string(), "0 bytes");
    assert_eq!(Size(512).to_string(), "512 bytes");
    assert_eq!(Size(1023).to_string(), "1023 bytes");
    assert_eq!(Size(1024).to_string(), "1024 bytes (1 KiB)");
    assert_eq!(Size(1025).to_string(), "1025 bytes");
    assert_eq!(Size(524_288).to_string(), "524288 bytes (512 KiB)");
}

#[test]
fn the_largest_declared_size_never_overflows() {
    // 4095mb is the largest size a u32 holds in mb.
    let (rom, d) = open_mismatched("4095mb", 16);
    assert!(d.message().contains("4293918720 bytes"), "{}", d.message());
    assert!(!rom.is_editable());
}
