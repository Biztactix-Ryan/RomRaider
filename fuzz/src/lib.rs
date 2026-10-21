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

mod expr_target;
mod pattern_target;
pub mod replay;
mod romraider_defs_target;
mod xml_target;

pub use expr_target::expr;
pub use pattern_target::pattern;
pub use romraider_defs_target::romraider_defs;
pub use xml_target::xml;

use rr_core::capability::{AddressRange, EcuId, ScopeError};

/// Every fuzz target, by name: the `[[bin]]` name in `fuzz/Cargo.toml`, the
/// file stem in `fuzz_targets/`, and the directory under `seeds/` and
/// `regressions/`. A regression runner iterates this list.
pub const TARGETS: &[(&str, Target)] = &[
    ("ecu_id", ecu_id),
    ("expr", expr),
    ("pattern", pattern),
    ("romraider_defs", romraider_defs),
    ("toy", toy),
    ("xml", xml),
];

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

    /// The `expr` limit-edge inputs are what their names say: each
    /// `seeds/expr/*-at-limit` parses within `Limits::DEFAULT`, and each
    /// `regressions/expr/*-over-limit` (and the other pathological inputs)
    /// is refused with that limit's error and diagnostic code, not a panic
    /// (US-RR-20-9).
    #[test]
    fn expr_limit_edges_are_just_within_and_just_over() {
        use rr_core::expr::{self, Func, ParseErrorKind as K};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |dir: &str, name: &str| {
            let path = root.join(dir).join("expr").join(name);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            String::from_utf8(bytes).unwrap()
        };
        for name in [
            "depth-at-limit",
            "len-at-limit",
            "literal-at-limit",
            "nodes-at-limit",
            "args-at-limit",
            "stack-at-limit",
        ] {
            let src = read("seeds", name);
            if let Err(e) = expr::parse(&src) {
                panic!("seeds/expr/{name}: {e:?}");
            }
        }
        let cases: [(&str, K); 11] = [
            ("depth-over-limit", K::TooDeep { limit: 128 }),
            ("unary-over-limit", K::TooDeep { limit: 128 }),
            (
                "len-over-limit",
                K::TooLong {
                    len: 4097,
                    limit: 4096,
                },
            ),
            (
                "huge-literal",
                K::TooLong {
                    len: 60000,
                    limit: 4096,
                },
            ),
            (
                "literal-over-limit",
                K::LiteralTooLong {
                    token: "a number",
                    len: 129,
                    limit: 128,
                },
            ),
            ("nodes-over-limit", K::TooManyNodes { limit: 1024 }),
            (
                "args-over-limit",
                K::TooManyArgs {
                    func: Func::Sum,
                    limit: 32,
                },
            ),
            (
                "deep-and-wide",
                K::TooManyArgs {
                    func: Func::Sum,
                    limit: 32,
                },
            ),
            ("stack-over-limit", K::StackTooDeep { limit: 32 }),
            ("number-out-of-range", K::NumberOutOfRange),
            ("unclosed-deep", K::TooDeep { limit: 128 }),
        ];
        for (name, want) in cases {
            let src = read("regressions", name);
            let err = expr::parse(&src).expect_err(name);
            assert_eq!(*err.kind(), want, "regressions/expr/{name}");
            assert_eq!(err.code(), want.code(), "regressions/expr/{name}");
        }
    }

    /// `regressions/xml/` is the hostile sample set of rr-defs, byte for
    /// byte (`crates/rr-defs/tests/data/xml/hostile/`, whose test checks
    /// each is refused with its expected code): the replay runs exactly
    /// those attacks through the `xml` target (US-RR-27-7).
    #[test]
    fn xml_regressions_are_the_hostile_set() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let listing = |dir: &std::path::Path| {
            let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("{dir:?}: {e}"))
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .map(|p| {
                    let name = p.file_name().unwrap().to_string_lossy().into_owned();
                    (name, std::fs::read(&p).unwrap())
                })
                .collect();
            files.sort();
            files
        };
        let ours = listing(&root.join("regressions/xml"));
        let theirs = listing(&root.join("../crates/rr-defs/tests/data/xml/hostile"));
        let names = |f: &[(String, Vec<u8>)]| f.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
        assert_eq!(names(&ours), names(&theirs));
        for ((name, a), (_, b)) in ours.iter().zip(&theirs) {
            assert!(
                a == b,
                "regressions/xml/{name} differs from the rr-defs sample"
            );
        }
        assert!(ours.len() >= 20, "{}", ours.len());
        // Nothing else in the directory: every input is one of the set.
        let all = std::fs::read_dir(root.join("regressions/xml"))
            .unwrap()
            .count();
        assert_eq!(all, ours.len());
    }

    /// The real-shaped `xml` seeds read cleanly (all but the deliberately
    /// broken `unclosed`), so the fuzzer starts from inputs that reach the
    /// whole tree builder; `attlist-default` and the rest with no
    /// diagnostic but its one warning.
    #[test]
    fn xml_seeds_read_as_intended() {
        use rr_core::diag::codes;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds/xml");
        for entry in std::fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let loaded = rr_defs::xml::parse(&name, &std::fs::read(&path).unwrap());
            let codes: Vec<_> = loaded.diagnostics().iter().map(|d| d.code()).collect();
            let want: &[_] = match name.as_str() {
                "unclosed" => &[codes::XML_MALFORMED],
                "attlist-default" => &[codes::XML_DTD_DEFAULT_IGNORED],
                _ => &[],
            };
            assert_eq!(codes, want, "seeds/xml/{name}: {:#?}", loaded.diagnostics());
        }
    }

    /// The `romraider_defs` seeds reach what they are there for: the
    /// well-formed ones read, load and resolve with no Error, between them
    /// resolving a table of every kind (a static axis, a scaling `base` and
    /// an `omit` included); `errors.xml`, `unknown.xml` and `not-roms.xml`
    /// report what their names say; and the built-mode seeds (first byte
    /// `0x00`) build definitions that resolve tables (US-RR-29-9).
    #[test]
    fn romraider_defs_seeds_read_as_intended() {
        use rr_core::diag::{Severity, codes};
        use rr_defs::romraider::{DefinitionSet, TableKind};
        use std::collections::BTreeSet;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds/romraider_defs");
        let mut kinds = BTreeSet::new();
        let mut built_tables = 0;
        let mut static_axis = false;
        for entry in std::fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).unwrap();
            let built = bytes.first() == Some(&0);
            let xml = if built {
                romraider_defs_target::build(&bytes[1..]).into_bytes()
            } else {
                bytes
            };
            let (set, mut diags) =
                DefinitionSet::read_all([("seed.xml", xml.as_slice())]).accept_errors();
            let mut tables = 0;
            let mut resolver = set.resolver();
            for (r, _) in set.roms() {
                let (rom, more) = resolver.resolve(r).accept_errors();
                diags.extend(more);
                for t in rom.iter().flat_map(|rom| rom.tables()) {
                    tables += 1;
                    kinds.insert(t.kind.name());
                    static_axis |= t.axes.iter().any(|a| a.cells.is_none());
                }
            }
            let has = |code| diags.iter().any(|d| d.code() == code);
            let errors = diags.iter().filter(|d| d.is_error()).count();
            match name.as_str() {
                "errors.xml" => {
                    for code in [
                        codes::UNKNOWN_BASE,
                        codes::INHERITANCE_CYCLE,
                        codes::MISSING_AXIS,
                        codes::MISSING_STORAGE_TYPE,
                        codes::UNKNOWN_SCALING,
                        codes::UNTYPED_TABLE,
                        codes::MISSING_XMLID,
                    ] {
                        assert!(has(code), "{name}: no {code}: {diags:#?}");
                    }
                }
                "unknown.xml" => {
                    assert_eq!(errors, 0, "{name}: {diags:#?}");
                    for code in [
                        codes::UNKNOWN_ELEMENT,
                        codes::UNKNOWN_ATTRIBUTE,
                        codes::UNEXPECTED_TEXT,
                        codes::DUPLICATE_ELEMENT,
                        codes::DUPLICATE_XMLID,
                        codes::UNSUPPORTED_ELEMENT,
                    ] {
                        assert!(has(code), "{name}: no {code}: {diags:#?}");
                    }
                }
                "not-roms.xml" => assert!(has(codes::NOT_ROM_DEFINITION), "{diags:#?}"),
                _ if built => {
                    assert!(tables > 0, "{name} resolves no table: {diags:#?}");
                    built_tables += tables;
                }
                _ => {
                    assert!(
                        diags.iter().all(|d| d.severity() != Severity::Error),
                        "{name}: {diags:#?}"
                    );
                    assert!(tables > 0, "{name} resolves no table");
                }
            }
        }
        let all: BTreeSet<_> = [
            TableKind::OneD,
            TableKind::TwoD,
            TableKind::ThreeD,
            TableKind::Switch,
            TableKind::BitwiseSwitch,
        ]
        .iter()
        .map(|k| k.name())
        .collect();
        assert_eq!(kinds, all);
        assert!(static_axis, "no seed resolves a static axis");
        assert!(built_tables > 0, "no built-mode seed");
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
