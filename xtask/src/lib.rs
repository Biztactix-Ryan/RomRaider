//! Library half of `cargo xtask`: checks that are unit-testable without a shell.

pub mod codeowners;
pub mod coverage_gate;
pub mod deny_selftest;
pub mod deps;
pub mod diagnostics_doc;
pub mod fixtures_selftest;
pub mod fuzz;
pub mod fuzz_issue;
pub mod gates_selftest;
pub mod gen_synthetic;
pub mod globals;
pub mod lint_docs;
pub mod lint_selftest;
pub mod mutants_exclusions;
pub mod mutants_scope;
pub mod mutants_selftest;
pub mod reference_isolation;
pub mod time_exceptions;
pub mod validate_fixtures;
pub mod writes;
