//! `<table>`: one table of a ROM definition, with its axes, switch states
//! and bits, read as a partial document (C1); see [`TableDoc`].

use rr_core::diag::{Span, codes};
use rr_core::encoding::legacy::{parse_data_layout, parse_endian, parse_mask, parse_storage_type};
use rr_core::encoding::{BitField, EncodingError, Endian, Layout, Storage};
use rr_core::layer::{Appended, Field, Keyed, Merge as _, Nested};

use super::ctx::{Ctx, attribute, is};
use super::scaling::{ScalingDoc, ScalingsRead};
use super::value::{parse_bool, parse_decimal_int, parse_hex_int, parse_state_data};
use crate::xml::{Attribute, Element, Node};

/// A table's kind: legacy's `Table1D`, `Table2D`, `Table3D`, `TableSwitch`
/// and `TableBitwiseSwitch` (`xml/TableScaleUnmarshaller.java:105-135`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableKind {
    /// `type="1D"`: a row of values. `X Axis`, `Y Axis`, `Static X Axis`
    /// and `Static Y Axis` as a table's own type also read as 1D, as in
    /// legacy (`:115-121`).
    OneD,
    /// `type="2D"`: values along one axis.
    TwoD,
    /// `type="3D"`: a grid of values along an X and a Y axis.
    ThreeD,
    /// `type="Switch"`: bytes compared against named states.
    Switch,
    /// `type="BitwiseSwitch"`: named bits of one cell.
    BitwiseSwitch,
}

impl TableKind {
    /// The `type` attribute value that names this kind.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OneD => "1D",
            Self::TwoD => "2D",
            Self::ThreeD => "3D",
            Self::Switch => "Switch",
            Self::BitwiseSwitch => "BitwiseSwitch",
        }
    }

    /// Whether a table of this kind has axes.
    const fn has_axes(self) -> bool {
        matches!(self, Self::TwoD | Self::ThreeD)
    }
}

/// Where a table's or an axis's cells are, how they are stored and how
/// many there are: the attributes legacy read the same way on both
/// (`xml/TableScaleUnmarshaller.java:145-196`, `:279-286`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellsDoc {
    /// `storageaddress`: where the first cell is, as a ROM address (the
    /// ROM's `ram_offset` maps it to an image offset unless
    /// [`before_ram`](Self::before_ram)).
    pub storage_address: Field<u32>,
    /// `storagetype`: the cell storage, signedness included (MF-06).
    pub storage: Field<Storage>,
    /// `endian`: the cell byte order. A `<memmodel endian>` overrides it
    /// ([`RomIdDoc::mem_model_endian`](super::RomIdDoc::mem_model_endian)).
    pub endian: Field<Endian>,
    /// `mask`: the bits of each cell that hold the value.
    pub bitfield: Field<BitField>,
    /// `dataLayout`: how the cells encode the values.
    ///
    /// Legacy reset it to the default on every `<table>` without the
    /// attribute (`:150`), so a child definition changing a Bosch-subtract
    /// table lost its layout; here it is inherited like every other field.
    pub layout: Field<Layout>,
    /// `beforeram="true"`: the address is an image offset, not mapped by the
    /// ROM's `ram_offset`.
    ///
    /// Legacy only ever set it (`:145-148`): `beforeram="false"` leaves the
    /// inherited value, and so it does here.
    pub before_ram: Field<bool>,
    /// The cell count of a 1D, 2D or switch table or of an axis: the
    /// element's `sizey`, else its `sizex` (`:185-186`). (Legacy then gave
    /// an axis its table's size along it, `:235-236`, `:250-252`,
    /// `:264-266`.)
    pub size: Field<u32>,
    /// Whether the values run in reverse: the element's `flipy`, else its
    /// `flipx` (`:187-188`).
    pub flip: Field<bool>,
    /// `<data>` children: the values of a static axis (or 1D table), as
    /// written, read from the definition rather than the image
    /// (`maps/Table.java:104-113`).
    pub static_values: Field<Vec<String>>,
    /// `<scaling>` children: how the values convert and are shown, one per
    /// category, keyed by [`scaling_key`](super::scaling_key) (see
    /// [`ScalingDoc`]).
    pub scalings: Keyed<String, ScalingDoc>,
}

rr_core::merge_fields!(CellsDoc {
    storage_address,
    storage,
    endian,
    bitfield,
    layout,
    before_ram,
    size,
    flip,
    static_values,
    scalings,
});

/// One axis of a 2D or 3D table: a child `<table type="X Axis">` (or
/// `Y Axis`, `Static X Axis`, `Static Y Axis`), as a partial document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AxisDoc {
    /// `name`, as written.
    pub name: Field<String>,
    /// `description` or `<description>`.
    pub description: Field<String>,
    /// `logparam`: the logger parameter this axis follows.
    pub log_param: Field<String>,
    /// Where the axis values are and how they are stored, or its static
    /// `<data>` values.
    pub cells: CellsDoc,
}

rr_core::merge_fields!(AxisDoc {
    name,
    description,
    log_param,
    cells,
});

