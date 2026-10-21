//! Byte pattern text (`rr_core::pattern`): parse, print back, match. The body
//! and the input layout are in `rr_fuzz::pattern` (`src/pattern_target.rs`).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rr_fuzz::pattern(data));
