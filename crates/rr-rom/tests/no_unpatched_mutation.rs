//! No public API mutates image bytes without producing a patch — US-RR-31-4.
//!
//! Proves the US-RR-31 acceptance criterion *"there is no public API that
//! mutates image bytes without producing a patch"* from outside `rr-rom`,
//! with compile-fail (trybuild) cases pinned to committed `.stderr`
//! snapshots:
//!
//! * `Image::apply_patch`, the one mutator, is private to the crate (E0624),
//!   on a fresh image and on one released by [`Journal::into_image`]
//!   (`ui/no_unpatched_mutation/apply_patch_is_private.rs`);
//! * an [`Image`] lends its bytes only as `&[u8]`: no public field, no
//!   `bytes_mut`, no `DerefMut`, `IndexMut`, `AsMut`, `BorrowMut`, `&mut`
//!   iteration or `Extend`
//!   (`ui/no_unpatched_mutation/image_bytes_not_mutable.rs`);
//! * a [`Journal`] lends its image only as `&Image`: its image and history
//!   are private, there is no `image_mut` or `IndexMut`, the image cannot be
//!   replaced through [`Journal::image`], and recorded patches and groups
//!   cannot be edited or forged
//!   (`ui/no_unpatched_mutation/journal_no_unpatched_mutation.rs`).
//!
//! `ui/no_unpatched_mutation_pass/patch_api.rs` is the positive control: the
//! same change made through [`Journal::apply`] compiles and runs, so each
//! compile-fail case fails for the reason its snapshot records, not because
//! nothing builds. A changed diagnostic (e.g. `Image` gaining `IndexMut`, so
//! one error disappears) fails the test.
//!
//! Out of scope: building a new [`Image`] from bytes ([`Image::new`],
//! `From<Vec<u8>>`) is a new base, not a mutation, and overwriting a whole
//! `Journal` value the caller owns (`*journal = Journal::new(..)`) replaces
//! it, image and history together, rather than changing bytes under a
//! history. `unsafe` code is outside any type-level guarantee.
//!
//! Regenerate snapshots with `TRYBUILD=overwrite cargo test -p rr-rom --test
//! no_unpatched_mutation` and review every `.stderr` diff.
//!
//! [`Image`]: rr_rom::image::Image
//! [`Image::new`]: rr_rom::image::Image::new
//! [`Journal`]: rr_rom::journal::Journal
//! [`Journal::into_image`]: rr_rom::journal::Journal::into_image
//! [`Journal::image`]: rr_rom::journal::Journal::image
//! [`Journal::apply`]: rr_rom::journal::Journal::apply

/// US-RR-31-4: every attempt to change image bytes from outside `rr-rom`
/// other than through a journal patch fails to compile with the committed
/// diagnostic, and the patch route compiles and runs.
#[test]
fn image_bytes_change_only_through_journal_patches() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/no_unpatched_mutation_pass/*.rs");
    cases.compile_fail("tests/ui/no_unpatched_mutation/*.rs");
}
