//! The registry of stable diagnostic codes (US-RR-17-6).
//!
//! Every [`Code`] the workspace emits is defined here, once, by the
//! `registry!` invocation below. Each entry is a public constant (so code
//! uses `codes::UNKNOWN_SCALING`, never a string literal) and a row of
//! [`REGISTRY`] carrying its name, severity, the version it first appeared in
//! and its documentation — the `///` comment on the entry, which is both the
//! rustdoc of the constant and the text of `docs/diagnostics.md`.
//!
//! # Rules
//!
//! * A code is three upper-case letters naming its [`Domain`] followed by
//!   four digits: `DEF0001`.
//! * A code is never reused. Once published it keeps its meaning forever; to
//!   retire one, move it from the registry to [`DEPRECATED`] with the version
//!   and reason (and the replacement, if any). A deprecated code is never
//!   handed out again.
//! * The severity listed is the one the code is always reported with.
//!
//! [`problems`] checks these rules (unique codes and names, well-formed codes,
//! known domains, non-empty docs, no deprecated code back in use); the unit
//! tests run it on the real tables.
//!
//! Stability is checked against `PUBLISHED`, an append-only ledger of every
//! code ever published, kept in this file's tests (US-RR-17-1). A test fails
//! when a ledger code is neither active nor in [`DEPRECATED`] (removed without
//! a deprecation entry), when a code's name differs from the ledger (renumbered
//! or reused for another meaning), or when a registered code is missing from
//! the ledger. Adding a code means adding its ledger row too; ledger rows are
//! never edited or deleted.
//!
//! # Documentation
//!
//! `docs/diagnostics.md` is [`render_markdown`]'s output, committed.
//! Regenerate it with `cargo xtask diagnostics-doc` after changing this file;
//! `cargo xtask diagnostics-doc --check` and the `xtask/tests/diagnostics_doc.rs`
//! test (run by CI's nextest job) fail when the committed file has drifted.

use core::fmt;
use core::fmt::Write as _;

use super::{Code, Severity};

/// A code family: the three-letter prefix of a [`Code`] and what it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    /// The prefix, e.g. `DEF`.
    pub prefix: &'static str,
    /// Heading used in the docs, e.g. `Definitions`.
    pub title: &'static str,
    /// One sentence on what the domain covers.
    pub description: &'static str,
}

/// Every code domain, in documentation order.
pub const DOMAINS: &[Domain] = &[
    Domain {
        prefix: "DEF",
        title: "Definitions",
        description: "Reading ROM, logger and device definitions (rr-defs): \
                      malformed or inconsistent definition content.",
    },
    Domain {
        prefix: "ENC",
        title: "Encoding",
        description: "Storage types, endianness and bit layouts (C3, \
                      `rr-core::encoding`): values a definition asks for that \
                      cannot be encoded or decoded.",
    },
    Domain {
        prefix: "EXP",
        title: "Expressions",
        description: "Formulas in scalings, logger conversions and derived \
                      parameters (C4, `rr-core::expr`): text that does not \
                      parse.",
    },
    Domain {
        prefix: "CNV",
        title: "Conversions",
        description: "Scalings and conversions between raw and real values \
                      (C4, `rr-core::conversion`): formulas, lookup tables \
                      and linear forms that cannot convert.",
    },
    Domain {
        prefix: "UNT",
        title: "Units",
        description: "The units registry (C5, `rr-core::units`): unit \
                      strings of legacy definitions that name no known unit.",
    },
    Domain {
        prefix: "LAY",
        title: "Layering",
        description: "Layered documents and the resolver (C1, \
                      `rr-core::layer`): `extends`/`base` chains, overrides \
                      and omits.",
    },
    Domain {
        prefix: "PAT",
        title: "Patterns",
        description: "Byte patterns for ROM identification, frame sync \
                      headers and response validation (C2, \
                      `rr-core::pattern`): pattern text that does not parse.",
    },
    Domain {
        prefix: "XML",
        title: "XML",
        description: "The hardened XML reader every XML definition format \
                      goes through (`rr-defs::xml`, SECURITY.md API Security): \
                      files that are not well-formed, use DTD entities, or \
                      exceed the reader's limits.",
    },
];

/// One registered, active diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeInfo {
    /// The code.
    pub code: Code,
    /// The constant's name, e.g. `UNKNOWN_SCALING`.
    pub name: &'static str,
    /// The severity the code is always reported with.
    pub severity: Severity,
    /// Workspace version the code first appeared in.
    pub since: &'static str,
    /// The entry's doc comment, one element per line, leading space removed
    /// by [`CodeInfo::doc_lines`].
    pub doc: &'static [&'static str],
}

impl CodeInfo {
    /// The doc comment's lines with the single leading space that `///`
    /// leaves removed.
    pub fn doc_lines(&self) -> impl Iterator<Item = &'static str> {
        self.doc.iter().map(|l| l.strip_prefix(' ').unwrap_or(l))
    }

    /// The first line of the doc comment: a one-sentence summary.
    #[must_use]
    pub fn summary(&self) -> &'static str {
        self.doc_lines().next().unwrap_or("").trim()
    }
}

/// A retired code. It stays listed forever so it is never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeprecatedCode {
    /// The retired code.
    pub code: Code,
    /// The name it had.
    pub name: &'static str,
    /// Workspace version in which it stopped being emitted.
    pub deprecated_in: &'static str,
    /// Why it was retired.
    pub reason: &'static str,
    /// The active code that replaces it, if any.
    pub replaced_by: Option<Code>,
}

