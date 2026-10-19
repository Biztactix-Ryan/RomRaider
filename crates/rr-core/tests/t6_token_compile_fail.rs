//! T6 safety-invariant suite: capability tokens cannot be forged outside
//! `rr-core` (SECURITY.md "Authorization", D-013).
//!
//! Proves acceptance criterion **US-RR-7-1**: compile-fail (trybuild) tests
//! show that, from a crate outside the minting module, a token
//! ([`RomWrite`], [`EcuMemoryWrite`], [`EcuReset`]) cannot be
//!
//! * constructed with a struct literal (`ui/always/construct_*.rs`);
//! * cloned or copied into existence (`ui/always/clone_tokens.rs`,
//!   `ui/always/copy_tokens.rs`);
//! * defaulted (`ui/always/default_tokens.rs`);
//! * deserialized — or serialized for replay (`ui/always/deserialize_tokens.rs`,
//!   `ui/always/serialize_tokens.rs`);
//! * transmuted, zeroed or `assume_init`ed in safe code, and not even in an
//!   `unsafe` block under `#![forbid(unsafe_code)]`
//!   (`ui/always/transmute_*.rs`);
//! * minted without the `confirmation-authority` feature: `authority::mint_*`
//!   and the private `mint` module are unnameable, `Confirmation` cannot be
//!   implemented and its `Sealed` supertrait cannot be named
//!   (`ui/sealed/*.rs`).
//!
//! `ui/pass/read_only_api.rs` is the positive control: legitimate use of the
//! tokens compiles, so each compile-fail case fails for the reason its
//! committed `.stderr` snapshot records. A changed diagnostic (e.g. a token
//! gaining `Clone`, so one of three errors disappears) fails the test.
//!
//! Scope: forging with `unsafe` code is out of scope for the type-level
//! guarantee. The workspace lints do not (yet) forbid `unsafe_code`; that is
//! a lint-policy and review matter, not something a token can prevent.
//!
//! The `ui/sealed` cases hold only when `rr-core` is built *without*
//! `confirmation-authority` (trybuild compiles the cases with the features
//! this test crate was built with). They are skipped when the feature is
//! unified on; the isolated per-crate builds and the manifest check described
//! in `rr-core/src/capability.rs` cover that configuration.
//!
//! Regenerate snapshots with `TRYBUILD=overwrite cargo test -p rr-core --test
//! t6_token_compile_fail` and review every `.stderr` diff.
//!
//! [`RomWrite`]: rr_core::capability::RomWrite
//! [`EcuMemoryWrite`]: rr_core::capability::EcuMemoryWrite
//! [`EcuReset`]: rr_core::capability::EcuReset

/// US-RR-7-1: every forging attempt from outside `rr-core` fails to compile
/// with the committed diagnostic, and legitimate use compiles.
#[test]
fn t6_tokens_cannot_be_forged_outside_rr_core() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/pass/*.rs");
    cases.compile_fail("tests/ui/always/*.rs");
    #[cfg(not(feature = "confirmation-authority"))]
    cases.compile_fail("tests/ui/sealed/*.rs");
}
