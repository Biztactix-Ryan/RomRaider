//! The value syntaxes of legacy RomRaider ECU definitions: hexadecimal
//! integers, file sizes, hexadecimal byte strings and booleans.
//!
//! Each parser accepts what legacy accepted (cited per function) and returns
//! `None` for anything else; the caller reports it with the element's
//! position. Numbers are read after trimming XML whitespace around the
//! value, which legacy did not do (`Integer.parseInt(" 2000")` threw and
//! failed the whole file): a value that legacy accepted reads the same here.

/// Largest magnitude [`parse_hex_int`] accepts: 32 bits.
const MAX_HEX: u64 = 0xFFFF_FFFF;

/// A hexadecimal integer as legacy's `HexUtil.hexToInt` read it
/// (`util/HexUtil.java:161-163`: `Integer.parseInt(text.replace("0x", ""),
/// 16)`): an optional sign, an optional `0x` (or `0X`) prefix, then one or
/// more hex digits, either case. The magnitude is at most `FFFFFFFF`
/// (legacy: a signed 32-bit `int`, so at most `7FFFFFFF`; ROM addresses use
/// the full 32 bits).
///
/// Legacy removed every `0x` anywhere in the text (`20x00` read as `0x200`);
/// only a prefix is accepted here.
pub(super) fn parse_hex_int(text: &str) -> Option<i64> {
    let t = text.trim_matches(is_xml_space);
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    if digits.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    for c in digits.chars() {
        let d = u64::from(c.to_digit(16)?);
        value = value.checked_mul(16)?.checked_add(d)?;
        if value > MAX_HEX {
            return None;
        }
    }
    let value = i64::try_from(value).ok()?;
    Some(if negative { -value } else { value })
}

/// A `<filesize>` as legacy's `RomAttributeParser.parseFileSize` read it
/// (`xml/RomAttributeParser.java:238-250`): decimal digits with an optional
/// `kb` (× 1024), `mb` (× 1024²) or `b` suffix, any case. The result is
/// in bytes and fits a `u32`.
///
/// Legacy also accepted a sign (`Integer.parseInt`), so `-1kb` was a size of
/// −1024 bytes; a size has no sign here.
pub(super) fn parse_file_size(text: &str) -> Option<u32> {
    let t = text.trim_matches(is_xml_space);
    let (number, unit) = if let Some(n) = strip_suffix_ci(t, "kb") {
        (n, 1024)
    } else if let Some(n) = strip_suffix_ci(t, "mb") {
        (n, 1024 * 1024)
    } else if let Some(n) = strip_suffix_ci(t, "b") {
        (n, 1)
    } else {
        (t, 1)
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: u32 = 0;
    for b in number.bytes() {
        value = value.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
    }
    value.checked_mul(unit)
}

/// `text` without `suffix` at its end, compared ASCII case-insensitively.
fn strip_suffix_ci<'t>(text: &'t str, suffix: &str) -> Option<&'t str> {
    let at = text.len().checked_sub(suffix.len())?;
    let (head, tail) = (text.get(..at)?, text.get(at..)?);
    tail.eq_ignore_ascii_case(suffix).then_some(head)
}

/// Whether `text` is a hexadecimal `internalidstring`: longer than two
/// characters and starting with `0x` in any case (`maps/RomID.java:66-67`).
/// A bare `0x` is text.
pub(super) fn is_hex_id(text: &str) -> bool {
    text.len() > 2 && text.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("0x"))
}

/// The bytes of a hexadecimal `internalidstring`, as legacy's
/// `HexUtil.asBytes` read them (`util/HexUtil.java:51-60`, `:120-145`):
/// every space and every `0x` (any case) removed, then hex digit pairs, an
/// odd digit count padded with a leading `0`. `None` for a character that is
/// not a hex digit, or no digit at all (legacy then compared zero bytes and
/// matched every image).
pub(super) fn parse_hex_bytes(text: &str) -> Option<Vec<u8>> {
    let mut digits: Vec<u8> = Vec::with_capacity(text.len());
    let bytes: Vec<u8> = text.bytes().filter(|&b| b != b' ').collect();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'0' && bytes.get(i + 1).is_some_and(|&x| x == b'x' || x == b'X') {
            i += 2;
            continue;
        }
        digits.push(hex_digit(b)?);
        i += 1;
    }
    if digits.is_empty() {
        return None;
    }
    if digits.len() % 2 == 1 {
        digits.insert(0, 0);
    }
    Some(digits.chunks_exact(2).map(|p| (p[0] << 4) | p[1]).collect())
}