/// Defines the code constants and [`REGISTRY`] from one list, so a code
/// cannot exist without its registry row and documentation.
macro_rules! registry {
    (
        $(
            $(#[doc = $doc:literal])+
            $name:ident = $code:literal, $severity:ident, since $since:literal;
        )*
    ) => {
        $(
            $(#[doc = $doc])+
            #[doc = ""]
            #[doc = concat!("Code `", $code, "`, severity `", stringify!($severity), "`.")]
            pub const $name: Code = Code::new($code);
        )*

        /// Every active diagnostic code, with its documentation.
        pub const REGISTRY: &[CodeInfo] = &[
            $(
                CodeInfo {
                    code: $name,
                    name: stringify!($name),
                    severity: Severity::$severity,
                    since: $since,
                    doc: &[$($doc),+],
                },
            )*
        ];
    };
}

registry! {
    /// A table or parameter references a scaling that no layer defines.
    ///
    /// The definition names a scaling by id but the resolved definition has
    /// no scaling with that id. The table is not loaded: decoding it with a
    /// guessed scaling would show wrong values.
    UNKNOWN_SCALING = "DEF0001", Error, since "0.1.0";

    /// A file read as a RomRaider ECU definition is some other kind of XML.
    ///
    /// Its root element is neither `<roms>` nor `<rom>` (for example a
    /// logger definition's `<logger>`). Nothing in it is read as a ROM
    /// definition; legacy RomRaider found no ROM in such a file.
    NOT_ROM_DEFINITION = "DEF0002", Error, since "0.1.0";

    /// An ECU definition file's root element is a `<rom>`, not `<roms>`.
    ///
    /// The file is read as a file holding that one ROM. Legacy RomRaider
    /// looked for `<rom>` children of the root and so found no ROM in such a
    /// file; wrap the `<rom>` in `<roms>` for a file both read alike.
    SINGLE_ROM_ROOT = "DEF0003", Warning, since "0.1.0";

    /// A definition has an element that is not part of its format.
    ///
    /// The element (a misspelt `<xmlid>`, or another tool's element such as
    /// ECUFlash's `<include>`) is not read; the diagnostic names it and the
    /// element it is in. Legacy RomRaider skipped such elements without a
    /// word, so a misspelt element silently lost its value.
    UNKNOWN_ELEMENT = "DEF0004", Warning, since "0.1.0";

    /// A definition element has an attribute that is not part of its format.
    ///
    /// The attribute is not read; the diagnostic names it and its element.
    /// Attribute names are case-sensitive, as legacy RomRaider read them: a
    /// `BASE` attribute on `<rom>` is not `base`, and legacy ignored it
    /// silently.
    UNKNOWN_ATTRIBUTE = "DEF0005", Warning, since "0.1.0";

    /// A definition has an element legacy RomRaider read but this reader does not yet.
    ///
    /// The element (for example a `<rom>`'s `<checksum>` or `<Simulation>`)
    /// is valid in the format but not supported yet, and is ignored. A
    /// definition that relies on it (a checksum to fix on save) is not
    /// complete.
    UNSUPPORTED_ELEMENT = "DEF0006", Warning, since "0.1.0";

    /// A definition element holds text where only child elements belong.
    ///
    /// For example text directly inside `<rom>` or `<romid>`, or inside an
    /// element that is a flag (`<noramoffset>`). The text is ignored, as
    /// legacy RomRaider ignored it.
    UNEXPECTED_TEXT = "DEF0007", Warning, since "0.1.0";

    /// An element that says one thing appears more than once.
    ///
    /// For example two `<xmlid>` in one `<romid>`, or two `<romid>` in one
    /// `<rom>`. The later one wins, as in legacy RomRaider; the diagnostic
    /// points at it. (A ROM is identified by its first `<romid>` only, as
    /// legacy did.)
    DUPLICATE_ELEMENT = "DEF0008", Warning, since "0.1.0";

    /// A ROM definition has no `<romid>` with a non-empty `<xmlid>`.
    ///
    /// The `xmlid` names the ROM: other definitions name it as their `base`,
    /// and it identifies the definition in every message. A `<rom>` without
    /// one is not read.
    MISSING_XMLID = "DEF0009", Error, since "0.1.0";

    /// An address or offset is not a hexadecimal number in range.
    ///
    /// Addresses (`internalidaddress`) and offsets (`<offset>`) are
    /// hexadecimal, with or without a `0x` prefix (`2000` is `0x2000`), at
    /// most `FFFFFFFF`; an `internalidaddress` may also be `-1`, legacy's
    /// marker for an `internalidstring` of `force`. The value is not read:
    /// legacy RomRaider failed to load the whole file on such a value.
    INVALID_ADDRESS = "DEF0010", Error, since "0.1.0";

    /// A `<filesize>` is not a size legacy RomRaider understood.
    ///
    /// A file size is decimal digits with an optional `kb` or `mb` (times
    /// 1024 or 1024²) or `b` suffix, in any case (`1024kb`, `512Kb`,
    /// `196608`), at most 4 GiB less one byte. The value is not read: legacy
    /// RomRaider failed to load the whole file on such a value.
    INVALID_FILE_SIZE = "DEF0011", Error, since "0.1.0";

    /// A ROM's hexadecimal identification string is not hexadecimal bytes.
    ///
    /// An `internalidstring` starting with `0x` gives the ID as bytes (`0x`
    /// prefixes and spaces are ignored, an odd digit count is padded with a
    /// leading `0`, as legacy RomRaider did), but this one holds a character
    /// that is not a hex digit, or no digit at all. The ROM cannot be
    /// identified from an image.
    INVALID_ROM_ID = "DEF0012", Error, since "0.1.0";

    /// A ROM's identification has only one of its two halves.
    ///
    /// An `internalidstring` without an `internalidaddress` is matched at
    /// address 0, as legacy RomRaider did. An `internalidaddress` without
    /// an `internalidstring`, or the address `-1` with a string other than
    /// `force`, never matches an image, as in legacy. Either way the
    /// definition is probably missing an element.
    ROM_ID_INCOMPLETE = "DEF0013", Warning, since "0.1.0";

    /// A yes/no definition value is neither `true` nor `false`.
    ///
    /// The value (for example `<obsolete>yes</obsolete>`) is read as
    /// `false`, as legacy RomRaider's `Boolean.parseBoolean` read every
    /// value but `true` (in any case).
    INVALID_BOOLEAN = "DEF0014", Warning, since "0.1.0";

    /// A definition `<table>` has no `name`.
    ///
    /// A table is known by its name: a child definition changes or omits an
    /// inherited table by naming it. A `<table>` without a `name` (or with an
    /// empty one) is not read; legacy RomRaider dropped it without a word.
    MISSING_TABLE_NAME = "DEF0015", Error, since "0.1.0";

    /// A definition `<table>` has an unknown `type`, or none where one is needed.
    ///
    /// A table's `type` is `1D`, `2D`, `3D`, `Switch` or `BitwiseSwitch` (any
    /// case; `X Axis`, `Y Axis`, `Static X Axis` and `Static Y Axis` read as
    /// `1D`, as in legacy RomRaider). A table that is not a change to one
    /// inherited from a `base` (or to one earlier in the same `<rom>`) must
    /// say its type. Legacy RomRaider logged such a table and left it out.
    INVALID_TABLE_TYPE = "DEF0016", Error, since "0.1.0";

    /// A definition number is not a decimal integer in range.
    ///
    /// Table sizes (`sizex`, `sizey`) are whole numbers from 1, `skipCells`
    /// from 0, `userlevel` an integer, and a `<bit>`'s `position` 0 to 31,
    /// all in decimal digits with an optional sign. The value is not read:
    /// legacy RomRaider failed to load the whole file on such a value.
    INVALID_NUMBER = "DEF0017", Error, since "0.1.0";

    /// A table's `userlevel` is outside 1 to 5.
    ///
    /// User levels run from 1 (beginner) to 5 (debug). The value is clamped
    /// to that range, as legacy RomRaider meant to do (its clamp had no
    /// effect, so a level of 7 hid the table at every setting).
    USER_LEVEL_OUT_OF_RANGE = "DEF0018", Warning, since "0.1.0";

    /// A table is missing an axis its type needs.
    ///
    /// A `2D` table has one axis, and a `3D` table an `X Axis` and a `Y Axis`
    /// (child `<table>` elements). In a `<rom>` without a `base` nothing can
    /// supply a missing one, so the table cannot be shown; legacy RomRaider
    /// read it against an empty axis.
    MISSING_AXIS = "DEF0019", Error, since "0.1.0";

    /// A definition element is valid but has no effect where it is.
    ///
    /// For example an axis on a `1D` or `Switch` table, a child `<table>`
    /// whose `type` is not an axis type, static `<data>` on a `2D` or `3D`
    /// table, or both an `X Axis` and a `Y Axis` on a `2D` table (which has
    /// one axis: both are read into it, the later winning, as in legacy
    /// RomRaider). Legacy RomRaider ignored these without a word.
    CONTENT_IGNORED = "DEF0020", Warning, since "0.1.0";

    /// A switch table's `<state data>` is not hexadecimal values.
    ///
    /// A state's `data` is the table's cell values in hexadecimal, separated
    /// by spaces or commas (`01 00 FF`), each at most `FFFFFFFF`. The state
    /// is not read: legacy RomRaider failed to load the whole file on such a
    /// value.
    INVALID_STATE_DATA = "DEF0021", Error, since "0.1.0";

    /// A static axis has a different number of values than the axis has cells.
    ///
    /// A static axis lists its values as `<data>` elements; their count
    /// should be the axis size (its own `sizey`/`sizex`, which legacy
    /// RomRaider replaced with the table's size along that axis). With too
    /// few, legacy showed empty headers; with too many, it dropped the rest.
    STATIC_AXIS_SIZE = "DEF0022", Warning, since "0.1.0";

    /// A scaling's `min`, `max` or increment is not a decimal number.
    ///
    /// A RomRaider `<scaling>` (or `<scalingbase>`) gives `min`, `max`,
    /// `increment` / `coarseincrement` and `fineincrement` as decimal
    /// numbers (`.01`, `-40`, `1e3`). The value is not read: legacy
    /// RomRaider failed to load the whole file on such a value.
    INVALID_SCALING_NUMBER = "DEF0023", Error, since "0.1.0";

    /// A table or axis read from the image has no storage type.
    ///
    /// Once its definition and every `base` it inherits from are merged, a
    /// table (or a non-static axis) whose cells are read from the ROM image
    /// has no `storagetype`, so its cells cannot be decoded. (A `Switch` or
    /// `BitwiseSwitch` table without one is `uint8`, as in legacy
    /// RomRaider.) Legacy read every cell of such a table as 0
    /// (`RomAttributeParser.parseByteValue` with a length of 0); here it is
    /// not loaded (MF-02).
    MISSING_STORAGE_TYPE = "DEF0024", Error, since "0.1.0";

    /// A scaling's `to_byte` formula cannot be used; it is ignored.
    ///
    /// The real-to-raw formula of a RomRaider `<scaling>` does not parse (an
    /// unbalanced parenthesis, say) or names something other than `x`; the
    /// message gives the expression error and its code. Nothing is lost:
    /// values typed into a cell are always stored with the inverse of the
    /// scaling's `expression`, and a definition's `to_byte` is only checked
    /// against it (`CNV0005`), never used. Legacy RomRaider loaded such a
    /// definition, and its JEP evaluated the formula as NaN when a value was
    /// written.
    UNUSABLE_TO_BYTE = "DEF0025", Warning, since "0.1.0";

    /// Two ROM definitions in the loaded definition files have the same `xmlid`.
    ///
    /// The `xmlid` names a ROM definition, compared ignoring case as legacy
    /// RomRaider compared it. Within one file the first `<rom>` of the name
    /// is used and the later one is ignored, as legacy found the first. In
    /// different files both are kept: a `base` is looked up in its own file
    /// first, as legacy looked it up there only, so each file's copy of a
    /// shared base (RomRaider's per-ROM files each carry their own
    /// `32BITBASE`) serves that file; a reference from a file that has none,
    /// and a lookup of the name from outside the files, gets the one in the
    /// first file loaded, as legacy took the first file that matched. The
    /// message names where both are.
    DUPLICATE_XMLID = "DEF0026", Warning, since "0.1.0";

    /// A table placed on a ROM has no type in any definition of its `base` chain; it is not loaded.
    ///
    /// The `<table>` gives the table an address but no `type`, and no
    /// definition the ROM inherits from has a table of that name to take
    /// one from: usually a child definition changing a table its base does
    /// not define (a misspelt name, or a table missing from that copy of
    /// the base). Its cells cannot be laid out, so the table is left out
    /// and the rest of the ROM loads, as in legacy RomRaider, which logged
    /// "Table type unspecified" and skipped it. (A table of a `<rom>` with
    /// no `base` that has no type is `DEF0016` when the file is read.)
    UNTYPED_TABLE = "DEF0027", Warning, since "0.1.0";

    /// A ROM image matches more than one ROM definition; none is applied.
    ///
    /// The ROM ID bytes of the image (`internalidaddress` +
    /// `internalidstring`) match the identification of two or more `<rom>`s
    /// of the loaded definition files: two files defining the same ROM (a
    /// standard and a metric copy, say), two ROM IDs one of which is a
    /// prefix of the other at one address, or a `force` definition, which
    /// matches every image. The message names every candidate and where it
    /// is defined. No definition is picked, so the image is never shown or
    /// edited through the wrong one (EC-03); legacy RomRaider silently used
    /// the first match. Load only the definition file meant for the ROM, or
    /// fix the overlapping IDs.
    AMBIGUOUS_ROM_ID = "DEF0028", Error, since "0.1.0";

    /// A ROM image is not the size its definition declares; it opens read-only.
    ///
    /// The image is longer or shorter than the `<filesize>` of the ROM
    /// definition it was opened with (inherited from a `base` or its own).
    /// The message names the definition and both sizes, in bytes. Every
    /// table address of a definition is laid out for an image of that size,
    /// so the image is probably not this ROM, or not a whole one (a partial
    /// read, a dump with a header or trailing bytes): an edit made through
    /// the definition could change the wrong bytes. The ROM opens for
    /// viewing with editing disabled (EC-02); legacy RomRaider never
    /// compared the sizes and silently shifted every table address by the
    /// difference. Open the full, raw image of the ROM, or the definition
    /// meant for it. A definition with no `<filesize>` is not checked.
    ROM_SIZE_MISMATCH = "DEF0029", Error, since "0.1.0";

    /// A definition names a storage type the encoding does not support.
    ///
    /// The storage string (for example `uint48` or a misspelt `unit16`) does
    /// not name any supported storage type, or names one in a combination
    /// (endianness, bit layout) that is not supported. The table is not
    /// decoded; its values are never shown as zero (MF-02).
    UNSUPPORTED_STORAGE_TYPE = "ENC0001", Error, since "0.1.0";

    /// A definition names a byte order other than big or little endian.
    ///
    /// The endian string (for example `middle`, or legacy's internal codes
    /// `1` / `2`) is neither `big` nor `little`. The table is not decoded:
    /// guessing a byte order (legacy assumed little-endian) shows scrambled
    /// values.
    UNKNOWN_ENDIAN = "ENC0002", Error, since "0.1.0";

    /// A bitfield mask or shift cannot apply to the storage it is on.
    ///
    /// The mask is not a hexadecimal number, is zero, has bits outside the
    /// stored value, or is on a float storage; or the shift would drop field
    /// bits; or a signed field's mask is not one contiguous run of at least
    /// two bits. The table is not decoded: legacy silently clamped such masks
    /// and showed values from the wrong bits.
    INVALID_BITFIELD = "ENC0003", Error, since "0.1.0";

    /// A definition names a table data layout that does not exist.
    ///
    /// The layout string is not `bosch_subtract` (or empty / `default`). The
    /// table is not decoded: legacy fell back to the default layout, which
    /// shows a Bosch-subtract table as meaningless values.
    UNKNOWN_LAYOUT = "ENC0004", Error, since "0.1.0";

    /// A cell does not fit in the bytes it is read from or written to.
    ///
    /// The cell needs more bytes than the image (or buffer) has from the
    /// cell's address, for example a table whose address runs past the end of
    /// the ROM. Nothing is read or written: legacy padded or wrapped such
    /// reads and showed values that are not in the image.
    CELL_OUT_OF_BOUNDS = "ENC0005", Error, since "0.1.0";

    /// A raw value does not fit the storage type or bitfield it is written to.
    ///
    /// The value is below the minimum or above the maximum of the storage
    /// (for example 256 into `u8`, or -1 into `u16`), or has bits outside the
    /// bitfield. Nothing is written: the value is never truncated or wrapped
    /// to fit.
    RAW_OUT_OF_RANGE = "ENC0006", Error, since "0.1.0";

    /// A raw value of the wrong kind for its storage type.
    ///
    /// An integer raw value was written to float storage, a float to integer
    /// storage, or an `f32` to `f64` storage (or the reverse). Nothing is
    /// written: converting between kinds silently would change the value.
    RAW_KIND_MISMATCH = "ENC0007", Error, since "0.1.0";

    /// One cell of a whole-table layout was decoded or encoded on its own.
    ///
    /// In the Bosch-subtract layout each value depends on every later cell of
    /// the table, so a single cell has no value by itself. Nothing is read or
    /// written; the table must be decoded or encoded as a whole.
    WHOLE_TABLE_LAYOUT = "ENC0008", Error, since "0.1.0";

    /// An edit was refused because a value is a NaN, infinite or subnormal float.
    ///
    /// The value stored in the cell, or the value the edit would write, is a
    /// NaN (quiet or signalling), `+Inf`/`-Inf` or a subnormal (denormal)
    /// float, and special floats were not explicitly allowed (PROJECT.md
    /// EC-07). Such values are decoded and displayed exactly as stored;
    /// nothing is written unless the user explicitly allows editing them.
    SPECIAL_FLOAT_EDIT = "ENC0009", Error, since "0.1.0";

    /// A formula is not valid expression syntax.
    ///
    /// The formula has a character or token where none can be (an
    /// unbalanced parenthesis, two operands with no operator between them, a
    /// `=`, a malformed `[id:unit]` reference, a number too large for a
    /// double). The scaling or parameter that owns it is not loaded: legacy
    /// reported such formulas only when a cell was first drawn.
    EXPR_SYNTAX = "EXP0001", Error, since "0.1.0";

    /// A formula calls a function that does not exist, or calls one wrongly.
    ///
    /// The name is not a supported function (names are case-sensitive:
    /// `BitWise`, `sqrt`), is `rand` or `str`, which formulas may not use, is
    /// used without an argument list, or is given the wrong number of
    /// arguments; or `BitWise` is given a literal operation that is not 1 to
    /// 7. The scaling or parameter that owns the formula is not loaded.
    EXPR_BAD_CALL = "EXP0002", Error, since "0.1.0";

    /// A formula is nested too deeply to parse safely.
    ///
    /// Parentheses, prefix operators, `^` chains, right operands or call
    /// arguments are nested deeper than the parser's depth limit (128 levels
    /// by default; real formulas nest under 10). Parsing stops at the level
    /// past the limit, which the diagnostic points at. The scaling or
    /// parameter that owns the formula is not loaded.
    EXPR_TOO_COMPLEX = "EXP0003", Error, since "0.1.0";

    /// A formula is longer than the parser accepts.
    ///
    /// The formula text is over the length limit (4096 bytes by default;
    /// real formulas are under 100). It is refused before it is read. The
    /// scaling or parameter that owns the formula is not loaded.
    EXPR_TOO_LONG = "EXP0004", Error, since "0.1.0";

    /// A number, name or parameter reference in a formula is too long.
    ///
    /// One literal token (a number such as `4.8828125e-4`, a name such as
    /// `P12`, or a whole `[id:unit]` reference) is over the literal length
    /// limit (128 bytes by default). The diagnostic points at the token. The
    /// scaling or parameter that owns the formula is not loaded.
    EXPR_LITERAL_TOO_LONG = "EXP0005", Error, since "0.1.0";

    /// A formula has too many operands, operators and calls.
    ///
    /// Its parsed tree would have more nodes than the node limit (1024 by
    /// default; real formulas have under 30): one per number, `x`,
    /// reference, operator and call. The diagnostic points at the token
    /// that would have been one too many. The scaling or parameter that owns
    /// the formula is not loaded.
    EXPR_TOO_MANY_NODES = "EXP0006", Error, since "0.1.0";

    /// A function call in a formula has too many arguments.
    ///
    /// A call is given more than the argument limit (32 by default; `sum`
    /// takes any number of arguments, every other function at most 4). The
    /// diagnostic points at the first argument too many. The scaling or parameter that owns
    /// the formula is not loaded.
    EXPR_TOO_MANY_ARGS = "EXP0007", Error, since "0.1.0";

    /// A formula would need too large an evaluation stack.
    ///
    /// Evaluating it would hold more intermediate values at once than the
    /// evaluation stack limit (32 by default, the size evaluation keeps on
    /// the native stack; real formulas need under 6). Values pile up in
    /// right-nested operators (`1+(2+(3+...))`) and in calls with many
    /// arguments. The diagnostic points at the operand that would have been
    /// one value too many. The scaling or parameter that owns the formula
    /// is not loaded.
    EXPR_STACK_TOO_DEEP = "EXP0008", Error, since "0.1.0";

    /// A conversion formula refers to a parameter; it may use only `x`.
    ///
    /// A scaling's `to_real` or `to_raw` formula names something other than
    /// `x`, the value being converted: a parameter reference such as `P8`
    /// or `[P9:km/h]`, or a misspelt `X` (names are case-sensitive). A
    /// conversion has no other input, so the name has no value; legacy
    /// evaluated such a formula to NaN when a cell was drawn. The
    /// diagnostic points at the first such name. The scaling is not loaded.
    CONVERSION_PARAM_REF = "CNV0001", Error, since "0.1.0";

    /// A lookup-table conversion's points do not describe a function of the raw value.
    ///
    /// The table has fewer than two points, a raw or real value that is not
    /// a finite number, or a raw value that is not greater than the one
    /// before it (raw values must strictly increase, so every raw value has
    /// at most one real value). The conversion is not loaded.
    INVALID_LUT = "CNV0002", Error, since "0.1.0";

    /// A linear conversion has a factor or offset that is not a finite number.
    ///
    /// The `m` or `b` of `m * x + b` is NaN or infinite, so no raw value has
    /// a finite real value. The conversion is not loaded.
    INVALID_LINEAR = "CNV0003", Error, since "0.1.0";

    /// A conversion gives no real value for any raw value the cell can hold.
    ///
    /// Over the whole raw range of the cell's storage, the conversion's
    /// result is NaN or infinite (`1/(x-x)`), or every raw value is outside
    /// its lookup table (a table covering 300 to 400 for an 8-bit cell), so
    /// there is no display range and nothing can be shown or edited. The
    /// diagnostic names the lowest raw value and why it has no value.
    CONVERSION_NO_VALUE = "CNV0004", Error, since "0.1.0";

    /// A scaling's `to_raw` formula does not invert its `to_real` formula.
    ///
    /// For some raw value the cell can hold, the definition's real-to-raw
    /// formula (RomRaider `to_byte`), rounded half up as legacy did, turns
    /// that raw value's real value into a raw value that displays
    /// differently: the two formulas disagree (a wrong factor, a missing
    /// offset). Edits use the exact inverse of `to_real` (or, where it is
    /// not linear, a numeric solve of it) and the `to_raw` formula is not
    /// used, so values typed are stored as shown; legacy used the `to_raw`
    /// formula and wrote the wrong bytes.
    /// The diagnostic names the first raw value found and both values.
    TO_RAW_MISMATCH = "CNV0005", Warning, since "0.1.0";

    /// A conversion cannot be inverted over the cell's raw range, so its values cannot be edited.
    ///
    /// Writing a value needs the one raw value whose real value is nearest
    /// it. A conversion that is not linear is solved numerically, which
    /// needs it to be monotonic over the raw values the cell can hold (a
    /// `to_raw` formula the definition gives is checked, never trusted, so
    /// it does not stand in); this one rises
    /// over some and falls over others (`(x-128)^2`, `1/(x-10)`), has no
    /// real value at a raw value between two that do (a division by zero
    /// inside the range), or gives every raw value the same real value. The
    /// diagnostic names the raw values where it rises and falls, or the one
    /// with no value. Legacy searched such a scaling anyway and wrote
    /// whichever raw value its search ended on. The values are shown;
    /// edits are refused.
    NOT_INVERTIBLE = "CNV0006", Error, since "0.1.0";

    /// A value typed for a cell is outside what the cell can hold.
    ///
    /// The raw value whose real value is nearest the value typed is outside
    /// the raw range of the cell's storage: the value is more than half a
    /// raw step below the lowest real value the cell can hold or above the
    /// highest (for a decreasing scaling, the lowest real value is the
    /// highest raw value's). The diagnostic names the value, the range it
    /// must be in and the raw value at the nearest end. Nothing is written:
    /// legacy narrowed the rounded raw value with a Java `int` cast, which
    /// wraps past `2^31`, then clamped it to the storage limits, so it wrote
    /// a different value without saying so.
    EDIT_OUT_OF_RANGE = "CNV0007", Error, since "0.1.0";

    /// A value typed for a cell is not a finite number, or the scaling has no value there.
    ///
    /// The value is NaN or infinite, or it lies past the last real value
    /// the scaling gives on that side where the next raw value has none (a
    /// division by zero, as at the pole of `1/(x-10)`, the square root of a
    /// negative number, an overflow), so there is no nearest raw value to
    /// round it to. The diagnostic names the value and, for the second
    /// case, the last real value and the raw value with none. Nothing is
    /// written.
    EDIT_NOT_FINITE = "CNV0008", Error, since "0.1.0";

    /// An `extends`/`base` chain loops back on itself.
    ///
    /// Following the inheritance chain from a document reaches the same
    /// document again. The document is not resolved; the provenance points
    /// at the reference that closes the loop. A document whose `base` is
    /// its own name is not a loop: it extends the definition of that name
    /// in an enclosing scope (`LAY0002` if there is none).
    INHERITANCE_CYCLE = "LAY0001", Error, since "0.1.0";

    /// A document names a base that no loaded file defines.
    ///
    /// The `base`/`extends` reference names a document that is in none of
    /// the loaded files, or only in a local scope the reference cannot see
    /// (MF-16: a name local to one table is not visible from the ROM, from
    /// another table, or globally); the message then names the scopes where
    /// it is defined. A document whose `base` is its own name extends the
    /// definition of that name in an enclosing scope, and gets this error
    /// when no enclosing scope has one (a ROM definition whose `base` is
    /// its own `xmlid`). The document, and everything inheriting from it, is
    /// not resolved; the provenance points at the document with the
    /// reference.
    UNKNOWN_BASE = "LAY0002", Error, since "0.1.0";

    /// Two loaded documents have the same name.
    ///
    /// Names are global across every loaded file, so a second document with
    /// a name already defined (in the same file or another one) would make
    /// every reference to that name ambiguous. The first document loaded is
    /// kept and the second is ignored; the provenance points at the ignored
    /// one and the message names where the kept one is. Legacy silently used
    /// whichever matching definition it found first.
    DUPLICATE_DOCUMENT = "LAY0003", Error, since "0.1.0";

    /// A document's bases cannot be put in one consistent order.
    ///
    /// The document names several bases whose own inheritance orders some
    /// documents both ways (for example one base puts `A` above `B` and
    /// another `B` above `A`), or names one base twice. The document is not
    /// resolved: picking an order arbitrarily would decide silently which
    /// value wins.
    INCONSISTENT_BASES = "LAY0004", Error, since "0.1.0";

    /// A document inherits from more documents than the resolver allows.
    ///
    /// Its inheritance would hold more than 255 documents besides itself
    /// (real definitions inherit from a handful). The document is not
    /// resolved; the limit bounds the work an untrusted definition pack can
    /// demand.
    LINEAGE_TOO_LARGE = "LAY0005", Error, since "0.1.0";

    /// A resolved document lacks required fields.
    ///
    /// After applying every layer it inherits from, the document still has
    /// required fields that no layer sets, or that a layer omits. The
    /// message names each missing field by path. The document is not used:
    /// a missing value is never filled with a default.
    INCOMPLETE_DOCUMENT = "LAY0006", Error, since "0.1.0";

    /// A document was asked for by a name that no loaded file defines.
    ///
    /// Something outside the definitions (a ROM image's identifier, a
    /// command-line argument, a setting) names a document that none of the
    /// loaded files has. The provenance points at where the name was asked
    /// for.
    UNKNOWN_DOCUMENT = "LAY0007", Error, since "0.1.0";

    /// A local document has the same name as one in an enclosing scope.
    ///
    /// A document defined inside a scope (a table-local scaling inside its
    /// table, inside its ROM) has the id of a document in a scope enclosing
    /// it (the ROM, or global). Inside its scope, a `base` naming that id
    /// now means the local document; outside, the outer one, which is
    /// unchanged. Both are loaded. Legacy had one flat namespace in which a
    /// later definition with the name replaced the earlier one for every
    /// lookup after it, so a pack relying on that resolves differently
    /// here; the provenance points at the local document and the message
    /// names where the shadowed one is. A local document whose `base` is
    /// its own name extends the outer one rather than hiding it, and is not
    /// warned about. Two documents with one name in the same scope are
    /// `LAY0003` instead.
    SHADOWED_NAME = "LAY0008", Warning, since "0.1.0";

    /// A byte pattern's text is not valid pattern syntax.
    ///
    /// The text (for example `80 F0 ?? 01`) has a character that is not a
    /// hex digit, `?`, `/`, `@`, space or tab; a byte with one digit
    /// (`80 F`); a `0x` with no digits after it; a `/` mask that is not two
    /// hex digits, follows a `?` wildcard, or leaves bits of its byte
    /// unchecked (`41/0F`); or no bytes at all. The definition or profile
    /// that owns the pattern is not used: a pattern guessed from malformed
    /// text could match the wrong ROM or frame.
    PATTERN_SYNTAX = "PAT0001", Error, since "0.1.0";

    /// A byte pattern's text or the pattern itself is longer than the parser accepts.
    ///
    /// The text is over the length limit (4096 bytes; real patterns are
    /// under 100), and is refused before it is read; or it gives more than
    /// 512 bytes of pattern (real patterns are under 32). The definition or
    /// profile that owns the pattern is not used.
    PATTERN_TOO_LONG = "PAT0002", Error, since "0.1.0";

    /// A byte pattern's `@` offset is malformed, misplaced or out of range.
    ///
    /// The `@` is not followed by decimal digits or `0x` and hex digits,
    /// comes after the bytes or twice, or gives an offset at which the
    /// pattern would end past the largest address. The definition or
    /// profile that owns the pattern is not used.
    PATTERN_BAD_OFFSET = "PAT0003", Error, since "0.1.0";

    /// An XML file is not well-formed.
    ///
    /// The file breaks XML's syntax: a tag is not closed or closes the wrong
    /// element, an attribute is unquoted, has no value or appears twice, a
    /// character reference such as `&#xZZ;` names no character, a bare `&`
    /// starts no reference, there is no root element or a second one, text
    /// sits outside the root, or the DTD's internal subset holds something
    /// other than declarations. The position is where reading stopped. The
    /// file is not read: no part of a file that is not well-formed is
    /// trusted.
    XML_MALFORMED = "XML0001", Error, since "0.1.0";

    /// An XML file is not in an encoding the reader supports.
    ///
    /// The reader reads UTF-8 (with or without a byte-order mark), and
    /// ISO-8859-1 or US-ASCII when the XML declaration says so
    /// (`<?xml version="1.0" encoding="ISO-8859-1"?>`). The file starts
    /// with a UTF-16 or UTF-32 byte-order mark (or is UTF-16 or UTF-32
    /// without one), its declaration names another encoding (the position
    /// is the name), or its bytes are not valid in its encoding (the
    /// position is the first byte that does not decode: typically an
    /// ISO-8859-1 `°` or `µ` in a file without the declaration that says
    /// so). The file is not read; re-save it as UTF-8, or add the
    /// declaration.
    XML_ENCODING = "XML0002", Error, since "0.1.0";

    /// An XML file's DTD declares an entity or uses a parameter entity.
    ///
    /// The DOCTYPE's internal subset has an `<!ENTITY>` declaration (general
    /// or `%` parameter, internal or `SYSTEM`/`PUBLIC` external) or a
    /// `%name;` reference. Entity declarations are how billion-laughs files
    /// exhaust memory and external-entity (XXE) files read local files or
    /// reach the network, so the reader refuses them outright and never
    /// expands an entity. The file is not read. A DOCTYPE that only names an
    /// external DTD (`<!DOCTYPE roms SYSTEM "ecu_defs.dtd">`) is fine: the
    /// DTD is never fetched.
    XML_ENTITY_DECLARED = "XML0003", Error, since "0.1.0";

    /// An XML file refers to an entity other than the predefined ones.
    ///
    /// Text or an attribute value has a `&name;` reference whose name is not
    /// `lt`, `gt`, `amp`, `apos` or `quot`. Without a DTD no other entity
    /// exists, and the reader never looks one up. The file is not read; if
    /// the `&` was meant literally, write `&amp;`.
    XML_UNKNOWN_ENTITY = "XML0004", Error, since "0.1.0";

    /// An XML file's DTD gives an attribute a default value, which is not applied.
    ///
    /// An `<!ATTLIST>` in the DOCTYPE's internal subset declares a default
    /// (or `#FIXED`) value. Legacy's Java parser added such defaults to
    /// every element lacking the attribute; this reader processes no DTD,
    /// so the element is read as written and the definition reader's own
    /// default for the attribute applies. The file is still read.
    XML_DTD_DEFAULT_IGNORED = "XML0005", Warning, since "0.1.0";

    /// An XML file is larger than the reader accepts.
    ///
    /// The file is over the input length limit (64 MiB by default; the
    /// largest pinned definition is under 9 MiB). It is refused before any
    /// of it is read.
    XML_TOO_LARGE = "XML0006", Error, since "0.1.0";

    /// An XML file nests elements too deeply.
    ///
    /// An element is nested deeper than the depth limit (64 levels by
    /// default, never more than 1024; real definitions nest under 10). The
    /// position is the first element past the limit. The file is not read.
    XML_TOO_DEEP = "XML0007", Error, since "0.1.0";

    /// An XML file has more elements than the reader accepts.
    ///
    /// The file has more elements than the element limit (1 million by
    /// default; the largest pinned definition has under 110 thousand). The
    /// position is the first element past the limit. The file is not read.
    XML_TOO_MANY_ELEMENTS = "XML0008", Error, since "0.1.0";

    /// An XML element or file has more attributes than the reader accepts.
    ///
    /// One element has more attributes than the per-element limit (64 by
    /// default; real definitions use under 20), or the whole file more than
    /// the total limit (2 million by default; the largest pinned definition
    /// has under 220 thousand). The position is the first attribute past the
    /// limit. The file is not read.
    XML_TOO_MANY_ATTRIBUTES = "XML0009", Error, since "0.1.0";

    /// An XML name or attribute value is longer than the reader accepts.
    ///
    /// An element name, attribute name or attribute value (as written,
    /// before unescaping) is over the length limit (64 KiB by default; real
    /// values are under 1 KiB). It is refused before it is copied. The file
    /// is not read.
    XML_VALUE_TOO_LONG = "XML0010", Error, since "0.1.0";

    /// An XML file holds more text than the reader accepts.
    ///
    /// The element text and attribute values kept from the file add up to
    /// more than the text limit (32 MiB by default; the largest pinned
    /// definition keeps under 4 MiB). The position is the
    /// text that would have gone over. The file is not read.
    XML_TEXT_TOO_LONG = "XML0011", Error, since "0.1.0";

    /// An XML file's byte-order mark and XML declaration disagree on the encoding.
    ///
    /// The file starts with a UTF-8 byte-order mark but its declaration
    /// names another encoding, such as `encoding="ISO-8859-1"`: usually an
    /// editor that re-saved the file as UTF-8 without updating the
    /// declaration. The byte-order mark wins and the file is read as UTF-8
    /// (bytes that are not UTF-8 are still refused, `XML0002`). The
    /// position is the declared name; correct it to `UTF-8`.
    XML_ENCODING_MISMATCH = "XML0012", Warning, since "0.1.0";

    /// A definition's unit string names no unit the registry knows.
    ///
    /// The string (a `<scaling units>` or logger `<conversion units>`, say)
    /// is neither one of the legacy unit strings mapped to a registry unit
    /// nor a label of the form `Quantity (unit)` whose bracketed part is
    /// one, nor a string that says the value has no unit (`raw ecu value`,
    /// `count`). The message quotes it. The value is still shown, with the
    /// string as its label, but it cannot be converted to another unit or
    /// given a unit's default gauge range; legacy RomRaider gave every
    /// string it did not know a 0 to 100 gauge without a word. Strings are
    /// matched exactly, so `c` is not `C`: use one of the spellings listed
    /// in `rr_core::units::LEGACY_ALIASES`.
    UNKNOWN_UNIT_STRING = "UNT0001", Warning, since "0.1.0";
}

/// Retired codes, which may never be reused. Empty until a code is retired.
pub const DEPRECATED: &[DeprecatedCode] = &[];

/// The registry entry for `code`, if it is active.
#[must_use]
pub fn lookup(code: Code) -> Option<&'static CodeInfo> {
    REGISTRY.iter().find(|info| info.code == code)
}

/// The deprecation entry for `code`, if it has been retired.
#[must_use]
pub fn deprecation(code: Code) -> Option<&'static DeprecatedCode> {
    DEPRECATED.iter().find(|d| d.code == code)
}

