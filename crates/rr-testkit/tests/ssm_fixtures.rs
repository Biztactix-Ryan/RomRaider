//! Structural checks of the SSM K-line simulator fixtures
//! (`fixtures/public/ssm/*.toml`, US-RR-12-4; format in fixtures/README.md,
//! "SSM simulator fixtures").
//!
//! There is no SSM codec yet (US-RR-47). This file implements only the frame
//! rules of `docs/legacy/ssm.md`, independently, so an edit to a fixture
//! cannot silently break the protocol:
//!
//! * frame layout (ssm.md §2.1): header `80`; request destination is the
//!   module (`10` ECU, `18` TCU, §1.2) and source the tester `F0`; reply the
//!   other way round; `LL` = frame length − 5;
//! * checksum (§2.2): the low 8 bits of the sum of every byte before it, and
//!   each fixture's written working lists exactly those bytes and that sum;
//! * response code = command + `40` (§2.1);
//! * per-command parameters (§3.2–§3.7): `BF` has none; `A8` is `PP` plus
//!   3-byte addresses, `LL = 3N + 2`, `N ≤ 84`, reply `LL = N + 1`; `A0` is
//!   `LL = 6`, `CC = count − 1`, `1 ≤ count ≤ 254`, reply `LL = count + 1`;
//!   `B8` is `LL = 5`, ack `LL = 2` carrying the value; `B0` is
//!   `LL = n + 4`, `n ≤ 251`, ack `LL = n + 1` carrying the data; init data
//!   is at least 8 bytes and the ECU ID is init-data bytes 3–7;
//! * capability bits (§3.2): `index < len && data[index] & (1 << bit) != 0`;
//! * the K-line echo (§1.4, §5): with a request, the wire is the request
//!   bytes then the reply; an `A8` exchange fills `(N+6) + (3N+7)` bytes;
//!   a continuous stream has no request and no echo;
//! * every error case really has the defect it names (§4, §6), and
//!   everything else about it is well formed.

#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "the helpers below run only inside #[test] fns; panicking with the fixture and rule is how they fail the calling test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use rr_testkit::fixtures::repo_root;
use serde::Deserialize;

/// The fixture directory.
fn ssm_dir() -> PathBuf {
    repo_root().join("fixtures").join("public").join("ssm")
}

/// ssm.md, which every fixture cites.
fn ssm_md() -> String {
    fs::read_to_string(repo_root().join("docs").join("legacy").join("ssm.md"))
        .expect("docs/legacy/ssm.md")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema: u32,
    kind: String,
    title: String,
    fixture_ids: Vec<String>,
    synthetic: String,
    exchange: Vec<Exchange>,
    changelog: Vec<Changelog>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Changelog {
    #[allow(dead_code, reason = "presence is checked by deserializing")]
    date: toml::value::Datetime,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    name: String,
    spec: String,
    module: String,
    command: String,
    state: String,
    tx: String,
    #[serde(default)]
    tx_checksum: Option<String>,
    rx: Vec<String>,
    rx_checksum: Vec<String>,
    wire: String,
    outcome: String,
    #[serde(default)]
    meaning: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    legacy: Option<String>,
    #[serde(default)]
    rr: Option<String>,
    #[serde(default)]
    decoded: Option<Decoded>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decoded {
    pp: Option<String>,
    addresses: Option<Vec<String>>,
    values: Option<Vec<String>>,
    address: Option<String>,
    count: Option<usize>,
    data: Option<String>,
    value: Option<String>,
    init_data: Option<String>,
    ecu_id: Option<String>,
    capability: Option<Vec<Capability>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    index: usize,
    bit: u8,
    supported: bool,
}

/// Header byte (ssm.md §2.1).
const HEADER: u8 = 0x80;
/// Tester address (§1.2).
const TESTER: u8 = 0xF0;
/// Response code offset (§2.1).
const RESPONSE_OFFSET: u8 = 0x40;
/// Most `A8` addresses one request can carry: `3N + 2 ≤ 255` (§3.3).
const A8_MAX_ADDRESSES: usize = 84;
/// Largest `A0` count one reply can carry: `count + 1 ≤ 255` (§3.4).
const A0_MAX_COUNT: usize = 254;
/// Largest `B0` data length: `n + 4 ≤ 255` (§3.7).
const B0_MAX_DATA: usize = 251;

fn hex(s: &str, ctx: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|b| {
            assert_eq!(b.len(), 2, "{ctx}: `{b}` is not one hex byte");
            u8::from_str_radix(b, 16).unwrap_or_else(|e| panic!("{ctx}: `{b}`: {e}"))
        })
        .collect()
}

fn hex24(s: &str, ctx: &str) -> [u8; 3] {
    assert_eq!(s.len(), 6, "{ctx}: address `{s}` is not 6 hex digits");
    let v = u32::from_str_radix(s, 16).unwrap_or_else(|e| panic!("{ctx}: `{s}`: {e}"));
    let [_, a, b, c] = v.to_be_bytes();
    [a, b, c]
}

fn upper_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// The 8-bit sum of §2.2 over `bytes`.
fn sum8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b))
}