/// One `<state name data>` of a switch table: a named set of cell values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwitchState {
    /// `name`, as written (the key is lower-cased: legacy replaced a state
    /// whose name matched ignoring case, `maps/PresetManager.java:120-135`).
    pub name: Field<String>,
    /// `data`: one value per cell, in hexadecimal (`01 00`). As legacy
    /// (`maps/PresetManager.java:42-56`), a value of a 2- or 4-byte
    /// little-endian table is byte-swapped when compared with the cells.
    pub data: Field<Vec<u32>>,
}

rr_core::merge_fields!(SwitchState { name, data });

/// One `<bit position name>` of a bitwise switch table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitFlag {
    /// `name`, as written.
    pub name: String,
    /// `position`: the bit, 0 for the least significant.
    pub position: u8,
}

/// One `<table>` of a `<rom>`, as a partial document (C1): every field is
/// [`Field::Unset`] unless the element says it.
///
///
/// # The model
///
/// A `<rom>`'s tables are a [`Keyed`] list in [`RomDoc::tables`](super::RomDoc::tables),
/// keyed by the table name lower-cased: legacy kept a ROM's tables in a map
/// keyed by `getName().toLowerCase()` (`maps/Rom.java:208-240`), and a
/// `<table>` naming a table the ROM already has (from its `base`, or earlier
/// in the same `<rom>`) changed that table instead of making a new one
/// (`xml/DOMRomUnmarshaller.java:168-182`). So each `<table>` is a
/// [`TableDoc`] patch whose fields are [`Field::Unset`] unless the element
/// says them, and `omit="true"` is an omitted entry ([`Nested::Omit`],
/// legacy `TableIsOmittedException`, `xml/TableScaleUnmarshaller.java:82-85`).
/// A [`DefinitionSet`](super::DefinitionSet) stacks the patches along the `base` chain.
///
/// | Attribute / element | Field | Legacy (`xml/TableScaleUnmarshaller.java`) |
/// |---------------------|-------|--------------------------------------------|
/// | `name` | [`TableDoc::name`] (as written; the key is lower-cased) | `:143` |
/// | `type` | [`TableDoc::kind`] | `:103-135` |
/// | `base` | [`TableDoc::base`] | copy the ROM's table of that name, `:87-98` |
/// | `omit="true"` | the entry is omitted | `:82-85` |
/// | `category` | [`TableDoc::category`] | `:151` |
/// | `description`, `<description>` | [`TableDoc::description`] | `:181`, `:288-289` |
/// | `storageaddress` | [`CellsDoc::storage_address`] | `:175-178` |
/// | `storagetype` | [`CellsDoc::storage`] | `:153-161` |
/// | `endian` | [`CellsDoc::endian`] | `:162-170` |
/// | `mask` | [`CellsDoc::bitfield`] | `:195-196` |
/// | `dataLayout` | [`CellsDoc::layout`] | `:150` |
/// | `beforeram` | [`CellsDoc::before_ram`] | `:145-148` |
/// | `sizey`, else `sizex` | [`CellsDoc::size`] | `:185-186` |
/// | `flipy`, else `flipx` | [`CellsDoc::flip`] | `:187-188` |
/// | `sizex`, `sizey`, `flipx`, `flipy`, `swapxy`, `skipCells` | [`TableDoc::size_x`] … [`TableDoc::skip_cells`] (3D) | `:198-211` |
/// | `userlevel` | [`TableDoc::user_level`] | `:189-190` |
/// | `locked` | [`TableDoc::locked`] | `:191-192` |
/// | `logparam` | [`TableDoc::log_param`] | `:193-194` |
/// | `<data>` | [`CellsDoc::static_values`] | `:279-286` |
/// | `<table type="X Axis">` … | [`TableDoc::axis`], [`TableDoc::x_axis`], [`TableDoc::y_axis`] | `:220-271` |
/// | `<state name data>` | [`TableDoc::states`] | `:291-295` |
/// | `<bit position name>` | [`TableDoc::bits`] | `:296-299` |
/// | `<scaling>` | [`CellsDoc::scalings`] (see [`ScalingDoc`]) | `:273-277` |
///
/// Values are typed as they are read: a storage type is a [`Storage`], a
/// byte order an [`Endian`], a mask a [`BitField`] (C3), refused with an
/// `ENC` Error diagnostic at the attribute's value where they do not parse.
/// The [`Encoding`](rr_core::encoding::Encoding) and
/// [`Conversion`](rr_core::conversion::Conversion) themselves are built once
/// the layers are merged ([`build_cells`](super::build_cells)), since a
/// child definition may change only one of their parts.
///
/// # Per-element fields legacy read twice
///
/// Legacy set a table's one data size from `sizey`, else `sizex`, else the
/// inherited size, element by element, and a 3D table's two sizes from
/// `sizex` and `sizey` separately; likewise `flip` and `flipx` / `flipy`.
/// Both readings are kept ([`CellsDoc::size`] and
/// [`TableDoc::size_x`] / [`TableDoc::size_y`]), because which wins depends
/// on the layer order: a child's `sizex` overrides a base's `sizey` for a
/// 2D table.
///
/// A 2D table has one axis, and legacy read every axis child into it,
/// whichever its letter (`:223-241`); a 3D table reads its `X Axis` and its
/// `Y Axis` children into two (`:242-270`). A child definition that changes
/// an inherited table often has no `type`, so the reader cannot tell which
/// applies, and the pinned corpus has 2D tables whose base says `X Axis`
/// and whose child says `Y Axis`. So every axis child patches both
/// [`TableDoc::axis`] (the 2D reading) and its letter's
/// [`TableDoc::x_axis`] or [`TableDoc::y_axis`] (the 3D reading); the
/// resolved kind picks which to use.
///
/// # Legacy forms not read
///
/// * Axis types `4` and `5`, legacy's numeric codes for `X Axis` and
///   `Y Axis` (`xml/RomAttributeParser.java:92-103`, "only for backwards
///   compatibility, is anybody even using this?"): no pinned definition
///   uses them, and here they are not axis types (a `DEF0020` warning). As
///   a table's own `type` legacy refused them too.
/// * Numeric `storagetype` codes and `movi20s`, refused by
///   [`parse_storage_type`] (see [`rr_core::encoding::legacy`]).
///
/// # Diagnostics
///
/// Every unknown attribute (`DEF0005`, naming an axis as "an axis
/// `<table>`") and element (`DEF0004`) is a warning, and so is content that
/// legacy ignored where it is (`DEF0020`): an axis on a table that has
/// none, a child `<table>` that is not an axis, static `<data>` on a 2D or
/// 3D table. Malformed values are Errors: an address (`DEF0010`), a number
/// (`DEF0017`), a table type (`DEF0016`), switch data (`DEF0021`), and the
/// `ENC` codes for the encoding attributes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableDoc {
    /// `name`, as written.
    pub name: Field<String>,
    /// `type`. Legacy ignored it on a `<table>` changing an inherited
    /// table, which keeps its kind.
    pub kind: Field<TableKind>,
    /// `base`: the name of another table of the ROM this one starts as a
    /// copy of (resolved by a [`RomResolver`](super::RomResolver)).
    pub base: Field<String>,
    /// `category`: the group the table is listed under.
    pub category: Field<String>,
    /// `description` or `<description>`.
    pub description: Field<String>,
    /// `userlevel`: from 1 (beginner) to 5 (debug); hidden below it. Unset
    /// is legacy's 0: shown at every level.
    pub user_level: Field<u8>,
    /// `locked`: the table cannot be edited.
    pub locked: Field<bool>,
    /// `logparam`: the logger parameter for live data on this table.
    pub log_param: Field<String>,
    /// Where the cells are, how they are stored, the 1D/2D data size and
    /// flip, and static values.
    pub cells: CellsDoc,
    /// `sizex`: a 3D table's column count.
    pub size_x: Field<u32>,
    /// `sizey`: a 3D table's row count.
    pub size_y: Field<u32>,
    /// `flipx`: a 3D table's rows are stored in reverse.
    pub flip_x: Field<bool>,
    /// `flipy`: a 3D table's columns are stored in reverse.
    pub flip_y: Field<bool>,
    /// `swapxy`: a 3D table's cells are stored column by column.
    pub swap_xy: Field<bool>,
    /// `skipCells`: cells skipped after each stored row of a 3D table.
    pub skip_cells: Field<u32>,
    /// Every axis child, whichever its letter: a 2D table's one axis.
    pub axis: Nested<AxisDoc>,
    /// Every `X Axis` child: a 3D table's X axis.
    pub x_axis: Nested<AxisDoc>,
    /// Every `Y Axis` child: a 3D table's Y axis.
    pub y_axis: Nested<AxisDoc>,
    /// `<state>`s of a switch table, by lower-cased name.
    pub states: Keyed<String, SwitchState>,
    /// `<bit>`s of a bitwise switch table, appended to the inherited ones
    /// (legacy added them without checking names,
    /// `maps/TableBitwiseSwitch.java:70-75`).
    pub bits: Appended<BitFlag>,
}