/// A broken registry rule, reported by [`problems`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryProblem {
    /// The same code appears twice among the active and deprecated entries.
    DuplicateCode(Code),
    /// Two active entries share a constant name.
    DuplicateName(&'static str),
    /// A code is not three upper-case letters and four digits.
    MalformedCode(Code),
    /// A code's prefix is not a [`DOMAINS`] prefix.
    UnknownDomain(Code),
    /// An active entry has no documentation summary.
    MissingDoc(Code),
    /// A deprecated entry has no reason.
    MissingReason(Code),
    /// A deprecated entry points at a replacement that is not active.
    UnknownReplacement {
        /// The deprecated code.
        code: Code,
        /// Its claimed replacement.
        replaced_by: Code,
    },
}

impl fmt::Display for RegistryProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCode(c) => write!(f, "code {c} is registered more than once"),
            Self::DuplicateName(n) => write!(f, "name {n} is registered more than once"),
            Self::MalformedCode(c) => write!(
                f,
                "code {:?} is not three upper-case letters and four digits",
                c.as_str()
            ),
            Self::UnknownDomain(c) => write!(f, "code {c} has a prefix not listed in DOMAINS"),
            Self::MissingDoc(c) => write!(f, "code {c} has no documentation"),
            Self::MissingReason(c) => write!(f, "deprecated code {c} has no reason"),
            Self::UnknownReplacement { code, replaced_by } => write!(
                f,
                "deprecated code {code} is replaced by {replaced_by}, which is not an active code"
            ),
        }
    }
}

