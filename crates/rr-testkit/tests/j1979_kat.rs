//! The SAE J1979 service $01 PID vectors (`fixtures/kat/j1979-01-*.toml`,
//! US-RR-14-6, PROJECT.md T2 (d)) run end to end through rr-core.
//!
//! The vectors are ordinary `[[decode]]` fixtures: an inline big-endian
//! encoding of a PID's data bytes (a bitfield picks one signal out of a
//! multi-signal PID), the J1979 scaling as the conversion, and positive
//! service $01 responses as the hex input. The generic seed tests already
//! decode them (`encoding_codec`, `reference`) and convert them
//! (`conversion_kat`); this file checks what is J1979-specific, so the
//! vectors are exercised before an OBD codec exists (US-RR-77):
//!
//! * every case's cell is the data of one response: the two bytes before it
//!   are `0x41` (service `0x01` + `0x40`) and the PID echoed, and the cell
//!   ends where the response does ([`every_case_is_the_data_of_one_mode_01_response`]);
//! * bytes → rr-core's `Encoding::decode` → rr-core's `Conversion` gives the
//!   case's raw and real value ([`every_vector_decodes_through_rr_core`]);
//! * every included PID ([`PIDS`], pinned against the legacy logger list in
//!   the fixture headers) has vectors, and each of its signals a case whose
//!   data bytes are all `0x00`, one whose data bytes are all `0xFF`, cases at
//!   its lowest and highest raw value expecting the range endpoints its
//!   `Formula:` note documents, and a typical value (US-RR-14-4,
//!   [`every_included_pid_has_zero_and_max_vectors_at_its_documented_range`]);
//! * every fixture names SAE J1979 and its secondary sources in `notes`
//!   ([`every_fixture_cites_its_sources`]).
//!
//! Expected values come from the fixtures (hand-calculated from the cited
//! formulas), never from rr-core (D-024). This file is a test boundary: it
//! converts the fixture vocabulary to rr-core's types here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use rr_core::conversion::{Conversion, ConversionSpec, FormSpec};
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, Storage};
use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, DecodeCase, Definition, Input, KatKind, RawValue};

/// The service $01 PIDs with a scaling formula that legacy RomRaider's OBD
/// logger offers (the `<protocol id="OBD">` parameters of the pinned logger
/// definition; each fixture's header comment gives the line of each). The
/// bit-encoded and enumerated PIDs (01, 03, 12, 13, 1C, 1D, 1E, 51, 5F, 65)
/// have no formula, and PID 54 is left out because its sources disagree on
/// its signedness (`fixtures/README.md`, "J1979 PID vectors").
const PIDS: &[u8] = &[
    0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x14, 0x15,
    0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1F, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29,
    0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
    0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B,
    0x4C, 0x4D, 0x4E, 0x52, 0x53, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x61,
    0x62, 0x63,
];

/// The positive-response service id of a service $01 request.
const MODE_01_RESPONSE: u8 = 0x41;

/// One J1979 fixture, as this test needs it.
struct J1979Fixture {
    name: String,
    /// The PIDs of the file's header comment, each with its line in the
    /// pinned logger definition (see [`header_pids`]).
    header_pids: Result<Vec<u8>, String>,
    notes: String,
    encoding: kat::Encoding,
    conversion: kat::Conversion,
    bytes: Vec<u8>,
    cases: Vec<DecodeCase>,
}

#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn j1979_fixtures() -> Vec<J1979Fixture> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|ext| ext == "toml")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("j1979-01-"))
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures/kat/j1979-01-*.toml");
    paths
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let KatKind::Decode {
                definition, cases, ..
            } = fixture.data.kind
            else {
                panic!("{name}: a J1979 vector fixture has [[decode]] cases");
            };
            let Definition::Inline {
                encoding,
                conversion: Some(conversion),
            } = definition
            else {
                panic!("{name}: a J1979 vector fixture has an inline encoding and conversion");
            };
            let Some(Input::Hex(hex)) = fixture.data.input else {
                panic!("{name}: a J1979 vector fixture's input is the response bytes, as hex");
            };
            assert_eq!(
                encoding.endian,
                kat::Endian::Big,
                "{name}: J1979 data is big-endian"
            );
            assert_eq!(encoding.layout, kat::Layout::Default, "{name}");
            assert!(
                conversion.units.is_some(),
                "{name}: the conversion names its unit"
            );
            J1979Fixture {
                name,
                header_pids: header_pids(&text),
                notes: fixture.data.notes.unwrap_or_default(),
                encoding,
                conversion,
                bytes: hex.0,
                cases,
            }
        })
        .collect()
}

