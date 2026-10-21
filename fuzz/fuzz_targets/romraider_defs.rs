//! RomRaider ECU-definition files (`rr_defs::romraider`): read, load into a
//! one-file `DefinitionSet` and resolve every `<rom>`, checking diagnostics,
//! spans and the resolved tables; raw XML, or (first byte `0x00`) a
//! definition built from the input's choices. The body is in
//! `rr_fuzz::romraider_defs` (`src/romraider_defs_target.rs`).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rr_fuzz::romraider_defs(data));
