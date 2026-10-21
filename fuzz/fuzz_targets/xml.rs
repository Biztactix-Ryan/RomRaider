//! XML definition files (`rr_defs::xml`): read under default and tight
//! `Limits`, check the tree, write back and re-read. The body is in
//! `rr_fuzz::xml` (`src/xml_target.rs`).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| rr_fuzz::xml(data));
