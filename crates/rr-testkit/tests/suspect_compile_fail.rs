//! US-RR-11-2: tests consuming the corpus must handle suspect entries
//! explicitly. Compile-fail (trybuild) cases show that, from outside
//! rr-testkit, a suspect image's bytes cannot be used as a raw image without
//! matching [`Image`]:
//!
//! * [`Corpus::read`] returns an [`Image`], which is not a `Vec<u8>` and
//!   cannot be dereferenced, indexed or `as_ref`ed as bytes
//!   (`compile_fail/suspect/read_as_raw_bytes.rs`);
//! * a [`RomFile`]'s suspect table is private (`compile_fail/suspect/suspect_flag_is_private.rs`);
//! * a [`RomFile`] cannot be made up with a struct literal, nor re-built
//!   from a suspect one by functional update, so `read` cannot be steered to
//!   [`Image::Raw`] for a suspect file (`compile_fail/suspect/forge_a_clean_entry.rs`,
//!   `compile_fail/suspect/clean_copy_of_a_suspect.rs`).
//!
//! `compile_fail/suspect_pass/match_raw_or_suspect.rs` is the positive control.
//! Regenerate snapshots with `TRYBUILD=overwrite cargo test -p rr-testkit
//! --test suspect_compile_fail` and review every `.stderr` diff.
//!
//! [`Corpus::read`]: rr_testkit::corpus::Corpus::read
//! [`Image`]: rr_testkit::corpus::Image
//! [`Image::Raw`]: rr_testkit::corpus::Image::Raw
//! [`RomFile`]: rr_testkit::corpus::RomFile

/// US-RR-11-2: no path from a corpus entry to raw bytes skips the
/// raw-or-suspect match.
#[test]
fn suspect_images_cannot_be_read_as_raw_without_matching() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/compile_fail/suspect_pass/*.rs");
    cases.compile_fail("tests/compile_fail/suspect/*.rs");
}