rr_core::merge_fields!(TableDoc {
    name,
    kind,
    base,
    category,
    description,
    user_level,
    locked,
    log_param,
    cells,
    size_x,
    size_y,
    flip_x,
    flip_y,
    swap_xy,
    skip_cells,
    axis,
    x_axis,
    y_axis,
    states,
    bits,
});

/// The key of the table named `name` in [`RomDoc::tables`](super::RomDoc::tables):
/// the name lower-cased, as legacy's `toLowerCase` keys.
#[must_use]
pub fn table_key(name: &str) -> String {
    name.to_lowercase()
}

/// The attributes of a `<table>` in a `<rom>` (those [`read_table`] reads;
/// an axis reads the [`CellsDoc`] ones, `type`, `name`, `description` and
/// `logparam`).
const TABLE_ATTRIBUTES: &[&str] = &[
    "name",
    "type",
    "base",
    "omit",
    "category",
    "description",
    "storageaddress",
    "storagetype",
    "endian",
    "mask",
    "dataLayout",
    "beforeram",
    "sizex",
    "sizey",
    "flipx",
    "flipy",
    "swapxy",
    "skipCells",
    "userlevel",
    "locked",
    "logparam",
];

/// The child elements of a `<table>` in a `<rom>`.
const TABLE_CHILDREN: &[&str] = &["table", "scaling", "data", "description", "state", "bit"];