fn module_address(module: &str, ctx: &str) -> u8 {
    match module {
        "ecu" => 0x10,
        "tcu" => 0x18,
        other => panic!("{ctx}: unknown module `{other}` (ssm.md §1.2: ecu, tcu)"),
    }
}

/// Checks the written working `"b0+b1+... = 0xS; low 8 bits CS ..."`: the
/// terms are exactly `body` in order, `S` is their sum and `CS` its low 8
/// bits. Returns `CS`.
fn check_working(working: &str, body: &[u8], ctx: &str) -> u8 {
    let (terms, rest) = working
        .split_once(" = 0x")
        .unwrap_or_else(|| panic!("{ctx}: working `{working}` lacks ` = 0x`"));
    let terms: Vec<u8> = terms
        .split('+')
        .map(|t| u8::from_str_radix(t.trim(), 16).unwrap_or_else(|e| panic!("{ctx}: `{t}`: {e}")))
        .collect();
    assert_eq!(
        terms, body,
        "{ctx}: the working does not list the frame's bytes before the checksum"
    );
    let (total, rest) = rest
        .split_once("; low 8 bits ")
        .unwrap_or_else(|| panic!("{ctx}: working lacks `; low 8 bits`"));
    let total = u32::from_str_radix(total, 16).unwrap_or_else(|e| panic!("{ctx}: sum: {e}"));
    let real: u32 = body.iter().map(|&b| u32::from(b)).sum();
    assert_eq!(total, real, "{ctx}: the working's sum is wrong");
    let cs = u8::from_str_radix(rest.get(..2).unwrap_or(rest), 16)
        .unwrap_or_else(|e| panic!("{ctx}: checksum: {e}"));
    assert_eq!(
        u32::from(cs),
        real & 0xFF,
        "{ctx}: the working's low 8 bits are wrong"
    );
    assert_eq!(cs, sum8(body));
    cs
}

/// Header, addresses, `LL` and command of a frame (§2.1); returns `LL`.
fn check_layout(frame: &[u8], dst: u8, src: u8, code: u8, ctx: &str) -> usize {
    assert!(frame.len() >= 6, "{ctx}: frame shorter than 6 bytes");
    assert_eq!(frame[0], HEADER, "{ctx}: header (§2.1)");
    assert_eq!(frame[1], dst, "{ctx}: destination (§2.1)");
    assert_eq!(frame[2], src, "{ctx}: source (§2.1)");
    assert_eq!(frame[4], code, "{ctx}: command / response code (§2.1)");
    usize::from(frame[3])
}

/// A complete frame: layout, `LL = len − 5` and checksum (§2.1, §2.2).
/// Returns the bytes between the command byte and the checksum.
fn check_frame<'a>(frame: &'a [u8], dst: u8, src: u8, code: u8, ctx: &str) -> &'a [u8] {
    let ll = check_layout(frame, dst, src, code, ctx);
    assert_eq!(
        ll,
        frame.len() - 5,
        "{ctx}: LL must be frame length - 5 (§2.1)"
    );
    let last = frame.len() - 1;
    assert_eq!(frame[last], sum8(&frame[..last]), "{ctx}: checksum (§2.2)");
    &frame[5..last]
}

