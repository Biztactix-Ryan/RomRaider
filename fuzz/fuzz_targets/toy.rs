//! Toy target with optional planted bugs (features `planted-*`), used to
//! prove the harness reports panics, hangs and runaway memory.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rr_fuzz::toy(data));
