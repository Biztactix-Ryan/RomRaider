//! `<romid>`: what a ROM definition says about the ROM it describes, and how
//! an image is recognised as that ROM.

use rr_core::diag::{Span, codes};
use rr_core::encoding::Endian;
use rr_core::encoding::legacy::parse_endian;
use rr_core::layer::Field;
use rr_core::pattern::{BytePattern, Case};

use super::ctx::{Ctx, is};
use super::value::{is_hex_id, parse_bool, parse_file_size, parse_hex_bytes, parse_hex_int};

/// The `<romid>` of one `<rom>`, as a partial document (C1): each field is
/// [`Field::Unset`] unless the element names it, and a child definition's
/// fields replace its base's one by one, as legacy's single `RomID` object
/// was overwritten by each `<romid>` along the `base` chain
/// (`xml/DOMRomUnmarshaller.java:149-166`, `:216-291`).
///
/// Text fields hold the element's text exactly as written, untrimmed, as
/// legacy read it: the pinned corpus has `internalidstring`s ending in
/// spaces (`A2WA20  `) that the image bytes really hold.
///
/// | Element | Field | Legacy (`maps/RomID.java`) |
/// |---------|-------|----------------------------|
/// | `xmlid` | [`xmlid`](Self::xmlid) | the definition's name; `base` refers to it |
/// | `internalidaddress` | [`internal_id_address`](Self::internal_id_address) | hex, `HexUtil.hexToInt` |
/// | `internalidstring` | [`internal_id_string`](Self::internal_id_string) | ID text, or `0x`-hex bytes |
/// | `internalidhex` | [`internal_id_hex`](Self::internal_id_hex) | ECUFlash's hex form of the ID; legacy ignored it |
/// | `author`, `version`, `caseid`, `ecuid`, `make`, `market`, `model`, `submodel`, `transmission`, `year`, `flashmethod` | text fields of the same names | display only |
/// | `memmodel` | [`mem_model`](Self::mem_model), its `endian` attribute [`mem_model_endian`](Self::mem_model_endian) | `endian` overrode every table's byte order (`xml/TableScaleUnmarshaller.java:162-169`) |
/// | `filesize` | [`file_size`](Self::file_size) (bytes) | `RomAttributeParser.parseFileSize` |
/// | `noramoffset`, `offset` | [`ram_offset`](Self::ram_offset) | `disableRamOffset` / `setOffset` |
/// | `obsolete` | [`obsolete`](Self::obsolete) | `Boolean.parseBoolean` |
/// | `checksummodule` | [`checksum_module`](Self::checksum_module) | ECUFlash's checksum module name; legacy ignored it |
///
/// `internalidhex` and `checksummodule` are ECUFlash elements found in the
/// pinned corpus; they are kept as written so a later reader can use them,
/// and are not part of identification ([`Identification`] follows legacy,
/// which ignored them). Any other child element is reported
/// (`DEF0004`) and ignored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RomIdDoc {
    /// `<xmlid>`: the definition's name.
    pub xmlid: Field<String>,
    /// `<internalidaddress>`: where the ID is in the image.
    pub internal_id_address: Field<IdAddress>,
    /// `<internalidstring>`: the ID, as text or as `0x`-hex bytes.
    pub internal_id_string: Field<String>,
    /// `<internalidhex>` (ECUFlash), as written.
    pub internal_id_hex: Field<String>,
    /// `<author>`.
    pub author: Field<String>,
    /// `<version>`: the definition's version.
    pub version: Field<String>,
    /// `<caseid>`: the ECU hardware part number.
    pub case_id: Field<String>,
    /// `<ecuid>`: the ECU ID the ECU reports.
    pub ecu_id: Field<String>,
    /// `<make>`.
    pub make: Field<String>,
    /// `<market>`.
    pub market: Field<String>,
    /// `<model>`.
    pub model: Field<String>,
    /// `<submodel>`.
    pub submodel: Field<String>,
    /// `<transmission>`.
    pub transmission: Field<String>,
    /// `<year>`.
    pub year: Field<String>,
    /// `<flashmethod>`: the reflash method name.
    pub flash_method: Field<String>,
    /// `<memmodel>`: the ECU processor / memory model name.
    pub mem_model: Field<String>,
    /// `<memmodel endian="...">`: the byte order that overrides every
    /// table's own. Absent when the attribute is.
    ///
    /// Legacy set it from each `<memmodel>` it read, and reset it when a
    /// `<memmodel>` had no `endian`, for the tables read after that element
    /// (`xml/DOMRomUnmarshaller.java:276-278`); here a `<memmodel>` without
    /// it leaves the inherited value. No pinned definition has the
    /// attribute.
    pub mem_model_endian: Field<Endian>,
    /// `<filesize>`, in bytes.
    pub file_size: Field<u32>,
    /// `<noramoffset>` or `<offset>`: how table addresses map to image
    /// offsets. Unset means legacy's default, `filesize` minus the image
    /// length.
    pub ram_offset: Field<RamOffset>,
    /// `<obsolete>`: a newer revision of the ROM exists.
    ///
    /// Not inherited: legacy reset it to `false` after reading a base
    /// (`xml/DOMRomUnmarshaller.java:153`, `:209`), so the reader sets it to
    /// `false` on every `<rom>` with a `base` that does not say otherwise.
    pub obsolete: Field<bool>,
    /// `<checksummodule>` (ECUFlash), as written.
    pub checksum_module: Field<String>,
}