/// The attributes of an axis `<table>`.
const AXIS_ATTRIBUTES: &[&str] = &[
    "type",
    "name",
    "description",
    "logparam",
    "storageaddress",
    "storagetype",
    "endian",
    "mask",
    "dataLayout",
    "beforeram",
    "sizex",
    "sizey",
    "flipx",
    "flipy",
];

/// What reading one `<table>` of a `<rom>` found.
pub(super) struct ReadTable {
    /// The table's key ([`table_key`]).
    pub(super) key: String,
    /// The table's name, as written.
    pub(super) name: String,
    /// The patch, or [`Nested::Omit`].
    pub(super) state: Nested<TableDoc>,
}

/// Reads one `<table>` element of a `<rom>`. `rom_has_base` says whether
/// the `<rom>` has a `base`; `earlier` is what the `<rom>` already says
/// about a table of this name. `None` (with an error) for a table without
/// a name.
pub(super) fn read_table(
    ctx: &mut Ctx<'_>,
    el: &Element,
    rom_has_base: bool,
    earlier: impl FnOnce(&str) -> bool,
) -> Option<ReadTable> {
    let Some(name) = attribute(el, "name")
        .map(Attribute::value)
        .filter(|n| !n.is_empty())
    else {
        ctx.check_attributes(el, TABLE_ATTRIBUTES);
        ctx.error(
            codes::MISSING_TABLE_NAME,
            "<table> has no name; it cannot be read".to_owned(),
            el.span(),
        );
        return None;
    };
    let key = table_key(name);
    let name = name.to_owned();
    if let Some(omit) = attribute(el, "omit")
        && read_bool(ctx, omit, "omit")
    {
        ctx.check_attributes(el, TABLE_ATTRIBUTES);
        let has_content =
            el.attributes().iter().any(|a| {
                !matches!(a.name(), "name" | "omit") && TABLE_ATTRIBUTES.contains(&a.name())
            }) || el.elements().next().is_some();
        if has_content {
            ctx.warning(
                codes::CONTENT_IGNORED,
                format!("table `{name}` is omitted; the rest of its <table> is ignored"),
                el.span(),
            );
        }
        // Ignored, but an element that is not part of the format is still
        // named (MF-23).
        for child in el.elements() {
            if !TABLE_CHILDREN.iter().any(|c| is(child, c)) {
                ctx.unknown_element(child, el.name());
            }
        }
        return Some(ReadTable {
            key,
            name,
            state: Nested::Omit,
        });
    }
    let repeat = earlier(&key);
    let mut doc = TableDoc {
        name: Field::Set(name.clone()),
        ..TableDoc::default()
    };
    let mut sizes = Sizes::default();
    for attr in el.attributes() {
        if attr.value() == "NaN" {
            // Legacy read a `NaN` value as an absent attribute; an unknown
            // attribute is still reported, whatever its value.
            if !TABLE_ATTRIBUTES.contains(&attr.name()) {
                ctx.unknown_attribute(attr, &format!("<{}>", el.name()));
            }
            continue;
        }
        if read_cells_attribute(ctx, attr, &mut doc.cells, &mut sizes) {
            continue;
        }
        let value = attr.value();
        match attr.name() {
            "name" | "omit" => {}
            "type" => doc.kind = read_kind(ctx, attr).map_or(Field::Unset, Field::Set),
            "base" if !value.eq_ignore_ascii_case("none") => {
                doc.base = Field::Set(value.to_owned());
            }
            "base" => {}
            "category" => doc.category = Field::Set(value.to_owned()),
            "description" => doc.description = Field::Set(value.to_owned()),
            "logparam" => doc.log_param = Field::Set(value.to_owned()),
            "userlevel" => {
                if let Some(level) = read_user_level(ctx, attr) {
                    doc.user_level = Field::Set(level);
                }
            }
            "locked" => doc.locked = Field::Set(read_bool(ctx, attr, "locked")),
            "swapxy" => doc.swap_xy = Field::Set(read_bool(ctx, attr, "swapxy")),
            "skipCells" => {
                if let Some(n) = read_count(ctx, attr, 0) {
                    doc.skip_cells = Field::Set(n);
                }
            }
            _ => ctx.unknown_attribute(attr, &format!("<{}>", el.name())),
        }
    }
    doc.size_x = sizes.x.map_or(Field::Unset, Field::Set);
    doc.size_y = sizes.y.map_or(Field::Unset, Field::Set);
    doc.flip_x = sizes.flip_x.map_or(Field::Unset, Field::Set);
    doc.flip_y = sizes.flip_y.map_or(Field::Unset, Field::Set);
    let kind = doc.kind.get().copied();
    if kind.is_none() && !rom_has_base && !repeat && doc.base.is_unset() {
        let says = attribute(el, "type").map(Attribute::value);
        // An unknown type has been reported already.
        if says.is_none_or(|t| t.eq_ignore_ascii_case("none")) {
            ctx.error(
                codes::INVALID_TABLE_TYPE,
                format!(
                    "table `{name}` has no type, and its <rom> has no base it could change \
                     a table of: give it type=\"1D\", \"2D\", \"3D\", \"Switch\" or \
                     \"BitwiseSwitch\""
                ),
                el.span(),
            );
        }
    }
    let mut children = Children::default();
    for node in el.children() {
        let child = match node {
            Node::Element(child) => child,
            Node::Text(t) => {
                let message = format!("text directly inside <{}>; ignored", el.name());
                ctx.warning(codes::UNEXPECTED_TEXT, message, t.span());
                continue;
            }
        };
        if is(child, "table") {
            read_axis_child(ctx, child, kind, &sizes, &mut doc, &mut children);
        } else if is(child, "scaling") {
            let what = format!("table `{name}`");
            children
                .scalings
                .read(ctx, child, &what, &mut doc.cells.scalings);
        } else if is(child, "data") {
            let (text, _) = ctx.value_text(child, &[]);
            if kind.is_some_and(TableKind::has_axes) {
                ctx.warning(
                    codes::CONTENT_IGNORED,
                    format!(
                        "<{}> (a static value) on a {} table; ignored, as legacy RomRaider did: \
                         only an axis or a 1D table has static values",
                        child.name(),
                        kind.map_or("", TableKind::name)
                    ),
                    child.span(),
                );
            } else {
                children.data.push(text.into_owned());
            }
        } else if is(child, "description") {
            if children.description {
                ctx.duplicate_element(child, el.name());
            }
            children.description = true;
            let (text, _) = ctx.value_text(child, &[]);
            doc.description = Field::Set(text.into_owned());
        } else if is(child, "state") {
            read_state(ctx, child, &mut doc.states);
        } else if is(child, "bit") {
            if let Some(bit) = read_bit(ctx, child) {
                children.bits.push(bit);
            }
        } else {
            ctx.unknown_element(child, el.name());
        }
    }
    if !children.data.is_empty() {
        doc.cells.static_values = Field::Set(children.data);
    }
    if !children.bits.is_empty() {
        doc.bits = Appended::append(children.bits);
    }
    if !rom_has_base && !repeat && doc.base.is_unset() {
        check_axes_present(ctx, el, &name, kind, &doc);
    }
    Some(ReadTable {
        key,
        name,
        state: Nested::Patch(doc),
    })
}

