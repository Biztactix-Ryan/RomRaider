//! Bodies of the RomRaider-RS fuzz targets (test tier T3, story US-RR-5).
//!
//! Each target is a plain `pub fn <name>(data: &[u8])` here, and
//! `fuzz_targets/<name>.rs` is a one-line `fuzz_target!` wrapper around it.
//! Keeping the body in the library means it can run without libFuzzer or
//! nightly: committed crash inputs under `fuzz/regressions/<name>/` are fed to
//! the same function by an ordinary stable test, `fuzz/tests/regressions.rs`,
//! each input in its own child process under the per-input limits (see
//! [`TARGETS`], [`replay`] and docs/fuzzing.md).
//!
//! A target fails by panicking (an `assert!` on an invariant counts), by
//! running longer than the per-input timeout, or by exceeding the memory
//! limit. The limits live in `fuzz/Cargo.toml` `[package.metadata.rr-fuzz]`
//! and are applied by `cargo xtask fuzz run`.

use std::fmt::Write as _;

pub mod replay;

use rr_core::capability::{AddressRange, EcuId, ScopeError};

/// Every fuzz target, by name: the `[[bin]]` name in `fuzz/Cargo.toml`, the
/// file stem in `fuzz_targets/`, and the directory under `seeds/` and
/// `regressions/`. A regression runner iterates this list.
pub const TARGETS: &[(&str, Target)] = &[("ecu_id", ecu_id), ("toy", toy)];

/// A fuzz target body: runs one input, panics on a bug.
pub type Target = fn(&[u8]);

/// Template target: decoding an ECU ID and an address range from untrusted
/// bytes (as an ECU or a definition file would supply them) never panics, and
/// what it accepts obeys the documented invariants.
///
/// Copy this shape for a new target: call the API under test on `data`, then
/// assert the properties that must hold for every input it accepts.
pub fn ecu_id(data: &[u8]) {
    match EcuId::from_bytes(data) {
        Ok(id) => {
            assert_eq!(
                id.as_bytes(),
                data,
                "EcuId must keep the bytes it was given"
            );
            let hex = id.to_string();
            assert_eq!(hex.len(), data.len() * 2, "two hex digits per byte");
            assert!(
                hex.bytes()
                    .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)),
                "upper-case hex only: {hex}"
            );
        }
        Err(ScopeError::EmptyEcuId) => assert!(data.is_empty()),
        Err(ScopeError::EcuIdTooLong { len }) => {
            assert_eq!(len, data.len());
            assert!(len > EcuId::MAX_LEN);
        }
        Err(other) => panic!("EcuId::from_bytes returned an unexpected error: {other:?}"),
    }

    // The first eight bytes, if present, as a big-endian `start..end` range.
    let (Some(start), Some(end)) = (be_u32(data, 0), be_u32(data, 4)) else {
        return;
    };
    match AddressRange::new(start, end) {
        Ok(range) => {
            assert!(start < end);
            assert_eq!(range.len(), end - start);
            assert!(range.contains(start));
            assert!(!range.contains(end));
            let mut shown = String::new();
            let _ = write!(shown, "{range}");
            assert!(shown.contains(".."), "range display: {shown}");
        }
        Err(ScopeError::EmptyRange { start: s, end: e }) => {
            assert!(s >= e && s == start && e == end);
        }
        Err(other) => panic!("AddressRange::new returned an unexpected error: {other:?}"),
    }
}

/// Toy target: a length-prefixed record reader (`[len][len bytes]...`) that
/// is correct as shipped. Enabling a `planted-*` feature adds a deliberate
/// bug reached by an input starting with a magic prefix:
///
/// | Feature         | Input prefix  | Bug                                         |
/// |-----------------|---------------|---------------------------------------------|
/// | `planted-panic` | `PANIC`       | panics                                      |
/// | `planted-hang`  | `HANG`        | loops forever                               |
/// | `planted-oom`   | `OOM-MALLOC`  | one 1 GiB allocation                        |
/// | `planted-oom`   | `OOM-RSS`     | keeps 3 GiB of touched memory, then spins   |
///
/// They exist only to prove the harness turns each kind of bug into a failure
/// (`cargo xtask fuzz selftest`) and that the nightly job files it.
pub fn toy(data: &[u8]) {
    planted(data);
    let mut rest = data;
    let mut payload = 0usize;
    while let Some((&len, tail)) = rest.split_first() {
        let len = usize::from(len);
        let Some((record, tail)) = tail.split_at_checked(len) else {
            return; // truncated record: rejected, not a crash
        };
        payload += record.len();
        rest = tail;
    }
    assert!(payload <= data.len());
}

#[allow(unused_variables, reason = "unused when no planted-* feature is on")]
fn planted(data: &[u8]) {
    #[cfg(feature = "planted-panic")]
    if data.starts_with(b"PANIC") {
        panic!("planted panic (feature planted-panic)");
    }
    #[cfg(feature = "planted-hang")]
    if data.starts_with(b"HANG") {
        let mut n = 0u64;
        loop {
            n = std::hint::black_box(n.wrapping_add(1));
        }
    }
    #[cfg(feature = "planted-oom")]
    if data.starts_with(b"OOM-MALLOC") {
        let buf: Vec<u8> = Vec::with_capacity(1 << 30);
        std::hint::black_box(&buf);
    }
    #[cfg(feature = "planted-oom")]
    if data.starts_with(b"OOM-RSS") {
        // Touched 16 MiB chunks (each under malloc_limit_mb) up to 3 GiB,
        // then hold them: the RSS limit must fire before the timeout does.
        let mut held: Vec<Vec<u8>> = Vec::new();
        for _ in 0..192 {
            held.push(vec![1u8; 16 << 20]);
            std::hint::black_box(&held);
        }
        let mut n = 0u64;
        loop {
            n = std::hint::black_box(n.wrapping_add(1));
            std::hint::black_box(&held);
        }
    }
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every committed seed runs cleanly through its target (stable, no
    /// libFuzzer): `cargo test --manifest-path fuzz/Cargo.toml`.
    #[test]
    fn seeds_run_cleanly() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for &(name, target) in TARGETS {
            let dir = root.join("seeds").join(name);
            let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{dir:?}: {e}"));
            for entry in entries {
                let path = entry.unwrap().path();
                target(&std::fs::read(&path).unwrap());
            }
        }
    }

    #[test]
    fn every_target_has_a_fuzz_targets_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for &(name, _) in TARGETS {
            assert!(
                root.join("fuzz_targets")
                    .join(format!("{name}.rs"))
                    .is_file(),
                "{name}"
            );
        }
    }
}