rr_core::merge_fields!(RomIdDoc {
    xmlid,
    internal_id_address,
    internal_id_string,
    internal_id_hex,
    author,
    version,
    case_id,
    ecu_id,
    make,
    market,
    model,
    submodel,
    transmission,
    year,
    flash_method,
    mem_model,
    mem_model_endian,
    file_size,
    ram_offset,
    obsolete,
    checksum_module,
});

/// An `<internalidaddress>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdAddress {
    /// The ID starts at this offset of the image.
    At(u32),
    /// `-1`: legacy's marker that, with the `internalidstring` `force`
    /// (any case), the definition matches every image
    /// (`maps/RomID.java:62-63`). With any other string it matches none.
    MinusOne,
}

/// How a definition's table addresses map to offsets in the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RamOffset {
    /// `<noramoffset/>`: addresses are image offsets
    /// (`RomID.disableRamOffset`).
    Disabled,
    /// `<offset>N</offset>`: the image offset of an address is the address
    /// plus `N` (legacy stored `-N`, `RomID.setOffset`, and subtracted it).
    Offset(i64),
}

/// How an image is recognised as a definition's ROM: legacy's
/// `RomID.checkMatch` (`maps/RomID.java:58-93`), compiled once into a
/// [`BytePattern`] (C2).
///
/// It comes from a `<rom>`'s own first `<romid>` only, never an inherited
/// one, as legacy matched it (`xml/DOMRomUnmarshaller.java:114-133`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Identification {
    /// `internalidaddress` `-1` with `internalidstring` `force`: matches
    /// every image.
    Force,
    /// The ID bytes at the ID address. A text ID compares ASCII letters in
    /// either case, as legacy's `equalsIgnoreCase`; a `0x`-hex ID compares
    /// bytes exactly.
    Bytes(BytePattern),
}

impl Identification {
    /// Whether `image` is this definition's ROM. An image too short to hold
    /// the ID does not match.
    #[must_use]
    pub fn matches(&self, image: &[u8]) -> bool {
        match self {
            Self::Force => true,
            Self::Bytes(p) => p.matches(image),
        }
    }
}

/// What reading one `<romid>` found.
pub(super) struct ReadRomId {
    pub(super) doc: RomIdDoc,
    pub(super) identification: Option<Identification>,
}

/// The `<romid>` children, by lower-cased name.
const KNOWN: &[&str] = &[
    "xmlid",
    "internalidaddress",
    "internalidstring",
    "internalidhex",
    "author",
    "version",
    "caseid",
    "ecuid",
    "make",
    "market",
    "model",
    "submodel",
    "transmission",
    "year",
    "flashmethod",
    "memmodel",
    "filesize",
    "noramoffset",
    "offset",
    "obsolete",
    "checksummodule",
];

