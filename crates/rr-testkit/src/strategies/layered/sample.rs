//! A representative partial document for the Layered + Resolver tests: a ROM
//! definition's tables, the shape `xml/TableScaleUnmarshaller.java`
//! unmarshals.
//!
//! [`Rom`] holds every merge policy: replace ([`Field`]: the definition id,
//! each table's storage type, byte order and address), keyed lists
//! ([`Keyed`]: tables by name, each table's axes by name), deep-merge
//! ([`Nested`]: a table's or axis's scaling) and append ([`Appended`]:
//! notes). Signedness is carried by the storage type, a `Field<Storage>`
//! overridden like any other field (MF-06).
//!
//! Each partial type implements [`Complete`] into its finished `*Def`;
//! required fields are the storage type, byte order and address of a table,
//! the storage type, address and length of an axis, a scaling's expression
//! and the definition's id. A missing one is reported at its path
//! (`tables[Fuel Map].axes[X].address`).
//!
//! Strategies: [`rom`], [`table`], [`axis`] and [`scaling`] generate partial
//! documents (any field state); [`complete_rom`] and the other `complete_*`
//! generate ones whose required fields are all set at every level, so they
//! [complete](Complete) on their own. Key and value domains are small, so
//! layers hit each other's tables, axes and values.

use proptest::prelude::*;
use proptest::sample::select;
use rr_core::encoding::{Endian, Storage};
use rr_core::layer::{Appended, Complete, Field, Keyed, Missing, Nested};

use super::{FieldWeights, appended, field, field_with, keyed, nested};

// ---------------------------------------------------------------------------
// Partial documents
// ---------------------------------------------------------------------------

/// How a table's or axis's raw values become physical ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Scaling {
    /// Physical units (`rpm`, `kPa`).
    pub units: Field<String>,
    /// The raw-to-physical expression. Required.
    pub expr: Field<String>,
    /// Display format (`0.00`).
    pub format: Field<String>,
}
rr_core::merge_fields!(Scaling {
    units,
    expr,
    format
});

/// One axis of a table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Axis {
    /// Storage type (width and signedness). Required.
    pub storage: Field<Storage>,
    /// Address. Required.
    pub address: Field<u32>,
    /// Number of elements. Required.
    pub len: Field<u16>,
    /// Scaling.
    pub scaling: Nested<Scaling>,
}
rr_core::merge_fields!(Axis {
    storage,
    address,
    len,
    scaling
});

/// One table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Table {
    /// Storage type (width and signedness). Required.
    pub storage: Field<Storage>,
    /// Byte order. Required.
    pub endian: Field<Endian>,
    /// Address. Required.
    pub address: Field<u32>,
    /// Scaling.
    pub scaling: Nested<Scaling>,
    /// Axes, by name.
    pub axes: Keyed<String, Axis>,
}
rr_core::merge_fields!(Table {
    storage,
    endian,
    address,
    scaling,
    axes
});

/// A ROM definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Rom {
    /// The definition's id (`xmlid`). Required.
    pub xmlid: Field<String>,
    /// Tables, by name.
    pub tables: Keyed<String, Table>,
    /// Free-form notes, each layer's after its bases'.
    pub notes: Appended<String>,
}
rr_core::merge_fields!(Rom {
    xmlid,
    tables,
    notes
});

// ---------------------------------------------------------------------------
// Finished values
// ---------------------------------------------------------------------------

/// A completed [`Scaling`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScalingDef {
    /// Units, if any.
    pub units: Option<String>,
    /// Expression.
    pub expr: String,
    /// Format, if any.
    pub format: Option<String>,
}

/// A completed [`Axis`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AxisDef {
    /// Storage type.
    pub storage: Storage,
    /// Address.
    pub address: u32,
    /// Number of elements.
    pub len: u16,
    /// Scaling, if present.
    pub scaling: Option<ScalingDef>,
}

/// A completed [`Table`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TableDef {
    /// Storage type.
    pub storage: Storage,
    /// Byte order.
    pub endian: Endian,
    /// Address.
    pub address: u32,
    /// Scaling, if present.
    pub scaling: Option<ScalingDef>,
    /// The present axes, in list order.
    pub axes: Vec<(String, AxisDef)>,
}

