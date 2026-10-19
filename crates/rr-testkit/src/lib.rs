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
//! * [`clock`] — VirtualClock test harness: [`clock::TestClock`] steps worker
//!   threads through virtual time deterministically and asserts elapsed time.

pub mod clock;
pub mod fixtures;
pub mod snapshot;
pub mod strategies;