/// What a request implies about its reply.
#[derive(Debug)]
enum Expect {
    Init,
    A8 { addresses: usize },
    A0 { count: usize },
    B8 { value: u8 },
    B0 { data: Vec<u8> },
}

impl Expect {
    /// The reply's `LL` (§3.2–§3.7), when the request fixes it.
    fn reply_ll(&self) -> Option<usize> {
        match self {
            Self::Init => None,
            Self::A8 { addresses } => Some(addresses + 1),
            Self::A0 { count } => Some(count + 1),
            Self::B8 { .. } => Some(2),
            Self::B0 { data } => Some(data.len() + 1),
        }
    }
}

/// Request rules per command (§3.1–§3.7); checks `decoded` against the
/// request bytes.
fn check_request(x: &Exchange, tx: &[u8], decoded: &Decoded, ctx: &str) -> Expect {
    let module = module_address(&x.module, ctx);
    let command = hex(&x.command, ctx)[0];
    let params = check_frame(tx, module, TESTER, command, ctx);
    let working = x
        .tx_checksum
        .as_deref()
        .unwrap_or_else(|| panic!("{ctx}: tx_checksum missing"));
    check_working(working, &tx[..tx.len() - 1], &format!("{ctx} tx_checksum"));
    let ll = usize::from(tx[3]);
    match command {
        0xBF => {
            assert_eq!(ll, 1, "{ctx}: BF request LL is 1 (§3.2)");
            Expect::Init
        }
        0xA8 => {
            let pp = params[0];
            assert!(pp <= 1, "{ctx}: PP is 00 or 01 (§3.1)");
            let want_pp = if x.state == "single-shot" { 0 } else { 1 };
            assert_eq!(pp, want_pp, "{ctx}: PP for state `{}` (§3.1, §5)", x.state);
            let addrs = &params[1..];
            assert_eq!(addrs.len() % 3, 0, "{ctx}: A8 addresses are 3 bytes (§3.3)");
            let n = addrs.len() / 3;
            assert!(
                (1..=A8_MAX_ADDRESSES).contains(&n),
                "{ctx}: A8 N = {n}, limit {A8_MAX_ADDRESSES} (§3.3)"
            );
            assert_eq!(ll, 3 * n + 2, "{ctx}: A8 LL = 3N + 2 (§3.3)");
            let listed = decoded
                .addresses
                .as_ref()
                .unwrap_or_else(|| panic!("{ctx}: decoded.addresses"));
            let from_tx: Vec<String> = addrs.chunks(3).map(upper_hex).collect();
            assert_eq!(
                &from_tx, listed,
                "{ctx}: decoded.addresses vs request (big-endian, §3)"
            );
            assert_eq!(
                decoded.pp.as_deref(),
                Some(format!("{pp:02X}").as_str()),
                "{ctx}: decoded.pp"
            );
            Expect::A8 { addresses: n }
        }
        0xA0 => {
            assert_eq!(ll, 6, "{ctx}: A0 LL is always 6 (§3.4)");
            let count = usize::from(params[4]) + 1;
            assert!(
                (1..=A0_MAX_COUNT).contains(&count),
                "{ctx}: A0 count {count} (§3.4)"
            );
            assert_eq!(
                decoded.count,
                Some(count),
                "{ctx}: decoded.count = CC + 1 (§3.4)"
            );
            assert_eq!(
                decoded.pp.as_deref(),
                Some(format!("{:02X}", params[0]).as_str()),
                "{ctx}: decoded.pp"
            );
            assert_eq!(params[0], 0, "{ctx}: these A0 fixtures read once (§3.1)");
            let addr = decoded
                .address
                .as_deref()
                .unwrap_or_else(|| panic!("{ctx}: decoded.address"));
            assert_eq!(params[1..4], hex24(addr, ctx), "{ctx}: A0 address (§3.4)");
            Expect::A0 { count }
        }
        0xB8 => {
            assert_eq!(ll, 5, "{ctx}: B8 LL is 5 (§3.5)");
            let addr = decoded
                .address
                .as_deref()
                .unwrap_or_else(|| panic!("{ctx}: decoded.address"));
            assert_eq!(params[..3], hex24(addr, ctx), "{ctx}: B8 address (§3.5)");
            let value = hex(
                decoded
                    .value
                    .as_deref()
                    .unwrap_or_else(|| panic!("{ctx}: decoded.value")),
                ctx,
            );
            assert_eq!(params[3..], value, "{ctx}: B8 value (§3.5)");
            Expect::B8 { value: params[3] }
        }
        0xB0 => {
            let data = params[3..].to_vec();
            assert!(
                (1..=B0_MAX_DATA).contains(&data.len()),
                "{ctx}: B0 n (§3.7)"
            );
            assert_eq!(ll, data.len() + 4, "{ctx}: B0 LL = n + 4 (§3.7)");
            let addr = decoded
                .address
                .as_deref()
                .unwrap_or_else(|| panic!("{ctx}: decoded.address"));
            assert_eq!(params[..3], hex24(addr, ctx), "{ctx}: B0 address (§3.7)");
            let listed = hex(
                decoded
                    .data
                    .as_deref()
                    .unwrap_or_else(|| panic!("{ctx}: decoded.data")),
                ctx,
            );
            assert_eq!(data, listed, "{ctx}: decoded.data (§3.7)");
            Expect::B0 { data }
        }
        other => panic!("{ctx}: command {other:02X} is not a K-line SSM command (§3)"),
    }
}