/// The size and flip attributes of one element, as written; legacy's
/// one-value readings ([`CellsDoc::size`], [`CellsDoc::flip`]) prefer `y`.
#[derive(Debug, Default)]
struct Sizes {
    x: Option<u32>,
    y: Option<u32>,
    flip_x: Option<bool>,
    flip_y: Option<bool>,
}

/// What a `<table>`'s children said, gathered while they are read.
#[derive(Debug, Default)]
struct Children {
    data: Vec<String>,
    bits: Vec<BitFlag>,
    description: bool,
    scalings: ScalingsRead,
    axes: usize,
    x_axes: usize,
    y_axes: usize,
}

/// Reads `attr` into `cells` (and `sizes`) if it is one of the attributes
/// tables and axes share; `false` if it is not.
fn read_cells_attribute(
    ctx: &mut Ctx<'_>,
    attr: &Attribute,
    cells: &mut CellsDoc,
    sizes: &mut Sizes,
) -> bool {
    let value = attr.value();
    match attr.name() {
        "storageaddress" => match parse_hex_int(value).and_then(|a| u32::try_from(a).ok()) {
            Some(address) => cells.storage_address = Field::Set(address),
            None => ctx.error(
                codes::INVALID_ADDRESS,
                format!("storageaddress {value:?} is not a hexadecimal address (0 to FFFFFFFF)"),
                attr.value_span(),
            ),
        },
        "storagetype" => {
            if let Some(storage) = typed(ctx, attr, parse_storage_type(value)) {
                cells.storage = Field::Set(storage);
            }
        }
        "endian" => {
            if let Some(endian) = typed(ctx, attr, parse_endian(value)) {
                cells.endian = Field::Set(endian);
            }
        }
        "mask" => {
            let bitfield = parse_mask(value).and_then(|mask| {
                mask.map(|mask| BitField::new(mask, None, false))
                    .transpose()
            });
            // An empty mask changes nothing, as in legacy (`maps/Table.java:538`).
            if let Some(Some(bitfield)) = typed(ctx, attr, bitfield) {
                cells.bitfield = Field::Set(bitfield);
            }
        }
        "dataLayout" => {
            if let Some(layout) = typed(ctx, attr, parse_data_layout(value)) {
                cells.layout = Field::Set(layout);
            }
        }
        "beforeram" => {
            // Legacy set it only for `true`, any case (`:145-148`).
            if read_bool(ctx, attr, "beforeram") {
                cells.before_ram = Field::Set(true);
            }
        }
        "sizex" => sizes.x = read_count(ctx, attr, 1),
        "sizey" => sizes.y = read_count(ctx, attr, 1),
        "flipx" => sizes.flip_x = Some(read_bool(ctx, attr, "flipx")),
        "flipy" => sizes.flip_y = Some(read_bool(ctx, attr, "flipy")),
        _ => return false,
    }
    if let Some(size) = sizes.y.or(sizes.x) {
        cells.size = Field::Set(size);
    }
    if let Some(flip) = sizes.flip_y.or(sizes.flip_x) {
        cells.flip = Field::Set(flip);
    }
    true
}