/// `true` when `code` is three ASCII upper-case letters and four digits.
#[must_use]
pub fn is_well_formed(code: Code) -> bool {
    let b = code.as_str().as_bytes();
    b.len() == 7
        && b.iter().take(3).all(u8::is_ascii_uppercase)
        && b.iter().skip(3).all(u8::is_ascii_digit)
}

fn prefix(code: Code) -> &'static str {
    code.as_str().get(..3).unwrap_or("")
}

/// Every rule `registry`, `deprecated` and `domains` break. Empty when the
/// tables are sound. Pure, so tests can feed it broken tables.
#[must_use]
pub fn problems(
    registry: &[CodeInfo],
    deprecated: &[DeprecatedCode],
    domains: &[Domain],
) -> Vec<RegistryProblem> {
    let mut out = Vec::new();
    let all_codes = registry
        .iter()
        .map(|i| i.code)
        .chain(deprecated.iter().map(|d| d.code));
    let mut seen: Vec<Code> = Vec::new();
    for code in all_codes {
        if seen.contains(&code) {
            if !out.contains(&RegistryProblem::DuplicateCode(code)) {
                out.push(RegistryProblem::DuplicateCode(code));
            }
        } else {
            seen.push(code);
        }
        if !is_well_formed(code) {
            out.push(RegistryProblem::MalformedCode(code));
        } else if !domains.iter().any(|d| d.prefix == prefix(code)) {
            out.push(RegistryProblem::UnknownDomain(code));
        }
    }
    let mut names: Vec<&str> = Vec::new();
    for info in registry {
        if names.contains(&info.name) {
            out.push(RegistryProblem::DuplicateName(info.name));
        } else {
            names.push(info.name);
        }
        if info.summary().is_empty() {
            out.push(RegistryProblem::MissingDoc(info.code));
        }
    }
    for d in deprecated {
        if d.reason.trim().is_empty() {
            out.push(RegistryProblem::MissingReason(d.code));
        }
        if let Some(replaced_by) = d.replaced_by
            && !registry.iter().any(|i| i.code == replaced_by)
        {
            out.push(RegistryProblem::UnknownReplacement {
                code: d.code,
                replaced_by,
            });
        }
    }
    out
}