/// Reply rules per command; `data` is the bytes after the response code.
fn check_reply_data(
    expect: &Expect,
    data: &[u8],
    frame_index: usize,
    decoded: &Decoded,
    ctx: &str,
) {
    match expect {
        Expect::Init => {
            assert!(
                data.len() >= 8,
                "{ctx}: init data holds 3 bytes + 5-byte ECU ID (§3.2)"
            );
            let listed = hex(
                decoded
                    .init_data
                    .as_deref()
                    .unwrap_or_else(|| panic!("{ctx}: decoded.init_data")),
                ctx,
            );
            assert_eq!(data, listed, "{ctx}: decoded.init_data (§3.2)");
            assert_eq!(
                decoded.ecu_id.as_deref(),
                Some(upper_hex(&data[3..8]).as_str()),
                "{ctx}: ECU ID is init-data 3..=7, upper-case hex (§3.2)"
            );
            for c in decoded.capability.iter().flatten() {
                assert!(c.bit < 8, "{ctx}: capability bit {}", c.bit);
                let supported = c.index < data.len() && data[c.index] & (1 << c.bit) != 0;
                assert_eq!(
                    supported, c.supported,
                    "{ctx}: capability ({}, {}) (§3.2)",
                    c.index, c.bit
                );
            }
        }
        Expect::A8 { addresses } => {
            assert_eq!(
                data.len(),
                *addresses,
                "{ctx}: one value per address (§3.3)"
            );
            if let Some(values) = &decoded.values {
                let v = values
                    .get(frame_index)
                    .unwrap_or_else(|| panic!("{ctx}: decoded.values[{frame_index}]"));
                assert_eq!(
                    data,
                    hex(v, ctx),
                    "{ctx}: decoded.values[{frame_index}] (§3.3)"
                );
            }
        }
        Expect::A0 { count } => {
            assert_eq!(
                data.len(),
                *count,
                "{ctx}: A0 reply carries count bytes (§3.4)"
            );
            let listed = hex(
                decoded
                    .data
                    .as_deref()
                    .unwrap_or_else(|| panic!("{ctx}: decoded.data")),
                ctx,
            );
            assert_eq!(data, listed, "{ctx}: decoded.data (§3.4)");
        }
        Expect::B8 { value } => {
            assert_eq!(data, [*value], "{ctx}: B8 ack carries the value (§3.5)")
        }
        Expect::B0 { data: sent } => assert_eq!(
            data,
            sent.as_slice(),
            "{ctx}: B0 ack carries the data (§3.7)"
        ),
    }
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

fn check_exchange(x: &Exchange, md: &str, file: &Path) {
    let ctx = format!("{}: {}", file.display(), x.name);
    let ctx = ctx.as_str();
    assert!(
        x.spec.starts_with("docs/legacy/ssm.md §"),
        "{ctx}: spec must cite ssm.md sections"
    );
    for sec in x
        .spec
        .split(['§', ','])
        .skip(1)
        .map(str::trim)
        .filter(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))
    {
        let found = if sec.contains('.') {
            md.contains(&format!("\n### {sec} "))
        } else {
            md.contains(&format!("\n## {sec}. "))
        };
        assert!(found, "{ctx}: ssm.md has no section {sec}");
    }
    for ul in x.spec.split(", ").filter(|s| s.starts_with("UL-SSM-")) {
        assert!(
            md.contains(&format!("| {ul} |")),
            "{ctx}: ssm.md has no {ul}"
        );
    }
    let empty = Decoded::default();
    let decoded = x.decoded.as_ref().unwrap_or(&empty);
    let module = module_address(&x.module, ctx);
    let command = hex(&x.command, ctx);
    assert_eq!(command.len(), 1, "{ctx}: command is one byte");
    let code = command[0].wrapping_add(RESPONSE_OFFSET);
    assert!(
        matches!(code, 0xFF | 0xE8 | 0xE0 | 0xF8 | 0xF0),
        "{ctx}: response code set (§2.1, §4 check 6)"
    );

    let tx = hex(&x.tx, ctx);
    let rx: Vec<Vec<u8>> = x.rx.iter().map(|f| hex(f, ctx)).collect();
    let wire = hex(&x.wire, ctx);

    let expect = match x.state.as_str() {
        "single-shot" | "continuous-first" => {
            assert!(
                !tx.is_empty(),
                "{ctx}: state `{}` writes a request",
                x.state
            );
            check_request(x, &tx, decoded, ctx)
        }
        "continuous-stream" => {
            assert!(
                tx.is_empty(),
                "{ctx}: no request is written in STATE_1 (§5)"
            );
            assert_eq!(x.command, "A8", "{ctx}: only A8 streams (§3.1)");
            assert_eq!(
                decoded.pp.as_deref(),
                Some("01"),
                "{ctx}: a stream is PP = 01 (§3.1)"
            );
            let n = decoded.addresses.as_ref().map_or(0, Vec::len);
            assert!(
                (1..=A8_MAX_ADDRESSES).contains(&n),
                "{ctx}: decoded.addresses"
            );
            Expect::A8 { addresses: n }
        }
        other => panic!("{ctx}: unknown state `{other}`"),
    };
    if x.state == "continuous-first" {
        assert_eq!(x.command, "A8", "{ctx}: only A8 streams (§3.1)");
    }

    let error = x.error.as_deref();
    match x.outcome.as_str() {
        "ok" => {
            assert!(
                error.is_none() && x.legacy.is_none() && x.rr.is_none(),
                "{ctx}: ok case with error fields"
            );
            assert!(
                x.meaning.as_deref().is_some_and(|m| !m.is_empty()),
                "{ctx}: meaning missing"
            );
        }
        "error" => {
            assert!(
                x.legacy.as_deref().is_some_and(|s| !s.is_empty()),
                "{ctx}: legacy behaviour missing"
            );
            assert!(
                x.rr.as_deref().is_some_and(|s| !s.is_empty()),
                "{ctx}: rr behaviour missing"
            );
        }
        other => panic!("{ctx}: unknown outcome `{other}`"),
    }

    // Replies.
    let complete = !matches!(error, Some("short-reply" | "silent"));
    if complete {
        assert_eq!(
            x.rx_checksum.len(),
            rx.len(),
            "{ctx}: one rx_checksum per frame"
        );
    } else {
        assert!(
            x.rx_checksum.is_empty(),
            "{ctx}: an incomplete frame has no checksum working"
        );
    }
    for (i, frame) in rx.iter().enumerate() {
        let fctx = format!("{ctx} rx[{i}]");
        if !complete {
            let ll = check_layout(frame, TESTER, module, code, &fctx);
            assert!(
                frame.len() < ll + 5,
                "{fctx}: a short reply has fewer than LL + 5 bytes (§2.1)"
            );
            if let Some(want) = expect.reply_ll() {
                assert_eq!(ll, want, "{fctx}: LL of the reply being cut short");
            }
            continue;
        }
        let last = frame.len() - 1;
        let correct = check_working(
            &x.rx_checksum[i],
            &frame[..last],
            &format!("{fctx} rx_checksum"),
        );
        if error == Some("bad-checksum") {
            assert_ne!(
                frame[last], correct,
                "{fctx}: a bad-checksum case must carry a wrong checksum"
            );
        }
        // A bad-checksum frame must be good in every other respect.
        let mut fixed = frame.clone();
        fixed[last] = correct;
        let data = check_frame(&fixed, TESTER, module, code, &fctx);
        if let Some(want) = expect.reply_ll() {
            assert_eq!(
                usize::from(frame[3]),
                want,
                "{fctx}: reply LL for the request (§3)"
            );
        }
        check_reply_data(&expect, data, i, decoded, &fctx);
    }
    match error {
        Some("silent") => assert!(rx.is_empty(), "{ctx}: a silent ECU sends nothing"),
        Some("bad-checksum" | "short-reply" | "echo-mismatch" | "no-echo") | None => {
            assert!(!rx.is_empty(), "{ctx}: a reply frame is expected");
        }
        Some(other) => panic!("{ctx}: unknown error `{other}`"),
    }
    if x.state != "continuous-stream" && error != Some("silent") {
        assert_eq!(rx.len(), 1, "{ctx}: one reply per written request");
    }

    // The K-line echo (§1.4): what the tester's receive buffer holds.
    let rx_all: Vec<&[u8]> = rx.iter().map(Vec::as_slice).collect();
    let rx_all = concat(&rx_all);
    match error {
        Some("echo-mismatch") => {
            assert_eq!(
                wire.len(),
                tx.len() + rx_all.len(),
                "{ctx}: echo + reply length"
            );
            assert_ne!(
                &wire[..tx.len()],
                tx.as_slice(),
                "{ctx}: the echo must differ from the request"
            );
            assert_eq!(
                &wire[tx.len()..],
                rx_all.as_slice(),
                "{ctx}: the reply follows the echo"
            );
        }
        Some("no-echo") => assert_eq!(
            wire, rx_all,
            "{ctx}: without echo the wire is the reply alone"
        ),
        _ => assert_eq!(
            wire,
            concat(&[&tx, &rx_all]),
            "{ctx}: wire = echo of tx, then the reply (§1.4)"
        ),
    }
    if let (Expect::A8 { addresses: n }, "single-shot" | "continuous-first") =
        (&expect, x.state.as_str())
    {
        let buffer = (n + 6) + (3 * n + 7);
        if complete && error != Some("no-echo") {
            assert_eq!(
                wire.len(),
                buffer,
                "{ctx}: A8 buffer (N+6)+(3N+7) (§1.4, §3.3)"
            );
        } else {
            assert!(
                wire.len() < buffer,
                "{ctx}: a short A8 exchange fills less than (N+6)+(3N+7)"
            );
        }
    }
    if x.state == "continuous-stream" {
        let Expect::A8 { addresses: n } = expect else {
            unreachable!()
        };
        assert!(
            rx.iter().all(|f| f.len() == n + 6),
            "{ctx}: each streamed frame is N + 6 bytes (§3.3)"
        );
    }
}