/// A completed [`Rom`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RomDef {
    /// The definition's id.
    pub xmlid: String,
    /// The present tables, in list order.
    pub tables: Vec<(String, TableDef)>,
    /// Every note, in order.
    pub notes: Vec<String>,
}

/// `field` cloned out, if set.
fn optional<T: Clone>(field: &Field<T>) -> Option<T> {
    field.get().cloned()
}

/// Completes the scaling at `at` (path prefix, ending in `.` unless empty).
fn scaling_def(s: &Scaling, at: &str, missing: &mut Missing) -> Option<ScalingDef> {
    let expr = missing.require(&format!("{at}expr"), &s.expr)?;
    Some(ScalingDef {
        units: optional(&s.units),
        expr: expr.clone(),
        format: optional(&s.format),
    })
}

/// Completes the nested scaling of the item at `at`: `Some(None)` if
/// absent, `None` if present but incomplete.
fn nested_scaling(
    s: &Nested<Scaling>,
    at: &str,
    missing: &mut Missing,
) -> Option<Option<ScalingDef>> {
    match s.get() {
        None => Some(None),
        Some(s) => scaling_def(s, &format!("{at}scaling."), missing).map(Some),
    }
}

fn axis_def(a: &Axis, at: &str, missing: &mut Missing) -> Option<AxisDef> {
    let storage = missing
        .require(&format!("{at}storage"), &a.storage)
        .copied();
    let address = missing
        .require(&format!("{at}address"), &a.address)
        .copied();
    let len = missing.require(&format!("{at}len"), &a.len).copied();
    let scaling = nested_scaling(&a.scaling, at, missing);
    Some(AxisDef {
        storage: storage?,
        address: address?,
        len: len?,
        scaling: scaling?,
    })
}

fn table_def(t: &Table, at: &str, missing: &mut Missing) -> Option<TableDef> {
    let storage = missing
        .require(&format!("{at}storage"), &t.storage)
        .copied();
    let endian = missing.require(&format!("{at}endian"), &t.endian).copied();
    let address = missing
        .require(&format!("{at}address"), &t.address)
        .copied();
    let scaling = nested_scaling(&t.scaling, at, missing);
    let mut axes = Vec::new();
    let mut axes_ok = true;
    for (name, a) in t.axes.present() {
        match axis_def(a, &format!("{at}axes[{name}]."), missing) {
            Some(def) => axes.push((name.clone(), def)),
            None => axes_ok = false,
        }
    }
    let def = TableDef {
        storage: storage?,
        endian: endian?,
        address: address?,
        scaling: scaling?,
        axes,
    };
    axes_ok.then_some(def)
}

/// The value if nothing was recorded missing.
fn finish<T>(value: Option<T>, missing: Missing) -> Result<T, Missing> {
    match value {
        Some(value) if missing.is_empty() => Ok(value),
        _ => Err(missing),
    }
}

impl Complete for Scaling {
    type Output = ScalingDef;

    fn complete(&self) -> Result<ScalingDef, Missing> {
        let mut missing = Missing::new();
        let def = scaling_def(self, "", &mut missing);
        finish(def, missing)
    }
}

impl Complete for Axis {
    type Output = AxisDef;

    fn complete(&self) -> Result<AxisDef, Missing> {
        let mut missing = Missing::new();
        let def = axis_def(self, "", &mut missing);
        finish(def, missing)
    }
}

impl Complete for Table {
    type Output = TableDef;

    fn complete(&self) -> Result<TableDef, Missing> {
        let mut missing = Missing::new();
        let def = table_def(self, "", &mut missing);
        finish(def, missing)
    }
}

impl Complete for Rom {
    type Output = RomDef;

    /// Reports every missing field of every present table, not just the
    /// first.
    fn complete(&self) -> Result<RomDef, Missing> {
        let mut missing = Missing::new();
        let xmlid = missing.require("xmlid", &self.xmlid).cloned();
        let mut tables = Vec::new();
        for (name, t) in self.tables.present() {
            if let Some(def) = table_def(t, &format!("tables[{name}]."), &mut missing) {
                tables.push((name.clone(), def));
            }
        }
        let def = xmlid.map(|xmlid| RomDef {
            xmlid,
            tables,
            notes: self.notes.items().to_vec(),
        });
        finish(def, missing)
    }
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Table names: the keys of [`Rom::tables`].
pub const TABLE_NAMES: [&str; 5] = [
    "Fuel Map",
    "Timing",
    "Boost Target",
    "Idle Speed",
    "Rev Limit",
];

/// Axis names: the keys of [`Table::axes`].
pub const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

/// One of `values`, as a `String`.
fn text(values: &'static [&'static str]) -> impl Strategy<Value = String> + Clone {
    select(values).prop_map(str::to_owned)
}