/// `docs/diagnostics.md` for the real tables.
#[must_use]
pub fn render_markdown() -> String {
    render_markdown_from(REGISTRY, DEPRECATED, DOMAINS)
}

/// `docs/diagnostics.md` for the given tables: a summary table and one
/// section per code, grouped by domain and sorted by code, then the
/// deprecated codes. Deterministic, `\n` line endings, one trailing newline.
#[must_use]
pub fn render_markdown_from(
    registry: &[CodeInfo],
    deprecated: &[DeprecatedCode],
    domains: &[Domain],
) -> String {
    let mut s = String::new();
    // Writing to a String cannot fail; `let _ =` keeps clippy quiet without
    // an unwrap.
    let _ = render_into(&mut s, registry, deprecated, domains);
    s
}

fn render_into(
    s: &mut String,
    registry: &[CodeInfo],
    deprecated: &[DeprecatedCode],
    domains: &[Domain],
) -> fmt::Result {
    writeln!(
        s,
        "<!-- Generated by `cargo xtask diagnostics-doc` from \
         crates/rr-core/src/diag/codes.rs. Do not edit by hand. -->"
    )?;
    writeln!(s)?;
    writeln!(s, "# Diagnostic codes")?;
    writeln!(s)?;
    writeln!(
        s,
        "Every diagnostic (concept C15) carries a stable code. Tests and tools \
         match on the code; the message is for people and may be reworded. A \
         code keeps its meaning forever: retired codes are listed under \
         [Deprecated codes](#deprecated-codes) and are never reused."
    )?;
    writeln!(s)?;
    writeln!(
        s,
        "An `error` makes the owning load fail closed: the caller gets no \
         usable value without explicitly accepting the errors."
    )?;

    let mut sorted: Vec<&CodeInfo> = registry.iter().collect();
    sorted.sort_by_key(|i| i.code);

    writeln!(s)?;
    writeln!(s, "| Code | Name | Severity | Summary |")?;
    writeln!(s, "| --- | --- | --- | --- |")?;
    for d in domains {
        for i in sorted.iter().filter(|i| prefix(i.code) == d.prefix) {
            writeln!(
                s,
                "| [`{code}`](#{anchor}) | `{name}` | {sev} | {summary} |",
                code = i.code,
                anchor = i.code.as_str().to_ascii_lowercase(),
                name = i.name,
                sev = i.severity,
                summary = i.summary(),
            )?;
        }
    }

    for d in domains {
        let codes: Vec<&&CodeInfo> = sorted
            .iter()
            .filter(|i| prefix(i.code) == d.prefix)
            .collect();
        writeln!(s)?;
        writeln!(s, "## {}: {}", d.prefix, d.title)?;
        writeln!(s)?;
        writeln!(s, "{}", d.description)?;
        if codes.is_empty() {
            writeln!(s)?;
            writeln!(s, "No codes yet.")?;
        }
        for i in codes {
            writeln!(s)?;
            writeln!(s, "### {}", i.code)?;
            writeln!(s)?;
            writeln!(
                s,
                "`{}` · severity `{}` · since {}",
                i.name, i.severity, i.since
            )?;
            writeln!(s)?;
            let lines: Vec<&str> = i.doc_lines().map(str::trim_end).collect();
            let end = lines
                .iter()
                .rposition(|l| !l.is_empty())
                .map_or(0, |p| p + 1);
            for line in lines.iter().take(end) {
                writeln!(s, "{line}")?;
            }
        }
    }

    writeln!(s)?;
    writeln!(s, "## Deprecated codes")?;
    writeln!(s)?;
    if deprecated.is_empty() {
        writeln!(s, "None.")?;
    } else {
        let mut retired: Vec<&DeprecatedCode> = deprecated.iter().collect();
        retired.sort_by_key(|d| d.code);
        writeln!(s, "| Code | Name | Deprecated in | Replaced by | Reason |")?;
        writeln!(s, "| --- | --- | --- | --- | --- |")?;
        for d in retired {
            let replaced = d
                .replaced_by
                .map_or_else(|| "—".to_owned(), |c| format!("`{c}`"));
            writeln!(
                s,
                "| `{}` | `{}` | {} | {} | {} |",
                d.code,
                d.name,
                d.deprecated_in,
                replaced,
                d.reason.trim()
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(code: &'static str, name: &'static str) -> CodeInfo {
        CodeInfo {
            code: Code::new(code),
            name,
            severity: Severity::Error,
            since: "0.1.0",
            doc: &[" Something went wrong.", "", " More detail."],
        }
    }

    fn retired(code: &'static str, replaced_by: Option<&'static str>) -> DeprecatedCode {
        DeprecatedCode {
            code: Code::new(code),
            name: "OLD",
            deprecated_in: "0.2.0",
            reason: "split into finer codes",
            replaced_by: replaced_by.map(Code::new),
        }
    }

    /// Every code ever published, as `(code, name)`. Append-only: add a row
    /// when a code is added; never edit or delete one. Retiring a code moves
    /// it to [`DEPRECATED`] and leaves its row here.
    const PUBLISHED: &[(&str, &str)] = &[
        ("DEF0001", "UNKNOWN_SCALING"),
        ("ENC0001", "UNSUPPORTED_STORAGE_TYPE"),
        ("ENC0002", "UNKNOWN_ENDIAN"),
        ("ENC0003", "INVALID_BITFIELD"),
        ("ENC0004", "UNKNOWN_LAYOUT"),
        ("ENC0005", "CELL_OUT_OF_BOUNDS"),
        ("ENC0006", "RAW_OUT_OF_RANGE"),
        ("ENC0007", "RAW_KIND_MISMATCH"),
        ("ENC0008", "WHOLE_TABLE_LAYOUT"),
        ("ENC0009", "SPECIAL_FLOAT_EDIT"),
        ("LAY0001", "INHERITANCE_CYCLE"),
        ("EXP0001", "EXPR_SYNTAX"),
        ("EXP0002", "EXPR_BAD_CALL"),
        ("EXP0003", "EXPR_TOO_COMPLEX"),
        ("EXP0004", "EXPR_TOO_LONG"),
        ("EXP0005", "EXPR_LITERAL_TOO_LONG"),
        ("EXP0006", "EXPR_TOO_MANY_NODES"),
        ("EXP0007", "EXPR_TOO_MANY_ARGS"),
        ("EXP0008", "EXPR_STACK_TOO_DEEP"),
        ("LAY0002", "UNKNOWN_BASE"),
        ("LAY0003", "DUPLICATE_DOCUMENT"),
        ("LAY0004", "INCONSISTENT_BASES"),
        ("LAY0005", "LINEAGE_TOO_LARGE"),
        ("LAY0006", "INCOMPLETE_DOCUMENT"),
        ("LAY0007", "UNKNOWN_DOCUMENT"),
        ("LAY0008", "SHADOWED_NAME"),
        ("PAT0001", "PATTERN_SYNTAX"),
        ("PAT0002", "PATTERN_TOO_LONG"),
        ("PAT0003", "PATTERN_BAD_OFFSET"),
        ("XML0001", "XML_MALFORMED"),
        ("XML0002", "XML_ENCODING"),
        ("XML0003", "XML_ENTITY_DECLARED"),
        ("XML0004", "XML_UNKNOWN_ENTITY"),
        ("XML0005", "XML_DTD_DEFAULT_IGNORED"),
        ("XML0006", "XML_TOO_LARGE"),
        ("XML0007", "XML_TOO_DEEP"),
        ("XML0008", "XML_TOO_MANY_ELEMENTS"),
        ("XML0009", "XML_TOO_MANY_ATTRIBUTES"),
        ("XML0010", "XML_VALUE_TOO_LONG"),
        ("XML0011", "XML_TEXT_TOO_LONG"),
        ("XML0012", "XML_ENCODING_MISMATCH"),
        ("CNV0001", "CONVERSION_PARAM_REF"),
        ("CNV0002", "INVALID_LUT"),
        ("CNV0003", "INVALID_LINEAR"),
        ("CNV0004", "CONVERSION_NO_VALUE"),
        ("CNV0005", "TO_RAW_MISMATCH"),
        ("CNV0006", "NOT_INVERTIBLE"),
        ("CNV0007", "EDIT_OUT_OF_RANGE"),
        ("CNV0008", "EDIT_NOT_FINITE"),
        ("DEF0002", "NOT_ROM_DEFINITION"),
        ("DEF0003", "SINGLE_ROM_ROOT"),
        ("DEF0004", "UNKNOWN_ELEMENT"),
        ("DEF0005", "UNKNOWN_ATTRIBUTE"),
        ("DEF0006", "UNSUPPORTED_ELEMENT"),
        ("DEF0007", "UNEXPECTED_TEXT"),
        ("DEF0008", "DUPLICATE_ELEMENT"),
        ("DEF0009", "MISSING_XMLID"),
        ("DEF0010", "INVALID_ADDRESS"),
        ("DEF0011", "INVALID_FILE_SIZE"),
        ("DEF0012", "INVALID_ROM_ID"),
        ("DEF0013", "ROM_ID_INCOMPLETE"),
        ("DEF0014", "INVALID_BOOLEAN"),
        ("DEF0015", "MISSING_TABLE_NAME"),
        ("DEF0016", "INVALID_TABLE_TYPE"),
        ("DEF0017", "INVALID_NUMBER"),
        ("DEF0018", "USER_LEVEL_OUT_OF_RANGE"),
        ("DEF0019", "MISSING_AXIS"),
        ("DEF0020", "CONTENT_IGNORED"),
        ("DEF0021", "INVALID_STATE_DATA"),
        ("DEF0022", "STATIC_AXIS_SIZE"),
        ("DEF0023", "INVALID_SCALING_NUMBER"),
        ("DEF0024", "MISSING_STORAGE_TYPE"),
        ("DEF0025", "UNUSABLE_TO_BYTE"),
        ("DEF0026", "DUPLICATE_XMLID"),
        ("DEF0027", "UNTYPED_TABLE"),
        ("DEF0028", "AMBIGUOUS_ROM_ID"),
        ("DEF0029", "ROM_SIZE_MISMATCH"),
        ("UNT0001", "UNKNOWN_UNIT_STRING"),
    ];

    /// A break of the stability rules against the published ledger.
    #[derive(Debug, PartialEq, Eq)]
    enum LedgerProblem {
        /// A published code is neither active nor deprecated.
        RemovedWithoutDeprecation(&'static str),
        /// A published code now carries a different name (renumbered, or
        /// reused for another meaning).
        NameChanged {
            code: &'static str,
            published: &'static str,
            now: &'static str,
        },
        /// An active or deprecated code has no ledger row.
        Unpublished(&'static str),
        /// The ledger lists the same code twice.
        DuplicateRow(&'static str),
    }

    fn ledger_problems(
        ledger: &[(&'static str, &'static str)],
        registry: &[CodeInfo],
        deprecated: &[DeprecatedCode],
    ) -> Vec<LedgerProblem> {
        let mut out = Vec::new();
        for (i, &(code, published)) in ledger.iter().enumerate() {
            if ledger.iter().take(i).any(|&(c, _)| c == code) {
                out.push(LedgerProblem::DuplicateRow(code));
                continue;
            }
            let now = registry
                .iter()
                .find(|r| r.code.as_str() == code)
                .map(|r| r.name)
                .or_else(|| {
                    deprecated
                        .iter()
                        .find(|d| d.code.as_str() == code)
                        .map(|d| d.name)
                });
            match now {
                None => out.push(LedgerProblem::RemovedWithoutDeprecation(code)),
                Some(now) if now != published => out.push(LedgerProblem::NameChanged {
                    code,
                    published,
                    now,
                }),
                Some(_) => {}
            }
        }
        let current = registry
            .iter()
            .map(|r| r.code.as_str())
            .chain(deprecated.iter().map(|d| d.code.as_str()));
        for code in current {
            if !ledger.iter().any(|&(c, _)| c == code) {
                out.push(LedgerProblem::Unpublished(code));
            }
        }
        out
    }

    #[test]
    fn published_codes_are_stable() {
        let found = ledger_problems(PUBLISHED, REGISTRY, DEPRECATED);
        assert!(
            found.is_empty(),
            "diagnostic codes broke the stability rules (see the module docs): {found:?}"
        );
    }

    #[test]
    fn removing_a_code_without_deprecation_breaks_stability() {
        let ledger = [("DEF0001", "A"), ("DEF0002", "B")];
        let reg = [info("DEF0001", "A")];
        assert_eq!(
            ledger_problems(&ledger, &reg, &[]),
            vec![LedgerProblem::RemovedWithoutDeprecation("DEF0002")]
        );
        let mut dep = retired("DEF0002", Some("DEF0001"));
        dep.name = "B";
        assert!(ledger_problems(&ledger, &reg, &[dep]).is_empty());
    }

    #[test]
    fn reusing_or_renumbering_a_code_breaks_stability() {
        let ledger = [("DEF0001", "A")];
        // DEF0001 reused for a new meaning.
        let reused = [info("DEF0001", "NEW_MEANING")];
        assert_eq!(
            ledger_problems(&ledger, &reused, &[]),
            vec![LedgerProblem::NameChanged {
                code: "DEF0001",
                published: "A",
                now: "NEW_MEANING",
            }]
        );
        // A renumbered to DEF0002: the old code vanished, the new one is unpublished.
        let renumbered = [info("DEF0002", "A")];
        assert_eq!(
            ledger_problems(&ledger, &renumbered, &[]),
            vec![
                LedgerProblem::RemovedWithoutDeprecation("DEF0001"),
                LedgerProblem::Unpublished("DEF0002"),
            ]
        );
        // Deprecated under a different name than it was published with.
        assert_eq!(
            ledger_problems(&ledger, &[], &[retired("DEF0001", None)]),
            vec![LedgerProblem::NameChanged {
                code: "DEF0001",
                published: "A",
                now: "OLD",
            }]
        );
    }

    #[test]
    fn ledger_must_list_every_code_once() {
        let reg = [info("DEF0001", "A"), info("DEF0002", "B")];
        assert_eq!(
            ledger_problems(&[("DEF0001", "A"), ("DEF0001", "A")], &reg, &[]),
            vec![
                LedgerProblem::DuplicateRow("DEF0001"),
                LedgerProblem::Unpublished("DEF0002"),
            ]
        );
    }

    #[test]
    fn real_registry_has_no_problems() {
        let found = problems(REGISTRY, DEPRECATED, DOMAINS);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn real_registry_is_not_empty_and_every_domain_is_unique() {
        assert!(!REGISTRY.is_empty());
        for (i, d) in DOMAINS.iter().enumerate() {
            assert_eq!(d.prefix.len(), 3);
            assert!(d.prefix.bytes().all(|b| b.is_ascii_uppercase()));
            assert!(
                DOMAINS.iter().skip(i + 1).all(|o| o.prefix != d.prefix),
                "domain {} listed twice",
                d.prefix
            );
        }
    }

    #[test]
    fn constants_are_the_registry_rows() {
        assert_eq!(UNKNOWN_SCALING.as_str(), "DEF0001");
        assert_eq!(UNSUPPORTED_STORAGE_TYPE.as_str(), "ENC0001");
        assert_eq!(UNKNOWN_ENDIAN.as_str(), "ENC0002");
        assert_eq!(INVALID_BITFIELD.as_str(), "ENC0003");
        assert_eq!(UNKNOWN_LAYOUT.as_str(), "ENC0004");
        assert_eq!(CELL_OUT_OF_BOUNDS.as_str(), "ENC0005");
        assert_eq!(RAW_OUT_OF_RANGE.as_str(), "ENC0006");
        assert_eq!(RAW_KIND_MISMATCH.as_str(), "ENC0007");
        assert_eq!(WHOLE_TABLE_LAYOUT.as_str(), "ENC0008");
        assert_eq!(SPECIAL_FLOAT_EDIT.as_str(), "ENC0009");
        assert_eq!(INHERITANCE_CYCLE.as_str(), "LAY0001");
        assert_eq!(UNKNOWN_BASE.as_str(), "LAY0002");
        assert_eq!(DUPLICATE_DOCUMENT.as_str(), "LAY0003");
        assert_eq!(INCONSISTENT_BASES.as_str(), "LAY0004");
        assert_eq!(LINEAGE_TOO_LARGE.as_str(), "LAY0005");
        assert_eq!(INCOMPLETE_DOCUMENT.as_str(), "LAY0006");
        assert_eq!(UNKNOWN_DOCUMENT.as_str(), "LAY0007");
        assert_eq!(SHADOWED_NAME.as_str(), "LAY0008");
        assert_eq!(
            lookup(SHADOWED_NAME).map(|r| r.severity),
            Some(Severity::Warning)
        );
        let row = lookup(UNSUPPORTED_STORAGE_TYPE).unwrap();
        assert_eq!(row.name, "UNSUPPORTED_STORAGE_TYPE");
        assert_eq!(row.severity, Severity::Error);
        assert_eq!(
            row.summary(),
            "A definition names a storage type the encoding does not support."
        );
        assert!(lookup(Code::new("DEF9999")).is_none());
        assert!(deprecation(UNKNOWN_SCALING).is_none());
    }

    #[test]
    fn duplicate_code_is_reported_once() {
        let reg = [
            info("DEF0001", "A"),
            info("DEF0001", "B"),
            info("DEF0001", "C"),
        ];
        assert_eq!(
            problems(&reg, &[], DOMAINS),
            vec![RegistryProblem::DuplicateCode(Code::new("DEF0001"))]
        );
    }

    #[test]
    fn deprecated_code_reused_as_active_is_a_duplicate() {
        let reg = [info("DEF0001", "A")];
        let dep = [retired("DEF0001", None)];
        assert_eq!(
            problems(&reg, &dep, DOMAINS),
            vec![RegistryProblem::DuplicateCode(Code::new("DEF0001"))]
        );
    }

    #[test]
    fn duplicate_name_is_reported() {
        let reg = [info("DEF0001", "A"), info("DEF0002", "A")];
        assert_eq!(
            problems(&reg, &[], DOMAINS),
            vec![RegistryProblem::DuplicateName("A")]
        );
    }

    #[test]
    fn malformed_and_unknown_domain_codes_are_reported() {
        let reg = [
            info("DEF001", "A"),
            info("def0001", "B"),
            info("DEF00012", "C"),
            info("D3F0001", "D"),
            info("XYZ0001", "E"),
            info("DEFA001", "F"),
        ];
        assert_eq!(
            problems(&reg, &[], DOMAINS),
            vec![
                RegistryProblem::MalformedCode(Code::new("DEF001")),
                RegistryProblem::MalformedCode(Code::new("def0001")),
                RegistryProblem::MalformedCode(Code::new("DEF00012")),
                RegistryProblem::MalformedCode(Code::new("D3F0001")),
                RegistryProblem::UnknownDomain(Code::new("XYZ0001")),
                RegistryProblem::MalformedCode(Code::new("DEFA001")),
            ]
        );
    }

    #[test]
    fn missing_doc_reason_and_bad_replacement_are_reported() {
        let mut undocumented = info("DEF0001", "A");
        undocumented.doc = &["   "];
        let mut no_reason = retired("DEF0002", Some("DEF0001"));
        no_reason.reason = " ";
        let dangling = retired("DEF0003", Some("DEF0042"));
        assert_eq!(
            problems(&[undocumented], &[no_reason, dangling], DOMAINS),
            vec![
                RegistryProblem::MissingDoc(Code::new("DEF0001")),
                RegistryProblem::MissingReason(Code::new("DEF0002")),
                RegistryProblem::UnknownReplacement {
                    code: Code::new("DEF0003"),
                    replaced_by: Code::new("DEF0042"),
                },
            ]
        );
    }

    #[test]
    fn valid_deprecation_with_replacement_is_accepted() {
        let reg = [info("DEF0002", "NEW")];
        let dep = [retired("DEF0001", Some("DEF0002"))];
        assert!(problems(&reg, &dep, DOMAINS).is_empty());
    }

    #[test]
    fn problems_display_name_the_code() {
        let all = [
            RegistryProblem::DuplicateCode(Code::new("DEF0001")),
            RegistryProblem::DuplicateName("A"),
            RegistryProblem::MalformedCode(Code::new("DEF1")),
            RegistryProblem::UnknownDomain(Code::new("XYZ0001")),
            RegistryProblem::MissingDoc(Code::new("DEF0001")),
            RegistryProblem::MissingReason(Code::new("DEF0001")),
            RegistryProblem::UnknownReplacement {
                code: Code::new("DEF0001"),
                replaced_by: Code::new("DEF0002"),
            },
        ];
        let rendered: Vec<String> = all.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            [
                "code DEF0001 is registered more than once",
                "name A is registered more than once",
                "code \"DEF1\" is not three upper-case letters and four digits",
                "code XYZ0001 has a prefix not listed in DOMAINS",
                "code DEF0001 has no documentation",
                "deprecated code DEF0001 has no reason",
                "deprecated code DEF0001 is replaced by DEF0002, which is not an active code",
            ]
        );
    }

    #[test]
    fn doc_lines_strip_one_leading_space() {
        let i = info("DEF0001", "A");
        let lines: Vec<&str> = i.doc_lines().collect();
        assert_eq!(lines, ["Something went wrong.", "", "More detail."]);
        assert_eq!(i.summary(), "Something went wrong.");
        let mut empty = i;
        empty.doc = &[];
        assert_eq!(empty.summary(), "");
    }

    #[test]
    fn markdown_lists_every_code_and_deprecation() {
        let reg = [info("DEF0002", "NEW"), info("DEF0001", "FIRST")];
        let dep = [
            retired("DEF0003", Some("DEF0002")),
            retired("DEF0000", None),
        ];
        let md = render_markdown_from(&reg, &dep, &DOMAINS[..2]);
        let expected = "\
<!-- Generated by `cargo xtask diagnostics-doc` from crates/rr-core/src/diag/codes.rs. Do not edit by hand. -->

# Diagnostic codes

Every diagnostic (concept C15) carries a stable code. Tests and tools match on the code; the message is for people and may be reworded. A code keeps its meaning forever: retired codes are listed under [Deprecated codes](#deprecated-codes) and are never reused.

An `error` makes the owning load fail closed: the caller gets no usable value without explicitly accepting the errors.

| Code | Name | Severity | Summary |
| --- | --- | --- | --- |
| [`DEF0001`](#def0001) | `FIRST` | error | Something went wrong. |
| [`DEF0002`](#def0002) | `NEW` | error | Something went wrong. |

## DEF: Definitions

Reading ROM, logger and device definitions (rr-defs): malformed or inconsistent definition content.

### DEF0001

`FIRST` · severity `error` · since 0.1.0

Something went wrong.

More detail.

### DEF0002

`NEW` · severity `error` · since 0.1.0

Something went wrong.

More detail.

## ENC: Encoding

Storage types, endianness and bit layouts (C3, `rr-core::encoding`): values a definition asks for that cannot be encoded or decoded.

No codes yet.

## Deprecated codes

| Code | Name | Deprecated in | Replaced by | Reason |
| --- | --- | --- | --- | --- |
| `DEF0000` | `OLD` | 0.2.0 | — | split into finer codes |
| `DEF0003` | `OLD` | 0.2.0 | `DEF0002` | split into finer codes |
";
        assert_eq!(md, expected);
    }

    #[test]
    fn real_markdown_documents_every_registered_code() {
        let md = render_markdown();
        if DEPRECATED.is_empty() {
            assert!(md.ends_with("None.\n"));
        }
        for d in DEPRECATED {
            assert!(
                md.contains(&format!("| `{}` | `{}` |", d.code, d.name)),
                "{}",
                d.code
            );
        }
        for i in REGISTRY {
            assert!(md.contains(&format!("### {}\n", i.code)), "{}", i.code);
            assert!(md.contains(i.summary()), "{}", i.code);
        }
        for d in DOMAINS {
            assert!(md.contains(&format!("## {}: {}\n", d.prefix, d.title)));
        }
    }
}