fn load_all() -> Vec<(PathBuf, Fixture)> {
    let mut files: Vec<PathBuf> = fs::read_dir(ssm_dir())
        .expect("fixtures/public/ssm")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).expect("read fixture");
            let f: Fixture =
                toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p, f)
        })
        .collect()
}

#[test]
fn every_exchange_follows_the_frame_rules() {
    let md = ssm_md();
    let all = load_all();
    assert!(!all.is_empty(), "no SSM fixtures");
    for (path, f) in &all {
        let ctx = path.display();
        assert_eq!(f.schema, 1, "{ctx}: schema");
        assert_eq!(f.kind, "ssm-kline-sim", "{ctx}: kind");
        assert!(
            !f.title.is_empty() && !f.synthetic.is_empty(),
            "{ctx}: title and synthetic note"
        );
        assert!(
            !f.changelog.is_empty() && f.changelog.iter().all(|c| !c.reason.is_empty()),
            "{ctx}: changelog"
        );
        for id in &f.fixture_ids {
            assert!(
                md.contains(&format!("| {id} |")),
                "{ctx}: {id} is not in ssm.md's fixture list"
            );
        }
        assert!(!f.exchange.is_empty(), "{ctx}: no exchanges");
        for x in &f.exchange {
            check_exchange(x, &md, path);
        }
    }
}