fn core_encoding(encoding: &kat::Encoding) -> Result<Encoding, String> {
    let storage = match encoding.storage {
        kat::Storage::U8 => Storage::U8,
        kat::Storage::U16 => Storage::U16,
        kat::Storage::U32 => Storage::U32,
        kat::Storage::I16 => Storage::I16,
        other => return Err(format!("{other} is not a J1979 data width here")),
    };
    let bitfield = encoding
        .bitfield
        .map(|b| BitField::new(b.mask, b.shift, false))
        .transpose()
        .map_err(|e| format!("bitfield refused: {e}"))?;
    Encoding::new(storage, Endian::Big, bitfield, Layout::Default)
        .map_err(|e| format!("encoding refused: {e}"))
}

/// The cell's width in bytes: the whole data of the response.
const fn width(storage: kat::Storage) -> usize {
    match storage {
        kat::Storage::U8 | kat::Storage::I8 => 1,
        kat::Storage::U16 | kat::Storage::I16 => 2,
        kat::Storage::U20 | kat::Storage::U24 | kat::Storage::I24 => 3,
        kat::Storage::U32 | kat::Storage::I32 | kat::Storage::F32 => 4,
        kat::Storage::U64 | kat::Storage::I64 | kat::Storage::F64 => 8,
    }
}

/// The PID and data bytes of `case`'s response, or why it is not one.
fn response(fixture: &J1979Fixture, case: &DecodeCase) -> Result<(u8, Vec<u8>), String> {
    let offset = usize::try_from(case.offset).map_err(|e| e.to_string())?;
    let end = offset + width(fixture.encoding.storage);
    if offset < 2 || end > fixture.bytes.len() {
        return Err(format!(
            "offset {offset}: no room for the 0x41 / PID header and the data"
        ));
    }
    let (sid, pid) = (fixture.bytes[offset - 2], fixture.bytes[offset - 1]);
    if sid != MODE_01_RESPONSE {
        return Err(format!(
            "offset {offset}: byte before the PID is 0x{sid:02X}, not 0x41"
        ));
    }
    if end < fixture.bytes.len() && fixture.bytes[end] != MODE_01_RESPONSE {
        return Err(format!(
            "offset {offset}: the data runs past {} bytes into 0x{:02X}, not the next response",
            end - offset,
            fixture.bytes[end]
        ));
    }
    Ok((pid, fixture.bytes[offset..end].to_vec()))
}