/// `result`'s value, or `None` with its error reported at `attr`'s value.
fn typed<T>(ctx: &mut Ctx<'_>, attr: &Attribute, result: Result<T, EncodingError>) -> Option<T> {
    result
        .map_err(|e| {
            let provenance = ctx.provenance(attr.value_span());
            ctx.diags.push(e.to_diagnostic(provenance));
        })
        .ok()
}

/// A yes/no attribute: `true` or `false`, any case; anything else is
/// `false` with a warning, as legacy's `Boolean.parseBoolean`.
fn read_bool(ctx: &mut Ctx<'_>, attr: &Attribute, what: &str) -> bool {
    parse_bool(attr.value()).unwrap_or_else(|| {
        ctx.warning(
            codes::INVALID_BOOLEAN,
            format!(
                "{what} {:?} is neither true nor false; read as false",
                attr.value()
            ),
            attr.value_span(),
        );
        false
    })
}

/// A decimal count of at least `min`; `None` with an error otherwise.
fn read_count(ctx: &mut Ctx<'_>, attr: &Attribute, min: u32) -> Option<u32> {
    let count = parse_decimal_int(attr.value())
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n >= min);
    if count.is_none() {
        ctx.error(
            codes::INVALID_NUMBER,
            format!(
                "{} {:?} is not a whole number from {min} (decimal digits)",
                attr.name(),
                attr.value()
            ),
            attr.value_span(),
        );
    }
    count
}

/// A `userlevel`: a decimal integer, clamped to 1 to 5 with a warning.
fn read_user_level(ctx: &mut Ctx<'_>, attr: &Attribute) -> Option<u8> {
    let Some(level) = parse_decimal_int(attr.value()) else {
        ctx.error(
            codes::INVALID_NUMBER,
            format!(
                "userlevel {:?} is not a whole number (decimal digits)",
                attr.value()
            ),
            attr.value_span(),
        );
        return None;
    };
    let clamped = level.clamp(1, 5);
    if clamped != level {
        ctx.warning(
            codes::USER_LEVEL_OUT_OF_RANGE,
            format!("userlevel {level} is outside 1 to 5; read as {clamped}"),
            attr.value_span(),
        );
    }
    u8::try_from(clamped).ok()
}

/// A table's own `type`: `None` for `none` (legacy's "unspecified", as if
/// absent), or for an unknown type, reported.
fn read_kind(ctx: &mut Ctx<'_>, attr: &Attribute) -> Option<TableKind> {
    let value = attr.value();
    if value.eq_ignore_ascii_case("none") {
        return None;
    }
    let kind = parse_table_type(value);
    if kind.is_none() {
        ctx.error(
            codes::INVALID_TABLE_TYPE,
            format!(
                "table type {value:?} is not 1D, 2D, 3D, Switch or BitwiseSwitch (or an axis \
                 type, read as 1D)"
            ),
            attr.value_span(),
        );
    }
    kind
}

/// A table's own `type`, compared ignoring ASCII case
/// (`xml/TableScaleUnmarshaller.java:106-127`).
fn parse_table_type(text: &str) -> Option<TableKind> {
    let is = |name: &str| text.eq_ignore_ascii_case(name);
    if is("1D") || parse_axis_type(text).is_some() {
        Some(TableKind::OneD)
    } else if is("2D") {
        Some(TableKind::TwoD)
    } else if is("3D") {
        Some(TableKind::ThreeD)
    } else if is("Switch") {
        Some(TableKind::Switch)
    } else if is("BitwiseSwitch") {
        Some(TableKind::BitwiseSwitch)
    } else {
        None
    }
}

/// Which axis an axis `type` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Letter {
    X,
    Y,
}

/// An axis `type`, compared ignoring ASCII case: `X Axis` or
/// `Static X Axis`, `Y Axis` or `Static Y Axis`
/// (`xml/RomAttributeParser.java:92-103`, without the dead `4` and `5`).
fn parse_axis_type(text: &str) -> Option<Letter> {
    let is = |name: &str| text.eq_ignore_ascii_case(name);
    if is("X Axis") || is("Static X Axis") {
        Some(Letter::X)
    } else if is("Y Axis") || is("Static Y Axis") {
        Some(Letter::Y)
    } else {
        None
    }
}

