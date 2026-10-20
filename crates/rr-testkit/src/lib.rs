//! Test fixtures, snapshot helpers, proptest strategies and VirtualClock harness (dev-dependency only).
//!
//! `rr-testkit` is the one shared test kit for the workspace (ARCHITECTURE.md
//! "Crate Map", PROJECT.md "Testing Strategy"). Every crate takes it as a
//! **dev-dependency only**; `cargo xtask check-deps` rejects any normal or
//! build edge to it.
//!
//! Modules:
//!
//! * [`snapshot`] — preconfigured [`insta::Settings`] and the workspace-wide
//!   redaction conventions (`[path]`, `[wall-time]`, `[duration]`).
//! * [`strategies`] — `proptest` strategies, one submodule per concept.
//! * [`fixtures`] — three-tier fixture loader (public / `RR_CORPUS_DIR` /
//!   `RR_FIXTURES_DIR`), `schema = N` checks, and the
//!   [`corpus_test!`] / [`private_test!`] skip-reporting macros.
//! * [`corpus`] — read-only loader for the external stock-ROM corpus
//!   (`RR_CORPUS_DIR`, `fixtures/corpus-manifest.toml`): refuses a directory
//!   inside the repository working tree, reports "not fetched" distinctly,
//!   and makes callers handle suspect images explicitly. Its
//!   [`corpus::location`] submodule is shared, as source, with
//!   `cargo xtask fetch-corpus`; [`corpus::formulas`] harvests every formula
//!   of the pinned definition corpus at test time (never committed, D-028).
//! * [`kat`] — the schema-1 known-answer fixture types (decode-values and
//!   checksum fixtures, per-value [`kat::Source`], changelog), their loader, and
//!   the validator [`kat::validate`] (`cargo xtask validate-fixtures`).
//! * [`reference`](mod@reference) — the naive reference codec (T2(b)): byte-by-byte decode
//!   and encode for every storage type, independent of rr-core (enforced by
//!   `cargo xtask check-reference-isolation`). Its submodule
//!   [`reference::synthetic`] generates synthetic ROMs plus a manifest of
//!   expected raw values from a value plan (extremes, NaN/Inf/subnormals,
//!   overlapping tables, tables ending at the end of the ROM), under the same
//!   isolation rule.
//! * [`encoding_oracle`] — the test boundary between rr-core's Encoding and
//!   [`reference`](mod@reference): type conversions both ways and `check_*` /
//!   `assert_*_reference` helpers that run both codecs on the same input and
//!   compare them bit-exactly (used with [`strategies::encoding`]). It may
//!   name rr-core because it lives outside [`reference`](mod@reference), which never reaches
//!   it.
//! * [`clock`] — VirtualClock test harness: [`clock::TestClock`] steps worker
//!   threads through virtual time deterministically and asserts elapsed time.

pub mod clock;
pub mod corpus;
pub mod encoding_oracle;
pub mod fixtures;
pub mod kat;
pub mod reference;
pub mod snapshot;
pub mod strategies;
