//! Library half of `cargo xtask`: checks that are unit-testable without a shell.

pub mod codeowners;
// The corpus location and outside-the-repo check (US-RR-11-9), compiled from
// rr-testkit's source so fetch-corpus and the test-side corpus loader share
// one implementation without a crate edge (xtask may not link rr-testkit:
// `cargo xtask check-deps` rule 3). std-only by design.
pub mod corpus_coverage;
#[path = "../../crates/rr-testkit/src/corpus/location.rs"]
pub mod corpus_location;
pub mod corpus_manifest;
pub mod coverage_gate;
pub mod deny_selftest;
pub mod deps;
pub mod diagnostics_doc;
pub mod fetch_corpus;
pub mod fixtures_selftest;
pub mod fuzz;
pub mod fuzz_issue;
pub mod gates_selftest;
pub mod gen_synthetic;
pub mod globals;
pub mod leak_guard;
pub mod lint_docs;
pub mod lint_selftest;
pub mod mutants_exclusions;
pub mod mutants_scope;
pub mod mutants_selftest;
pub mod reference_isolation;
pub mod release_artifacts;
pub mod squashfs;
pub mod time_exceptions;
pub mod validate_fixtures;
pub mod writes;