/// Reads a child `<table>` of a table of `kind` (unknown for a change to an
/// inherited table) as an axis, into `doc`.
fn read_axis_child(
    ctx: &mut Ctx<'_>,
    el: &Element,
    kind: Option<TableKind>,
    parent: &Sizes,
    doc: &mut TableDoc,
    children: &mut Children,
) {
    let type_attr = attribute(el, "type");
    let Some(letter) = type_attr.and_then(|t| parse_axis_type(t.value())) else {
        let what = type_attr.map_or_else(
            || "has no type".to_owned(),
            |t| format!("has type {:?}", t.value()),
        );
        ctx.warning(
            codes::CONTENT_IGNORED,
            format!(
                "<{}> inside a table {what}, not an axis type (X Axis, Y Axis, Static X Axis, \
                 Static Y Axis); ignored, as legacy RomRaider did",
                el.name()
            ),
            el.span(),
        );
        ignored_axis_unknowns(ctx, el);
        return;
    };
    if let Some(kind) = kind.filter(|k| !k.has_axes()) {
        ctx.warning(
            codes::CONTENT_IGNORED,
            format!(
                "an axis on a {} table, which has none; ignored, as legacy RomRaider did",
                kind.name()
            ),
            el.span(),
        );
        ignored_axis_unknowns(ctx, el);
        return;
    }
    let along = match (kind, letter) {
        (Some(TableKind::TwoD), _) => parent.y.or(parent.x),
        (Some(TableKind::ThreeD), Letter::X) => parent.x,
        (Some(TableKind::ThreeD), Letter::Y) => parent.y,
        _ => None,
    };
    let axis = read_axis(ctx, el, along);
    let (slot, count) = match letter {
        Letter::X => (&mut doc.x_axis, &mut children.x_axes),
        Letter::Y => (&mut doc.y_axis, &mut children.y_axes),
    };
    *count += 1;
    if kind == Some(TableKind::TwoD) && children.axes > 0 {
        ctx.warning(
            codes::DUPLICATE_ELEMENT,
            "a 2D table has one axis, and this is its second: both are read into it, this \
             later one's values winning, as legacy RomRaider did"
                .to_owned(),
            el.span(),
        );
    } else if kind == Some(TableKind::ThreeD) && *count > 1 {
        ctx.warning(
            codes::DUPLICATE_ELEMENT,
            "this axis appears more than once in its 3D table; both are read into it, this \
             later one's values winning, as legacy RomRaider did"
                .to_owned(),
            el.span(),
        );
    }
    children.axes += 1;
    slot.merge_from(&Nested::Patch(axis.clone()));
    doc.axis.merge_from(&Nested::Patch(axis));
}

/// The child elements of an axis `<table>`.
const AXIS_CHILDREN: &[&str] = &["scaling", "data", "description", "table"];

/// Names the attributes and child elements of an axis `<table>` the reader
/// ignores that are not part of the format: ignored with it, but not
/// dropped in silence (MF-23), as for an omitted table.
fn ignored_axis_unknowns(ctx: &mut Ctx<'_>, el: &Element) {
    let what = format!("an axis <{}>", el.name());
    ctx.check_attributes_as(el, AXIS_ATTRIBUTES, &what);
    for child in el.elements() {
        if !AXIS_CHILDREN.iter().any(|c| is(child, c)) {
            ctx.unknown_element_in(child, &what);
        }
    }
}

/// Reads an axis `<table>`; `along` is its table's size along it, where
/// that element says it.
fn read_axis(ctx: &mut Ctx<'_>, el: &Element, along: Option<u32>) -> AxisDoc {
    let mut axis = AxisDoc::default();
    let mut sizes = Sizes::default();
    for attr in el.attributes() {
        if attr.value() == "NaN" {
            // Absent, as on a table; an unknown attribute is still reported.
            if !AXIS_ATTRIBUTES.contains(&attr.name()) {
                ctx.unknown_attribute(attr, &format!("an axis <{}>", el.name()));
            }
            continue;
        }
        if read_cells_attribute(ctx, attr, &mut axis.cells, &mut sizes) {
            continue;
        }
        let value = attr.value();
        match attr.name() {
            "type" => {}
            "name" => axis.name = Field::Set(value.to_owned()),
            "description" => axis.description = Field::Set(value.to_owned()),
            "logparam" => axis.log_param = Field::Set(value.to_owned()),
            _ => ctx.unknown_attribute(attr, &format!("an axis <{}>", el.name())),
        }
    }
    let mut data = Vec::new();
    let mut first_data = None;
    let mut described = false;
    let mut scalings = ScalingsRead::default();
    for node in el.children() {
        let child = match node {
            Node::Element(child) => child,
            Node::Text(t) => {
                let message = format!("text directly inside an axis <{}>; ignored", el.name());
                ctx.warning(codes::UNEXPECTED_TEXT, message, t.span());
                continue;
            }
        };
        if is(child, "scaling") {
            scalings.read(ctx, child, "this axis", &mut axis.cells.scalings);
        } else if is(child, "data") {
            let (text, _) = ctx.value_text(child, &[]);
            first_data.get_or_insert(child.span());
            data.push(text.into_owned());
        } else if is(child, "description") {
            if described {
                ctx.duplicate_element(child, el.name());
            }
            described = true;
            let (text, _) = ctx.value_text(child, &[]);
            axis.description = Field::Set(text.into_owned());
        } else if is(child, "table") {
            ctx.warning(
                codes::CONTENT_IGNORED,
                format!(
                    "<{}> inside an axis, which has no axes; ignored, as legacy RomRaider did",
                    child.name()
                ),
                child.span(),
            );
            ignored_axis_unknowns(ctx, child);
        } else {
            ctx.unknown_element_in(child, &format!("an axis <{}>", el.name()));
        }
    }
    if let Some(at) = first_data {
        check_static_size(ctx, at, data.len(), sizes.y.or(sizes.x), along);
        axis.cells.static_values = Field::Set(data);
    }
    axis
}

