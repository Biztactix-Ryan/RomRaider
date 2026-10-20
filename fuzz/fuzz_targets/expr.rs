//! Formulas (`rr_core::expr`): parse, print back, evaluate. The body and the
//! input layout are in `rr_fuzz::expr` (`src/expr_target.rs`).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rr_fuzz::expr(data));