/// The set US-RR-12-4 asks for: init, A8 with one and several addresses,
/// A0, B8 and B0 with their acks, an echo case, and the bad-checksum and
/// short/timeout error paths.
#[test]
fn the_required_cases_are_present() {
    let all = load_all();
    let xs: Vec<&Exchange> = all.iter().flat_map(|(_, f)| f.exchange.iter()).collect();
    let has = |pred: &dyn Fn(&Exchange) -> bool, what: &str| {
        assert!(xs.iter().any(|x| pred(x)), "no fixture covers {what}");
    };
    let ok = |x: &Exchange| x.outcome == "ok";
    let n_addr = |x: &Exchange| {
        x.decoded
            .as_ref()
            .and_then(|d| d.addresses.as_ref())
            .map_or(0, Vec::len)
    };
    has(
        &|x| ok(x) && x.command == "BF" && x.module == "ecu",
        "ECU init",
    );
    has(
        &|x| ok(x) && x.command == "A8" && x.state == "single-shot" && n_addr(x) == 1,
        "A8 with one address",
    );
    has(
        &|x| ok(x) && x.command == "A8" && x.state == "single-shot" && n_addr(x) > 1,
        "A8 with N addresses",
    );
    has(
        &|x| ok(x) && x.command == "A8" && x.state == "continuous-stream",
        "A8 continuous stream",
    );
    has(&|x| ok(x) && x.command == "A0", "A0 block read");
    has(&|x| ok(x) && x.command == "B8", "B8 write + ack");
    has(&|x| ok(x) && x.command == "B0", "B0 write + ack");
    has(
        &|x| x.error.as_deref() == Some("echo-mismatch"),
        "a corrupt echo",
    );
    has(
        &|x| x.error.as_deref() == Some("no-echo"),
        "an interface without echo",
    );
    has(
        &|x| x.error.as_deref() == Some("bad-checksum"),
        "a bad checksum",
    );
    has(
        &|x| x.error.as_deref() == Some("short-reply"),
        "a short reply / timeout",
    );
    has(&|x| x.error.as_deref() == Some("silent"), "a silent ECU");
}

