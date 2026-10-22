//! Protocol timing parameters taken from published standards (US-RR-14,
//! PROJECT.md T2 (d)).
//!
//! A link profile's timings (ARCHITECTURE.md: a Link pushes them to an
//! adapter that supports them and enforces them in software otherwise) start
//! from the defaults a diagnostic standard specifies. Those defaults live
//! here as named [`Duration`](core::time::Duration) constants, one module
//! per standard under [`iso`], so a comms path that waits says which
//! parameter it is waiting for instead of a bare number. Legacy RomRaider
//! waited a bare 55 ms after the NCS ECU-ID request
//! (`logger/ecu/comms/io/connection/NCSLoggerConnection.java:111`), which is
//! ISO 14230-2's default P3min ([`iso::iso14230_2::P3_MIN`], PROJECT.md
//! MF-28).
//!
//! Every constant cites its document, edition and clause, and has a
//! known-answer case in `fixtures/kat/timing-*.toml` with the same citation;
//! rr-testkit's `timing_kat` test checks each constant against its case, so
//! changing either side alone fails.
//!
//! These are constants only: time itself is read from a
//! [`Clock`](crate::clock::Clock) (C9, D-010), and a module here holds no
//! state (`cargo xtask check-globals`).

pub mod iso;