/// A storage type: any of [`Storage::ALL`], so every signedness switch is
/// reachable.
pub fn storage() -> impl Strategy<Value = Storage> + Clone {
    select(Storage::ALL.to_vec())
}

/// A byte order.
pub fn endian() -> impl Strategy<Value = Endian> + Clone {
    select(vec![Endian::Big, Endian::Little])
}

/// An address: a multiple of `0x100` below `0x800`.
pub fn address() -> impl Strategy<Value = u32> + Clone {
    (0..8_u32).prop_map(|a| a * 0x100)
}

fn scaling_with(required: FieldWeights) -> BoxedStrategy<Scaling> {
    (
        field(text(&["rpm", "kPa", "°C", "%", "ms"])),
        field_with(text(&["x", "x*0.1", "x/4-40", "(x-128)*0.5"]), required),
        field(text(&["0", "0.0", "0.00"])),
    )
        .prop_map(|(units, expr, format)| Scaling {
            units,
            expr,
            format,
        })
        .boxed()
}

fn axis_with(required: FieldWeights) -> BoxedStrategy<Axis> {
    (
        field_with(storage(), required),
        field_with(address(), required),
        field_with(1..=16_u16, required),
        nested(scaling_with(required)),
    )
        .prop_map(|(storage, address, len, scaling)| Axis {
            storage,
            address,
            len,
            scaling,
        })
        .boxed()
}

fn table_with(required: FieldWeights) -> BoxedStrategy<Table> {
    (
        field_with(storage(), required),
        field_with(endian(), required),
        field_with(address(), required),
        nested(scaling_with(required)),
        keyed(text(&AXIS_NAMES), axis_with(required), 0..=2),
    )
        .prop_map(|(storage, endian, address, scaling, axes)| Table {
            storage,
            endian,
            address,
            scaling,
            axes,
        })
        .boxed()
}

fn rom_with(required: FieldWeights) -> BoxedStrategy<Rom> {
    (
        field_with(text(&["A2WC522N", "16BITBASE", "32BITBASE"]), required),
        keyed(text(&TABLE_NAMES), table_with(required), 0..=4),
        appended(text(&["base", "fix", "user"]), 0..=2),
    )
        .prop_map(|(xmlid, tables, notes)| Rom {
            xmlid,
            tables,
            notes,
        })
        .boxed()
}

/// A partial [`Scaling`]: every field in any state.
pub fn scaling() -> BoxedStrategy<Scaling> {
    scaling_with(FieldWeights::MIXED)
}

/// A partial [`Axis`].
pub fn axis() -> BoxedStrategy<Axis> {
    axis_with(FieldWeights::MIXED)
}

/// A partial [`Table`]: up to two axes.
pub fn table() -> BoxedStrategy<Table> {
    table_with(FieldWeights::MIXED)
}

/// A partial [`Rom`]: up to four table entries, up to two notes.
pub fn rom() -> BoxedStrategy<Rom> {
    rom_with(FieldWeights::MIXED)
}

/// A [`Scaling`] that completes on its own.
pub fn complete_scaling() -> BoxedStrategy<Scaling> {
    scaling_with(FieldWeights::SET)
}

/// An [`Axis`] that completes on its own.
pub fn complete_axis() -> BoxedStrategy<Axis> {
    axis_with(FieldWeights::SET)
}

/// A [`Table`] that completes on its own: every required field set, in it
/// and in each present axis and scaling.
pub fn complete_table() -> BoxedStrategy<Table> {
    table_with(FieldWeights::SET)
}

/// A [`Rom`] that completes on its own.
pub fn complete_rom() -> BoxedStrategy<Rom> {
    rom_with(FieldWeights::SET)
}