/// The checker itself rejects frames that break the rules, so a passing
/// run means something.
#[test]
fn the_rules_reject_broken_frames() {
    use std::panic::catch_unwind;
    let good = hex("80 F0 10 02 E8 64 CE", "good");
    assert_eq!(check_frame(&good, TESTER, 0x10, 0xE8, "good"), [0x64]);
    let mut variants = Vec::new();
    for (i, b) in [
        (0, 0x81),
        (1, 0xF1),
        (2, 0x18),
        (3, 0x03),
        (4, 0xE0),
        (6, 0xCF),
    ] {
        let mut f = good.clone();
        f[i] = b;
        variants.push(f);
    }
    for f in variants {
        let r = catch_unwind(|| check_frame(&f, TESTER, 0x10, 0xE8, "broken").to_vec());
        assert!(r.is_err(), "{} passed", upper_hex(&f));
    }
    // Working that lists the wrong bytes, or the wrong sum.
    assert!(
        catch_unwind(|| check_working("80+F0+10+02+E8+65 = 0x2CF; low 8 bits CF", &good[..6], "w"))
            .is_err()
    );
    assert!(
        catch_unwind(|| check_working("80+F0+10+02+E8+64 = 0x2CF; low 8 bits CF", &good[..6], "w"))
            .is_err()
    );
    assert_eq!(
        check_working("80+F0+10+02+E8+64 = 0x2CE; low 8 bits CE", &good[..6], "w"),
        0xCE
    );
}
