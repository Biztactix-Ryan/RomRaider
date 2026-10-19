//! Library half of `cargo xtask`: checks that are unit-testable without a shell.

pub mod codeowners;
pub mod deny_selftest;
pub mod deps;
pub mod fixtures_selftest;
pub mod gates_selftest;
pub mod globals;
pub mod lint_docs;
pub mod lint_selftest;
pub mod time_exceptions;
pub mod writes;