/// Warns when a static axis's `count` values differ from its own size, or
/// else from its table's size along it.
fn check_static_size(
    ctx: &mut Ctx<'_>,
    at: Span,
    count: usize,
    own: Option<u32>,
    along: Option<u32>,
) {
    let differs = |size: u32| usize::try_from(size).is_ok_and(|s| s != count);
    let message = if let Some(own) = own.filter(|&s| differs(s)) {
        format!("this static axis has {count} <data> value(s) but its size is {own}")
    } else if let Some(along) = along.filter(|&s| differs(s)) {
        format!(
            "this static axis has {count} <data> value(s) but its table has {along} cell(s) \
             along it"
        )
    } else {
        return;
    };
    ctx.warning(codes::STATIC_AXIS_SIZE, message, at);
}

/// Reads a `<state name data>` into `states`.
fn read_state(ctx: &mut Ctx<'_>, el: &Element, states: &mut Keyed<String, SwitchState>) {
    ctx.attributes_only(el, &["name", "data"]);
    // Legacy's defaults: an empty name, and data `0`.
    let name = attribute(el, "name").map_or("", Attribute::value);
    let data = match attribute(el, "data") {
        None => vec![0],
        Some(attr) => {
            let Some(data) = parse_state_data(attr.value()) else {
                ctx.error(
                    codes::INVALID_STATE_DATA,
                    format!(
                        "state data {:?} is not hexadecimal values separated by spaces or \
                         commas",
                        attr.value()
                    ),
                    attr.value_span(),
                );
                return;
            };
            data
        }
    };
    let key = name.to_lowercase();
    if states.get(&key).is_some() {
        ctx.warning(
            codes::DUPLICATE_ELEMENT,
            format!(
                "state {name:?} appears more than once in its table; this later one wins, as \
                 in legacy RomRaider"
            ),
            el.span(),
        );
    }
    // A later state of the same name replaces the earlier one whole
    // (`maps/PresetManager.java:120-135`).
    let state = SwitchState {
        name: Field::Set(name.to_owned()),
        data: Field::Set(data),
    };
    states.patch(key, state);
}

/// Reads a `<bit position name>`; `None` without a position (legacy
/// skipped it, `maps/TableBitwiseSwitch.java:71`, here with a warning) or
/// with a bad one (an error).
fn read_bit(ctx: &mut Ctx<'_>, el: &Element) -> Option<BitFlag> {
    ctx.attributes_only(el, &["position", "name"]);
    let name = attribute(el, "name").map_or("", Attribute::value);
    let Some(position) = attribute(el, "position").filter(|p| !p.value().is_empty()) else {
        ctx.warning(
            codes::CONTENT_IGNORED,
            format!(
                "<{}> without a position; ignored, as legacy RomRaider did",
                el.name()
            ),
            el.span(),
        );
        return None;
    };
    let bit = parse_decimal_int(position.value())
        .and_then(|p| u8::try_from(p).ok())
        .filter(|&p| p < 32);
    if bit.is_none() {
        ctx.error(
            codes::INVALID_NUMBER,
            format!(
                "bit position {:?} is not a bit number from 0 to 31",
                position.value()
            ),
            position.value_span(),
        );
    }
    Some(BitFlag {
        name: name.to_owned(),
        position: bit?,
    })
}

/// Reports the axes a new table of `kind` lacks: one for 2D, X and Y for
/// 3D.
fn check_axes_present(
    ctx: &mut Ctx<'_>,
    el: &Element,
    name: &str,
    kind: Option<TableKind>,
    doc: &TableDoc,
) {
    let missing = match kind {
        Some(TableKind::TwoD) if doc.axis.is_unset() => "its axis",
        Some(TableKind::ThreeD) => match (doc.x_axis.is_unset(), doc.y_axis.is_unset()) {
            (true, true) => "its X Axis and its Y Axis",
            (true, false) => "its X Axis",
            (false, true) => "its Y Axis",
            (false, false) => return,
        },
        _ => return,
    };
    ctx.error(
        codes::MISSING_AXIS,
        format!(
            "{} table `{name}` has no {missing}, and its <rom> has no base to inherit one from",
            kind.map_or("", TableKind::name)
        ),
        el.span(),
    );
}

#[cfg(test)]
mod tests;