/// The value of one hex digit.
const fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// A boolean: `true` or `false`, any ASCII case. Legacy's
/// `Boolean.parseBoolean` (`xml/DOMRomUnmarshaller.java:284`) read every
/// other value as `false`; the caller does too, with a warning.
pub(super) fn parse_bool(text: &str) -> Option<bool> {
    let t = text.trim_matches(is_xml_space);
    if t.eq_ignore_ascii_case("true") {
        Some(true)
    } else if t.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// A decimal integer as legacy's `Integer.parseInt` read the numeric table
/// attributes (`sizex`, `sizey`, `userlevel`, `skipCells`,
/// `xml/DOMHelper.java:57-59`; a `<bit position>`,
/// `maps/TableBitwiseSwitch.java:72`): an optional sign, then one or more
/// ASCII digits, within a signed 32-bit `int`.
pub(super) fn parse_decimal_int(text: &str) -> Option<i32> {
    let t = text.trim_matches(is_xml_space);
    let (negative, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value.checked_mul(10)?.checked_add(i64::from(b - b'0'))?;
    }
    i32::try_from(if negative { -value } else { value }).ok()
}

/// The cell values of a switch `<state data="...">`, as legacy's
/// `PresetManager.createPresetEntryValue` read them
/// (`maps/PresetManager.java:94-110`): the trimmed text split at commas if it
/// has any, else at spaces; each non-empty piece hexadecimal
/// (`ByteUtil.parseUnsignedInt(s, 16)`, `util/ByteUtil.java:167-193`: an
/// optional `+`, then hex digits, at most `FFFFFFFF`). `None` if a piece is
/// anything else.
///
/// Pieces may also be separated by tabs and line breaks, and have
/// whitespace around them, which legacy refused.
pub(super) fn parse_state_data(text: &str) -> Option<Vec<u32>> {
    let t = text.trim_matches(is_xml_space);
    let pieces: Vec<&str> = if t.contains(',') {
        t.split(',').map(|p| p.trim_matches(is_xml_space)).collect()
    } else {
        t.split(is_xml_space).collect()
    };
    pieces
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(|p| {
            let digits = p.strip_prefix('+').unwrap_or(p);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let mut value: u64 = 0;
            for c in digits.chars() {
                value = value * 16 + u64::from(c.to_digit(16)?);
                if value > MAX_HEX {
                    return None;
                }
            }
            u32::try_from(value).ok()
        })
        .collect()
}

/// A decimal number as legacy's `Double.parseDouble` read a scaling's
/// `min`, `max` and increments (`xml/TableScaleUnmarshaller.java:451-458`):
/// surrounding whitespace and control characters trimmed, an optional sign,
/// digits with an optional `.` (at least one digit on either side), an
/// optional exponent (`e` or `E`, an optional sign, digits) and an optional
/// `f`, `F`, `d` or `D` suffix; or `NaN`, or `Infinity` with an optional
/// sign, exactly so spelt. `None` for anything else.
///
/// Legacy also accepted Java's hexadecimal floating-point literals
/// (`0x1p3`); no definition writes one, and they are refused here.
pub(super) fn parse_java_double(text: &str) -> Option<f64> {
    let t = text.trim_matches(|c: char| c <= ' ');
    let (sign, body) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    if body == "Infinity" {
        return Some(sign * f64::INFINITY);
    }
    if body == "NaN" {
        return Some(f64::NAN);
    }
    let body = body.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(body);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (body.get(..at)?, Some(body.get(at + 1..)?)),
        None => (body, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || !digits(frac) || int.len() + frac.len() == 0 {
        return None;
    }
    if let Some(e) = exponent {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        if e.is_empty() || !digits(e) {
            return None;
        }
    }
    // The grammar checked above is a subset of what `f64::from_str`
    // accepts, so this parse cannot fail; it rounds to nearest, as Java's.
    body.parse::<f64>().ok().map(|v| sign * v)
}

/// XML's whitespace: space, tab, CR, LF.
pub(super) const fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}
