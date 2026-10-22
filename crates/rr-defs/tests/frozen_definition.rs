//! A resolved definition cannot be changed from outside `rr-defs` —
//! US-RR-34-5.
//!
//! Proves the compile-time half of the US-RR-34 acceptance criterion
//! *"Resolved Definitions are immutable (no interior mutability)"* with
//! compile-fail (trybuild) cases pinned to committed `.stderr` snapshots:
//!
//! * a [`Definition`], its parts and the [`RomIdIndex`] have no public
//!   field, no `&mut` getter and no `IndexMut`, and their getters lend no
//!   `&mut`, even to a caller that owns them
//!   (`ui/frozen_definition/no_mutable_access.rs`);
//! * nothing lent can be written, even by a caller holding the definition
//!   `&mut`: a table shared through its [`Arc`] is never lent `&mut`
//!   (`ui/frozen_definition/getters_lend_shared.rs`);
//! * the bound `rr_defs::resolve` asserts of every resolved type
//!   (`Send + Sync + UnwindSafe + RefUnwindSafe`, checked through every
//!   field) rejects `Cell`, `RefCell`, `OnceCell`, `UnsafeCell` and `Rc`
//!   (`ui/frozen_definition/interior_mutability_fails_the_bound.rs`).
//!
//! The bound does not reject `Mutex`, `RwLock`, `OnceLock`, `LazyLock` or
//! an atomic: `ui/frozen_definition_pass/bound_gaps.rs` pins that, and
//! `cargo xtask check-globals` (the review leg; xtask/tests/globals.rs)
//! rejects those names in the resolve module and in every module whose
//! types a resolved definition embeds.
//!
//! `ui/frozen_definition_pass/read_only_api.rs` is the positive control:
//! reading a definition from several threads compiles and runs, and every
//! public resolved type meets the bound, so each compile-fail case fails
//! for the reason its snapshot records.
//!
//! Out of scope: replacing a whole value the caller owns (`def = other`)
//! is not a change to a definition anyone else holds, and `unsafe` code is
//! outside any type-level guarantee.
//!
//! Regenerate snapshots with `TRYBUILD=overwrite cargo test -p rr-defs
//! --test frozen_definition` and review every `.stderr` diff.
//!
//! [`Definition`]: rr_defs::resolve::Definition
//! [`RomIdIndex`]: rr_defs::resolve::RomIdIndex
//! [`Arc`]: std::sync::Arc

/// US-RR-34-5: every attempt to change a resolved definition or its ROM-ID
/// index from outside `rr-defs` fails to compile with the committed
/// diagnostic, interior mutability fails the frozen bound, and read-only
/// use compiles and runs.
#[test]
fn resolved_definitions_cannot_be_changed_from_outside() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/frozen_definition_pass/*.rs");
    cases.compile_fail("tests/ui/frozen_definition/*.rs");
}
