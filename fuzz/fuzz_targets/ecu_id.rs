//! Template fuzz target: copy this file to add a target (docs/fuzzing.md).
//! The body lives in `rr_fuzz::ecu_id` so committed regression inputs can run
//! it on stable without libFuzzer.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rr_fuzz::ecu_id(data));