/// Reads one `<romid>` element.
pub(super) fn read_romid(ctx: &mut Ctx<'_>, el: &crate::xml::Element) -> ReadRomId {
    ctx.check_attributes(el, &[]);
    ctx.no_text(el);
    let mut doc = RomIdDoc::default();
    let mut seen: Vec<&'static str> = Vec::new();
    // Where the ID string is, for errors about it; and whether the address
    // was refused (already an error, so identification is not attempted).
    let mut id_span = el.span();
    let mut address_refused = false;
    for child in el.elements() {
        let Some(&name) = KNOWN.iter().find(|k| is(child, k)) else {
            ctx.unknown_element(child, el.name());
            continue;
        };
        if seen.contains(&name) {
            ctx.duplicate_element(child, el.name());
        } else {
            seen.push(name);
        }
        let attrs: &[&str] = if name == "memmodel" { &["endian"] } else { &[] };
        let (text, span) = ctx.value_text(child, attrs);
        let text_field = |text: &str| Field::Set(text.to_owned());
        match name {
            "xmlid" => doc.xmlid = text_field(&text),
            "internalidaddress" => match address(&text) {
                Some(a) => {
                    doc.internal_id_address = Field::Set(a);
                    address_refused = false;
                }
                None => {
                    ctx.error(
                        codes::INVALID_ADDRESS,
                        format!(
                            "internalidaddress {:?} is not a hexadecimal address (0 to FFFFFFFF, or -1)",
                            &*text
                        ),
                        span,
                    );
                    address_refused = true;
                }
            },
            "internalidstring" => {
                doc.internal_id_string = text_field(&text);
                id_span = span;
            }
            "internalidhex" => doc.internal_id_hex = text_field(&text),
            "author" => doc.author = text_field(&text),
            "version" => doc.version = text_field(&text),
            "caseid" => doc.case_id = text_field(&text),
            "ecuid" => doc.ecu_id = text_field(&text),
            "make" => doc.make = text_field(&text),
            "market" => doc.market = text_field(&text),
            "model" => doc.model = text_field(&text),
            "submodel" => doc.submodel = text_field(&text),
            "transmission" => doc.transmission = text_field(&text),
            "year" => doc.year = text_field(&text),
            "flashmethod" => doc.flash_method = text_field(&text),
            "checksummodule" => doc.checksum_module = text_field(&text),
            "memmodel" => {
                doc.mem_model = text_field(&text);
                if let Some(attr) = child.attribute("endian") {
                    match parse_endian(attr.value()) {
                        Ok(endian) => doc.mem_model_endian = Field::Set(endian),
                        Err(e) => ctx
                            .diags
                            .push(e.to_diagnostic(ctx.provenance(attr.value_span()))),
                    }
                }
            }
            "filesize" => match parse_file_size(&text) {
                Some(size) => doc.file_size = Field::Set(size),
                None => ctx.error(
                    codes::INVALID_FILE_SIZE,
                    format!(
                        "filesize {:?} is not a size: decimal digits with an optional kb, mb or b suffix, under 4 GiB",
                        &*text
                    ),
                    span,
                ),
            },
            "noramoffset" => {
                // Legacy disabled the offset whatever the element held.
                ctx.no_text(child);
                doc.ram_offset = Field::Set(RamOffset::Disabled);
            }
            "offset" => match parse_hex_int(&text) {
                Some(offset) => doc.ram_offset = Field::Set(RamOffset::Offset(offset)),
                None => ctx.error(
                    codes::INVALID_ADDRESS,
                    format!(
                        "offset {:?} is not a hexadecimal offset (-FFFFFFFF to FFFFFFFF)",
                        &*text
                    ),
                    span,
                ),
            },
            "obsolete" => {
                let value = parse_bool(&text).unwrap_or_else(|| {
                    ctx.warning(
                        codes::INVALID_BOOLEAN,
                        format!("obsolete {:?} is neither true nor false; read as false", &*text),
                        span,
                    );
                    false
                });
                doc.obsolete = Field::Set(value);
            }
            // Every name in KNOWN has an arm above.
            _ => ctx.unknown_element(child, el.name()),
        }
    }
    let identification = if address_refused {
        None
    } else {
        identify(ctx, el.span(), &doc, id_span)
    };
    ReadRomId {
        doc,
        identification,
    }
}

/// An `internalidaddress`: a hex address, or `-1`.
fn address(text: &str) -> Option<IdAddress> {
    match parse_hex_int(text)? {
        -1 => Some(IdAddress::MinusOne),
        a => u32::try_from(a).ok().map(IdAddress::At),
    }
}

/// Compiles the identification of a `<romid>` at `romid` whose own fields
/// are `doc`; `id_span` is where its `internalidstring` text is.
fn identify(
    ctx: &mut Ctx<'_>,
    romid: Span,
    doc: &RomIdDoc,
    id_span: Span,
) -> Option<Identification> {
    let address = doc.internal_id_address.get().copied();
    let id = doc
        .internal_id_string
        .get()
        .map(String::as_str)
        .filter(|s| !s.is_empty());
    let Some(id) = id else {
        // Legacy: an empty or absent string never matches (RomID.java:60).
        if address.is_some() {
            ctx.warning(
                codes::ROM_ID_INCOMPLETE,
                "<romid> has an internalidaddress but no internalidstring; it never matches an image"
                    .to_owned(),
                romid,
            );
        }
        return None;
    };
    let at = match address {
        Some(IdAddress::At(a)) => a,
        Some(IdAddress::MinusOne) if id.eq_ignore_ascii_case("force") => {
            return Some(Identification::Force);
        }
        Some(IdAddress::MinusOne) => {
            ctx.warning(
                codes::ROM_ID_INCOMPLETE,
                format!(
                    "<romid> has internalidaddress -1 with internalidstring {id:?}; only \"force\" \
                     goes with -1, so it never matches an image"
                ),
                romid,
            );
            return None;
        }
        None => {
            ctx.warning(
                codes::ROM_ID_INCOMPLETE,
                "<romid> has an internalidstring but no internalidaddress; matched at address 0, \
                 as legacy RomRaider did"
                    .to_owned(),
                romid,
            );
            0
        }
    };
    let pattern = if is_hex_id(id) {
        let Some(bytes) = parse_hex_bytes(id) else {
            ctx.error(
                codes::INVALID_ROM_ID,
                format!("internalidstring {id:?} starts with 0x but is not hexadecimal bytes"),
                id_span,
            );
            return None;
        };
        usize::try_from(at)
            .ok()
            .and_then(|at| BytePattern::exact(at, &bytes).ok())
    } else {
        usize::try_from(at)
            .ok()
            .and_then(|at| BytePattern::text(at, id, Case::AsciiInsensitive).ok())
    };
    match pattern {
        Some(p) => Some(Identification::Bytes(p)),
        None => {
            ctx.error(
                codes::INVALID_ROM_ID,
                format!("internalidstring {id:?} at {at:#X} would end past the largest address"),
                id_span,
            );
            None
        }
    }
}