#[test]
fn every_case_is_the_data_of_one_mode_01_response() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for fixture in j1979_fixtures() {
        for (index, case) in fixture.cases.iter().enumerate() {
            checked += 1;
            if let Err(e) = response(&fixture, case) {
                failures.push(format!("{} [[decode]] #{}: {e}", fixture.name, index + 1));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(checked >= 3 * PIDS.len(), "only {checked} vectors");
}

#[test]
fn every_vector_decodes_through_rr_core() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for fixture in j1979_fixtures() {
        let spec = ConversionSpec {
            form: FormSpec::Expr {
                to_real: &fixture.conversion.to_real,
                to_raw: fixture.conversion.to_raw.as_deref(),
            },
            unit: fixture.conversion.units.as_deref(),
            format: None,
        };
        let built = match (
            core_encoding(&fixture.encoding),
            Conversion::from_spec(&spec),
        ) {
            (Ok(encoding), Ok(conversion)) => (encoding, conversion),
            (Err(e), _) => {
                failures.push(format!("{}: {e}", fixture.name));
                continue;
            }
            (_, Err(e)) => {
                failures.push(format!("{}: conversion refused: {e}", fixture.name));
                continue;
            }
        };
        let (encoding, conversion) = built;
        for (index, case) in fixture.cases.iter().enumerate() {
            let label = format!("{} [[decode]] #{}", fixture.name, index + 1);
            let Some(expected) = case.real else {
                failures.push(format!("{label}: a J1979 vector has a real value"));
                continue;
            };
            checked += 1;
            let offset = usize::try_from(case.offset).unwrap_or(usize::MAX);
            let Some(bytes) = fixture.bytes.get(offset..) else {
                failures.push(format!("{label}: offset {offset} is past the input"));
                continue;
            };
            let raw = match encoding.decode(bytes) {
                Ok(Raw::Int(raw)) if RawValue::Int(raw) == case.raw => raw,
                other => {
                    failures.push(format!(
                        "{label}: decode expected raw {:?}, got {other:?}",
                        case.raw
                    ));
                    continue;
                }
            };
            let tolerance = case.tolerance.unwrap_or(0.0);
            match conversion.to_real_raw(Raw::Int(raw)) {
                Ok(real) if (real - expected).abs() <= tolerance => {}
                other => failures.push(format!(
                    "{label}: raw {raw} expected {expected} (± {tolerance}), got {other:?}"
                )),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(checked >= 3 * PIDS.len(), "only {checked} vectors");
    eprintln!("rr-core decoded {checked} J1979 vector(s)");
}

/// The PIDs whose data carries two signals that decode independently, one
/// fixture per signal on the same responses, each picking its bytes with a
/// bitfield (`fixtures/README.md`, "J1979 PID vectors"):
///
/// * 0x14–0x1B, oxygen sensor voltage (byte A) and short term fuel trim (B):
///   `j1979-01-o2-voltage-u16.toml`, `j1979-01-o2-trim-u16.toml`;
/// * 0x24–0x2B, wideband lambda (A, B) and voltage (C, D):
///   `j1979-01-wideband-lambda-u32.toml`, `j1979-01-wideband-voltage-u32.toml`;
/// * 0x34–0x3B, wideband lambda (A, B) and current (C, D):
///   `j1979-01-wideband-lambda-u32.toml`, `j1979-01-wideband-current-u32.toml`;
/// * 0x55–0x58, secondary oxygen sensor trim of two banks (A and B):
///   `j1979-01-secondary-o2-trim-a-u16.toml`, `j1979-01-secondary-o2-trim-b-u16.toml`.
///
/// Every other PID of [`PIDS`] is one signal over all its data bytes.
const TWO_SIGNAL_PIDS: &[u8] = &[
    0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B,
    0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x55, 0x56, 0x57, 0x58,
];

/// The PIDs a fixture's header comment lists after "Each PID and the line of
/// its <parameter> there:", as `XX line NNNNN` items (a hex PID and its line
/// in the pinned logger definition), comma-separated over `#` lines up to a
/// bare `#`.
fn header_pids(text: &str) -> Result<Vec<u8>, String> {
    const INTRO: &str = "Each PID and the line of its <parameter> there:";
    let mut lines = text.lines().skip_while(|l| !l.contains(INTRO));
    if lines.next().is_none() {
        return Err(format!("header has no {INTRO:?} list"));
    }
    let list: Vec<&str> = lines
        .take_while(|l| l.trim() != "#")
        .map(|l| l.strip_prefix("# ").unwrap_or(l))
        .collect();
    let mut pids = Vec::new();
    for item in list.join(" ").split(',') {
        let item = item.trim().trim_end_matches('.');
        if item.is_empty() {
            continue;
        }
        let words: Vec<&str> = item.split_whitespace().collect();
        let [pid, "line", line] = words.as_slice() else {
            return Err(format!("header item {item:?} is not `XX line N`"));
        };
        let pid = u8::from_str_radix(pid, 16).map_err(|e| format!("{item:?}: {e}"))?;
        line.parse::<u32>()
            .map_err(|e| format!("{item:?}: line: {e}"))?;
        pids.push(pid);
    }
    if pids.is_empty() {
        return Err("header lists no PID".to_owned());
    }
    Ok(pids)
}

/// The formula's range as `notes` documents it: the first `<lo> to <hi>`
/// with two numbers on the `Formula:` line (e.g. "0 to 25700 %",
/// "-40 to 215 degC").
fn documented_range(notes: &str) -> Result<(f64, f64), String> {
    let line = notes
        .lines()
        .find(|l| l.starts_with("Formula:"))
        .ok_or("notes have no `Formula:` line")?;
    let number = |w: &str| w.trim_end_matches(['.', ',', ':', ';']).parse::<f64>().ok();
    let words: Vec<&str> = line.split_whitespace().collect();
    words
        .windows(3)
        .find_map(|w| match w {
            [lo, "to", hi] => Some((number(lo)?, number(hi)?)),
            _ => None,
        })
        .ok_or_else(|| format!("no `<lo> to <hi>` range on {line:?}"))
}

/// Bits in the storage of `encoding`, for the storages J1979 data uses.
fn storage_bits(storage: kat::Storage) -> Result<u32, String> {
    match storage {
        kat::Storage::U8 => Ok(8),
        kat::Storage::U16 | kat::Storage::I16 => Ok(16),
        kat::Storage::U32 => Ok(32),
        other => Err(format!("{other} is not a J1979 data width here")),
    }
}

/// The bits of the cell that are the signal's: the bitfield mask, else all.
fn signal_mask(encoding: &kat::Encoding) -> Result<u64, String> {
    let all = (1_u64 << storage_bits(encoding.storage)?) - 1;
    Ok(encoding.bitfield.map_or(all, |b| b.mask))
}

/// The raw range of one signal: `(min, max)`. An unsigned signal is
/// `0 ..= mask >> shift` (every one of its bits set); the signed one
/// (`i16`, PID 0x32) is the two's complement range of its storage.
fn raw_range(encoding: &kat::Encoding) -> Result<(i128, i128), String> {
    let bits = storage_bits(encoding.storage)?;
    if encoding.storage == kat::Storage::I16 {
        if encoding.bitfield.is_some() {
            return Err("a bitfield over signed storage is not a J1979 shape here".to_owned());
        }
        let half = 1_i128 << (bits - 1);
        return Ok((-half, half - 1));
    }
    let max = match encoding.bitfield {
        Some(b) => {
            let shift = b.shift.unwrap_or(b.mask.trailing_zeros());
            let field = b.mask >> shift;
            if field << shift != b.mask || (field & field.wrapping_add(1)) != 0 {
                return Err(format!(
                    "bitfield mask 0x{:X} is not one contiguous field",
                    b.mask
                ));
            }
            field
        }
        None => (1_u64 << bits) - 1,
    };
    Ok((0, i128::from(max)))
}

/// One signal's cases: (data bytes, case) of each.
type SignalCases<'a> = Vec<(Vec<u8>, &'a DecodeCase)>;

/// US-RR-14-4: the J1979 vectors include boundary values (0 and max) for
/// every included PID.
///
/// * **Included PID**: a service $01 PID of [`PIDS`], the 82 PIDs with a
///   formula that legacy RomRaider's OBD logger offers. The list is pinned
///   three ways, so a PID dropped from the fixtures fails: it equals the
///   union of the PIDs the fixture headers list with their line in the
///   pinned logger definition; each fixture's responses are exactly the
///   PIDs its header lists; and every PID of [`PIDS`] has responses.
/// * **Signal**: one fixture's reading of one PID. A PID of
///   [`TWO_SIGNAL_PIDS`] has exactly two fixtures whose bitfield masks are
///   disjoint and together cover every data bit (the signals decode
///   independently); every other included PID has exactly one fixture.
/// * **0**: a case whose data bytes are all `0x00`; its raw value is 0.
/// * **max**: a case whose data bytes are all `0xFF`; its raw value is the
///   signal's largest (every bit of its field set), or −1 for the signed
///   signal (PID 0x32, `i16`).
/// * **Range endpoints**: each signal has a case at its lowest raw value and
///   one at its highest ([`raw_range`]: `0` and every field bit set; for
///   `i16` `0x8000` and `0x7FFF`), and their expected real values are the
///   `<lo> to <hi>` endpoints documented on the fixture's `Formula:` line,
///   so a boundary case tests the boundary. (rr-core producing each case's
///   real value is [`every_vector_decodes_through_rr_core`].)
/// * Each signal also has at least one more case, a typical value.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one criterion, checked rule by rule with its failure messages"
)]
fn every_included_pid_has_zero_and_max_vectors_at_its_documented_range() {
    let fixtures = j1979_fixtures();
    let mut failures = Vec::new();

    // Included PIDs: PIDS, the headers' legacy logger list, and the fixtures.
    let included: BTreeSet<u8> = PIDS.iter().copied().collect();
    if included.len() != 82 || PIDS.len() != 82 {
        failures.push(format!(
            "PIDS has {} distinct of {} entries; the legacy logger list has 82",
            included.len(),
            PIDS.len()
        ));
    }
    let mut documented = BTreeSet::new();
    let mut signals: BTreeMap<u8, Vec<&J1979Fixture>> = BTreeMap::new();
    let mut per_signal: BTreeMap<(&str, u8), SignalCases<'_>> = BTreeMap::new();
    for fixture in &fixtures {
        let listed: BTreeSet<u8> = match &fixture.header_pids {
            Ok(pids) => pids.iter().copied().collect(),
            Err(e) => {
                failures.push(format!("{}: {e}", fixture.name));
                BTreeSet::new()
            }
        };
        documented.extend(listed.iter().copied());
        let mut answered = BTreeSet::new();
        for (index, case) in fixture.cases.iter().enumerate() {
            match response(fixture, case) {
                Ok((pid, data)) => {
                    answered.insert(pid);
                    per_signal
                        .entry((fixture.name.as_str(), pid))
                        .or_default()
                        .push((data, case));
                }
                Err(e) => failures.push(format!("{} [[decode]] #{}: {e}", fixture.name, index + 1)),
            }
        }
        if answered != listed {
            failures.push(format!(
                "{}: responses are for PIDs {answered:02X?}, the header lists {listed:02X?}",
                fixture.name
            ));
        }
        for pid in answered {
            signals.entry(pid).or_default().push(fixture);
        }
    }
    if documented != included {
        failures.push(format!(
            "the fixture headers list PIDs {:02X?} not in PIDS and leave out {:02X?}",
            documented.difference(&included).collect::<Vec<_>>(),
            included.difference(&documented).collect::<Vec<_>>()
        ));
    }
    for pid in &included {
        if !signals.contains_key(pid) {
            failures.push(format!("PID 0x{pid:02X} has no vector"));
        }
    }
    for pid in signals.keys() {
        if !included.contains(pid) {
            failures.push(format!("PID 0x{pid:02X} has vectors but is not in PIDS"));
        }
    }
    assert!(
        TWO_SIGNAL_PIDS.iter().all(|pid| included.contains(pid)),
        "TWO_SIGNAL_PIDS is a subset of PIDS"
    );

    // Signals: one per PID, or two independent ones for TWO_SIGNAL_PIDS.
    for (pid, fixtures) in &signals {
        let expected = if TWO_SIGNAL_PIDS.contains(pid) { 2 } else { 1 };
        let names: Vec<&str> = fixtures.iter().map(|f| f.name.as_str()).collect();
        if fixtures.len() != expected {
            failures.push(format!(
                "PID 0x{pid:02X}: {expected} signal fixture(s) expected, found {names:?}"
            ));
            continue;
        }
        // The signals' masks are disjoint and together cover every data bit
        // (one signal: its mask is all of them), over the same data width.
        let storage = fixtures[0].encoding.storage;
        let all = storage_bits(storage).map(|bits| (1_u64 << bits) - 1);
        let masks: Result<Vec<u64>, String> =
            fixtures.iter().map(|f| signal_mask(&f.encoding)).collect();
        match (all, masks) {
            (Ok(all), Ok(masks)) => {
                let same_width = fixtures.iter().all(|f| f.encoding.storage == storage);
                let union = masks
                    .iter()
                    .try_fold(0_u64, |acc, &m| (acc & m == 0).then_some(acc | m));
                if !same_width || union != Some(all) {
                    failures.push(format!(
                        "PID 0x{pid:02X}: the signals of {names:?} do not split its data bits \
                         (masks {masks:X?}, data 0x{all:X})"
                    ));
                }
            }
            (Err(e), _) | (_, Err(e)) => failures.push(format!("PID 0x{pid:02X}: {e}")),
        }
    }

    // Per signal: the 0 and max cases, and the documented range endpoints.
    for fixture in &fixtures {
        let range = documented_range(&fixture.notes);
        let raws = raw_range(&fixture.encoding);
        let ((lo, hi), (raw_min, raw_max)) = match (range, raws) {
            (Ok(range), Ok(raws)) => (range, raws),
            (Err(e), _) | (_, Err(e)) => {
                failures.push(format!("{}: {e}", fixture.name));
                continue;
            }
        };
        let signed = fixture.encoding.storage == kat::Storage::I16;
        let raw_all_ff = if signed { -1 } else { raw_max };
        for ((name, pid), cases) in
            per_signal.range((fixture.name.as_str(), 0)..=(fixture.name.as_str(), u8::MAX))
        {
            let at = |raw: i128| cases.iter().find(|(_, c)| c.raw == RawValue::Int(raw));
            let with_bytes = |byte: u8| cases.iter().find(|(d, _)| d.iter().all(|&b| b == byte));
            let label = format!("{name}: PID 0x{pid:02X}");
            match with_bytes(0x00) {
                None => failures.push(format!("{label} has no case with every data byte 0x00")),
                Some((_, c)) if c.raw != RawValue::Int(0) => {
                    failures.push(format!(
                        "{label}: the all-0x00 case has raw {:?}, not 0",
                        c.raw
                    ));
                }
                Some(_) => {}
            }
            match with_bytes(0xFF) {
                None => failures.push(format!("{label} has no case with every data byte 0xFF")),
                Some((_, c)) if c.raw != RawValue::Int(raw_all_ff) => failures.push(format!(
                    "{label}: the all-0xFF case has raw {:?}, not {raw_all_ff}",
                    c.raw
                )),
                Some(_) => {}
            }
            for (raw, endpoint, which) in [(raw_min, lo, "lowest"), (raw_max, hi, "highest")] {
                match at(raw) {
                    None => failures.push(format!(
                        "{label} has no case at its {which} raw value {raw}"
                    )),
                    Some((_, c)) => {
                        let tolerance = c.tolerance.unwrap_or(0.0);
                        match c.real {
                            Some(real) if (real - endpoint).abs() <= tolerance => {}
                            other => failures.push(format!(
                                "{label}: raw {raw} expects {other:?}, but the documented \
                                 range's {which} end is {endpoint} (± {tolerance})"
                            )),
                        }
                    }
                }
            }
            let boundary = |c: &DecodeCase| {
                c.raw == RawValue::Int(raw_min)
                    || c.raw == RawValue::Int(raw_max)
                    || c.raw == RawValue::Int(0)
                    || c.raw == RawValue::Int(raw_all_ff)
            };
            if !cases.iter().any(|(_, c)| !boundary(c)) {
                failures.push(format!("{label} has no typical (non-boundary) case"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_fixture_cites_its_sources() {
    let mut failures = Vec::new();
    for fixture in j1979_fixtures() {
        for needle in [
            "Standard: SAE J1979",
            "Via (secondary): Wikipedia",
            "Via (secondary): CSS Electronics",
            "Framing: Elm Electronics ELM327 datasheet",
        ] {
            if !fixture.notes.contains(needle) {
                failures.push(format!("{}: notes do not contain {needle:?}", fixture.name));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
