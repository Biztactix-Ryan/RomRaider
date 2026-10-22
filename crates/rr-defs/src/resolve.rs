//! Frozen definitions (US-RR-34-6): a resolved ROM definition as one
//! immutable, typed [`Definition`], its parts shared between the
//! definitions of one set rather than copied (MF-18).
//!
//! A [`RomResolver`] merges a `<rom>`'s `base`
//! chain (C1) into a [`ResolvedRom`]: the merged partial documents, every
//! field still a [`Field`](rr_core::layer::Field) that may be unset. A
//! [`DefinitionResolver`] takes it one step further, into what the editor
//! and the size check (US-RR-34-8) read:
//!
//! | Part | Type | From |
//! |------|------|------|
//! | what the ROM is, how an image is recognised as it, its declared size | [`RomId`] | the merged `<romid>`, the `<rom>`'s own [`Identification`] |
//! | the tables on the ROM, typed | [`Table`], each with its [`Cells`] and [`Axis`]es | [`ResolvedRom::tables`] |
//! | the scalings they show | [`Scaling`] | each table's and axis's chosen scaling, its `base` applied |
//! | the checksum spec to fix the image with | [`ChecksumSpecRef`], a name only | `<checksummodule>` |
//!
//! Which definition an image is comes first: the [`RomIdIndex`]
//! (US-RR-34-7), built once per set, finds the `<rom>` whose ROM ID the
//! image holds, and refuses an image more than one matches (EC-03).
//!
//! Every default legacy applied to an absent attribute is applied here
//! (a `userlevel` of 0, `locked` false, the `Default` scaling category), so
//! a reader of a [`Definition`] never meets an unset field; what legacy had
//! no default for stays an [`Option`].
//!
//! # Frozen
//!
//! A [`Definition`] and everything in it is plain data: every field is
//! private behind a `&self` getter, nothing holds a lock, cell or atomic,
//! and the whole is `Send + Sync`. Changing a ROM's values is the image's
//! business (rr-rom's patch journal), never the definition's.
//!
//! Where that is enforced (US-RR-34-5), for every resolved type
//! ([`Definition`], [`RomId`], [`ChecksumSpecRef`], [`Table`], [`Axis`],
//! [`Cells`], [`CellSource`], [`CellFormat`], [`Scaling`], [`SwitchState`],
//! [`RomIdIndex`], [`Candidate`], [`RomMatch`]):
//!
//! 1. **Compile time, through every field.** A `const` assertion in this
//!    module requires each to be `Send + Sync + UnwindSafe + RefUnwindSafe`;
//!    the compiler derives those auto traits from every field, rr-core's
//!    embedded types (`Encoding`, `Conversion`, `Expr`, `BytePattern`,
//!    `Provenance`, ...) included, so a field that breaks one fails the build
//!    of this crate. That rules out `Cell`, `RefCell`, `OnceCell`,
//!    `LazyCell` and `UnsafeCell` (not `Sync`; `UnsafeCell` is not
//!    `RefUnwindSafe`, so no cell behind a hand-written `unsafe impl Sync`
//!    either) and `Rc`. It does **not** rule out `Mutex`, `RwLock`,
//!    `OnceLock`, `LazyLock` or an atomic: std declares all of them `Sync`
//!    and `RefUnwindSafe`, and stable Rust has no `Freeze` bound to say
//!    "no interior mutability" directly. A hand-written `impl
//!    RefUnwindSafe` (a safe trait) would also defeat it.
//! 2. **Compile time, from outside the crate.** `tests/frozen_definition.rs`
//!    (trybuild, pinned `.stderr`): no public field, no `&mut` getter, no
//!    `IndexMut`; the getters lend `&` only, so nothing can be written even
//!    through a `&mut Definition`, and a shared `Arc<Table>` is never lent
//!    `&mut`. It also pins which interior-mutability types the bound of 1
//!    rejects and which it lets through.
//! 3. **Review, by `cargo xtask check-globals` in CI.** This module and the
//!    modules defining every type a resolved type embeds (rr-defs'
//!    `romraider`; rr-core's `diag`, `encoding`, `conversion`, `expr`,
//!    `pattern`) are stateless modules (`xtask/src/globals.rs`
//!    `STATELESS_PATHS`): naming a lock, cell, atomic or once/lazy type
//!    anywhere in them fails, which closes the gap 1 leaves
//!    (`xtask/tests/globals.rs` plants a `Mutex`, `Cell`, `RwLock`, atomic
//!    and `OnceLock` field and checks each is rejected). Both files are
//!    code-owned (`.github/CODEOWNERS`), so weakening the rule is reviewed.
//!
//! Replacing a whole value the caller owns (`def = other`) is not a change
//! to a definition anyone else holds, and `unsafe` code is outside any
//! type-level guarantee.
//!
//! # Shared, not copied
//!
//! Legacy started a table with a `base` as a Java-serialization deep copy
//! of the ROM's object graph, and every child definition carried its own
//! copy of what it inherited (MF-18). Here the parts of a definition are
//! behind [`Arc`]s that one [`DefinitionResolver`] hands out once per
//! distinct value: a [`Table`] no child changes, a [`Scaling`] and a
//! [`CellFormat`] (with its parsed formulas) are one allocation however
//! many definitions of the set have them. Two children of one base share
//! every table neither changes ([`Arc::ptr_eq`]). Sharing is by value:
//! a part is shared only with an equal one, so a [`Definition`] is the same
//! value whichever definitions were resolved before it, and with which
//! resolver.
//!
//! # Diagnostics
//!
//! Freezing adds none: every problem is the [`RomResolver`]'s,
//! passed through unchanged (an unknown base or a cycle withholds the
//! definition; a table with an Error is left out and withholds it unless
//! the caller accepts the errors, [`Loaded`]).
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use rr_defs::resolve::DefinitionResolver;
//! use rr_defs::romraider::DefinitionSet;
//!
//! let xml = br#"<roms>
//!   <rom>
//!     <romid><xmlid>BASE</xmlid><filesize>192kb</filesize></romid>
//!     <table type="1D" name="Rev Limit" storagetype="uint16" storageaddress="100">
//!       <scaling units="rpm" expression="x*4"/>
//!     </table>
//!   </rom>
//!   <rom base="BASE">
//!     <romid><xmlid>ROM A</xmlid><internalidaddress>200</internalidaddress>
//!       <internalidstring>ROMA</internalidstring></romid>
//!   </rom>
//!   <rom base="BASE">
//!     <romid><xmlid>ROM B</xmlid><internalidaddress>200</internalidaddress>
//!       <internalidstring>ROMB</internalidstring></romid>
//!   </rom>
//! </roms>"#;
//! let (set, _) = DefinitionSet::read_all([("defs.xml", &xml[..])]).into_result().unwrap();
//! let mut resolver = DefinitionResolver::new(&set);
//! let a = resolver.resolve(set.find("rom a").unwrap()).into_result().unwrap().0.unwrap();
//! let b = resolver.resolve(set.find("rom b").unwrap()).into_result().unwrap().0.unwrap();
//!
//! assert_eq!(a.romid().file_size(), Some(192 * 1024));
//! let limit = a.table("rev limit").unwrap();
//! assert_eq!(limit.cells().address(), Some(0x100));
//! // Both inherit the table unchanged: one allocation.
//! assert!(Arc::ptr_eq(limit, b.table("Rev Limit").unwrap()));
//! ```

pub mod index;
#[cfg(test)]
mod tests;

pub use index::{Candidate, RomIdIndex, RomMatch};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;

use rr_core::conversion::DisplayFormat;
use rr_core::diag::{Loaded, Provenance};
use rr_core::encoding::{Encoding, Endian};

use crate::romraider::{
    AxisDoc, BitFlag, CellFormat, CellsDoc, DEFAULT_CATEGORY, DefinitionSet, Formula,
    Identification, RamOffset, ResolvedAxis, ResolvedRom, ResolvedTable, RomIdDoc, RomRef,
    RomResolver, ScalingDoc, TableKind, table_key,
};

// US-RR-34-5, compile time: every resolved type is `Send + Sync +
// UnwindSafe + RefUnwindSafe`, which the compiler checks through every
// field, the rr-core types embedded included (the module docs' "Frozen"
// says what that rules out and what it does not). A field that breaks it
// fails the build of this crate, not only its tests. Keep the bound in step
// with tests/ui/frozen_definition*/.
const _: () = {
    const fn frozen<T: Send + Sync + UnwindSafe + RefUnwindSafe>() {}
    frozen::<Definition>();
    frozen::<RomId>();
    frozen::<ChecksumSpecRef>();
    frozen::<Table>();
    frozen::<Axis>();
    frozen::<Cells>();
    frozen::<CellSource>();
    frozen::<CellFormat>();
    frozen::<Scaling>();
    frozen::<SwitchState>();
    frozen::<RomIdIndex>();
    frozen::<Candidate>();
    frozen::<RomMatch<'static>>();
};

/// One ROM definition, resolved and frozen: what the ROM is, its tables and
/// their scalings, and the checksum spec it names. See the
/// [module docs](self).
///
/// Cloning one is cheap: the tables and scalings are shared ([`Arc`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    romid: RomId,
    checksum: Option<ChecksumSpecRef>,
    tables: Vec<Arc<Table>>,
    /// The index in `tables` of each [`table_key`].
    by_key: BTreeMap<String, usize>,
    scalings: Vec<Arc<Scaling>>,
    unplaced: Vec<String>,
}

impl Definition {
    /// The definition's name: its `<rom>`'s own `<xmlid>`, as written.
    #[must_use]
    pub fn xmlid(&self) -> &str {
        self.romid.xmlid()
    }

    /// What the ROM is, how an image is recognised as it, and how big it is.
    #[must_use]
    pub const fn romid(&self) -> &RomId {
        &self.romid
    }

    /// The checksum spec the definition names, if any.
    #[must_use]
    pub const fn checksum(&self) -> Option<&ChecksumSpecRef> {
        self.checksum.as_ref()
    }

    /// The tables on the ROM, in the merged order (inherited tables first,
    /// in their base's order; new ones after), without those the resolver
    /// left out (an Error, or no type).
    #[must_use]
    pub fn tables(&self) -> &[Arc<Table>] {
        &self.tables
    }

    /// The table named `name`, compared ignoring case ([`table_key`]).
    #[must_use]
    pub fn table(&self, name: &str) -> Option<&Arc<Table>> {
        self.tables.get(*self.by_key.get(&table_key(name))?)
    }

    /// Every distinct scaling a table or axis of the definition shows, in
    /// the order the tables first show them.
    #[must_use]
    pub fn scalings(&self) -> &[Arc<Scaling>] {
        &self.scalings
    }

    /// The names of the tables the definition has but does not place on
    /// the ROM (no layer gives an address or static values), as
    /// [`ResolvedRom::unplaced`].
    #[must_use]
    pub fn unplaced(&self) -> &[String] {
        &self.unplaced
    }
}

/// What a [`Definition`] says about its ROM: the merged `<romid>`
/// ([`RomIdDoc`], whose docs give each element's legacy meaning), typed,
/// with the `<rom>`'s own [`Identification`].
#[derive(Debug, Clone, PartialEq)]
pub struct RomId {
    xmlid: String,
    identification: Option<Identification>,
    internal_id_hex: Option<String>,
    file_size: Option<u32>,
    ram_offset: Option<RamOffset>,
    mem_model: Option<String>,
    mem_model_endian: Option<Endian>,
    obsolete: bool,
    ecu_id: Option<String>,
    case_id: Option<String>,
    make: Option<String>,
    market: Option<String>,
    model: Option<String>,
    submodel: Option<String>,
    transmission: Option<String>,
    year: Option<String>,
    flash_method: Option<String>,
    author: Option<String>,
    version: Option<String>,
}

impl RomId {
    /// The `<rom>`'s own `<xmlid>`, as written.
    #[must_use]
    pub fn xmlid(&self) -> &str {
        &self.xmlid
    }

    /// How an image is recognised as this ROM: the `<rom>`'s own first
    /// `<romid>`, never an inherited one, as legacy matched it. Its
    /// [`Identification::Bytes`] is the (address, bytes) pattern (C2) the
    /// ROM-ID index matches. `None` for a definition that only serves as a
    /// base (no `internalidstring`).
    #[must_use]
    pub const fn identification(&self) -> Option<&Identification> {
        self.identification.as_ref()
    }

    /// `<internalidhex>` (ECUFlash), as written; not part of
    /// identification, as legacy ignored it.
    #[must_use]
    pub fn internal_id_hex(&self) -> Option<&str> {
        self.internal_id_hex.as_deref()
    }

    /// `<filesize>`, in bytes: the image size the definition is for.
    /// `None` when no layer of the chain declares one.
    #[must_use]
    pub const fn file_size(&self) -> Option<u32> {
        self.file_size
    }

    /// How table addresses map to image offsets. `None` is legacy's
    /// default: the image offset of an address is the address minus
    /// ([`file_size`](Self::file_size) minus the image length).
    #[must_use]
    pub const fn ram_offset(&self) -> Option<RamOffset> {
        self.ram_offset
    }

    /// `<memmodel>`: the ECU processor / memory model name.
    #[must_use]
    pub fn mem_model(&self) -> Option<&str> {
        self.mem_model.as_deref()
    }

    /// `<memmodel endian>`: the byte order that overrode every table's own
    /// (already applied to each [`CellFormat`]).
    #[must_use]
    pub const fn mem_model_endian(&self) -> Option<Endian> {
        self.mem_model_endian
    }

    /// `<obsolete>`: a newer revision of the ROM exists. Not inherited
    /// (see [`RomIdDoc::obsolete`]); `false` when unset.
    #[must_use]
    pub const fn obsolete(&self) -> bool {
        self.obsolete
    }

    /// `<ecuid>`: the ECU ID the ECU reports.
    #[must_use]
    pub fn ecu_id(&self) -> Option<&str> {
        self.ecu_id.as_deref()
    }

    /// `<caseid>`: the ECU hardware part number.
    #[must_use]
    pub fn case_id(&self) -> Option<&str> {
        self.case_id.as_deref()
    }

    /// `<make>`.
    #[must_use]
    pub fn make(&self) -> Option<&str> {
        self.make.as_deref()
    }

    /// `<market>`.
    #[must_use]
    pub fn market(&self) -> Option<&str> {
        self.market.as_deref()
    }

    /// `<model>`.
    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// `<submodel>`.
    #[must_use]
    pub fn submodel(&self) -> Option<&str> {
        self.submodel.as_deref()
    }

    /// `<transmission>`.
    #[must_use]
    pub fn transmission(&self) -> Option<&str> {
        self.transmission.as_deref()
    }

    /// `<year>`.
    #[must_use]
    pub fn year(&self) -> Option<&str> {
        self.year.as_deref()
    }

    /// `<flashmethod>`: the reflash method name.
    #[must_use]
    pub fn flash_method(&self) -> Option<&str> {
        self.flash_method.as_deref()
    }

    /// `<author>`.
    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// `<version>`: the definition's version.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

/// The checksum spec a [`Definition`] names: a name only, resolved to a
/// checksum spec by the checksum engine (US-RR-35), which refuses a name it
/// does not know.
///
/// It is the merged `<checksummodule>` of the `<romid>` (ECUFlash's
/// checksum module name, such as `subarudbw`; legacy ignored it). Legacy's
/// own `<checksum type="...">` element is not read yet (`DEF0006`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChecksumSpecRef {
    name: String,
}

impl ChecksumSpecRef {
    /// The name, as written.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// One table of a [`Definition`]: its base chain and any table `base`
/// merged, every field typed (see [`TableDoc`](crate::romraider::TableDoc)
/// for each attribute's legacy meaning).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    key: String,
    name: String,
    kind: TableKind,
    category: Option<String>,
    description: Option<String>,
    user_level: u8,
    locked: bool,
    log_param: Option<String>,
    cells: Cells,
    size_x: Option<u32>,
    size_y: Option<u32>,
    flip_x: bool,
    flip_y: bool,
    swap_xy: bool,
    skip_cells: u32,
    axes: Vec<Axis>,
    states: Vec<SwitchState>,
    bits: Vec<BitFlag>,
    at: Provenance,
}

impl Table {
    /// The key ([`table_key`]): the name lower-cased.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The name, as the strongest layer wrote it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The kind.
    #[must_use]
    pub const fn kind(&self) -> TableKind {
        self.kind
    }

    /// `category`: the group the table is listed under.
    #[must_use]
    pub fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }

    /// `description`.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// `userlevel`: from 1 (beginner) to 5 (debug); 0 (legacy's default
    /// when unset) shows the table at every level.
    #[must_use]
    pub const fn user_level(&self) -> u8 {
        self.user_level
    }

    /// `locked`: the table cannot be edited. `false` when unset.
    #[must_use]
    pub const fn locked(&self) -> bool {
        self.locked
    }

    /// `logparam`: the logger parameter for live data on the table.
    #[must_use]
    pub fn log_param(&self) -> Option<&str> {
        self.log_param.as_deref()
    }

    /// The table's own cells.
    #[must_use]
    pub const fn cells(&self) -> &Cells {
        &self.cells
    }

    /// `sizex`: a 3D table's column count.
    #[must_use]
    pub const fn size_x(&self) -> Option<u32> {
        self.size_x
    }

    /// `sizey`: a 3D table's row count.
    #[must_use]
    pub const fn size_y(&self) -> Option<u32> {
        self.size_y
    }

    /// `flipx`: a 3D table's rows are stored in reverse.
    #[must_use]
    pub const fn flip_x(&self) -> bool {
        self.flip_x
    }

    /// `flipy`: a 3D table's columns are stored in reverse.
    #[must_use]
    pub const fn flip_y(&self) -> bool {
        self.flip_y
    }

    /// `swapxy`: a 3D table's cells are stored column by column.
    #[must_use]
    pub const fn swap_xy(&self) -> bool {
        self.swap_xy
    }

    /// `skipCells`: cells skipped after each stored row of a 3D table; 0
    /// when unset.
    #[must_use]
    pub const fn skip_cells(&self) -> u32 {
        self.skip_cells
    }

    /// The axes: none for a 1D or switch table, one for 2D, X then Y for
    /// 3D.
    #[must_use]
    pub fn axes(&self) -> &[Axis] {
        &self.axes
    }

    /// A switch table's `<state>`s, in the merged order.
    #[must_use]
    pub fn states(&self) -> &[SwitchState] {
        &self.states
    }

    /// A bitwise switch table's `<bit>`s, inherited ones first.
    #[must_use]
    pub fn bits(&self) -> &[BitFlag] {
        &self.bits
    }

    /// The `<table>` element of the strongest definition that mentions the
    /// table, with that definition's layer: where a diagnostic about the
    /// table points.
    #[must_use]
    pub const fn at(&self) -> &Provenance {
        &self.at
    }
}

/// One axis of a [`Table`].
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    name: Option<String>,
    description: Option<String>,
    log_param: Option<String>,
    cells: Cells,
}

impl Axis {
    /// `name`, as written.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// `description`.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// `logparam`: the logger parameter the axis follows.
    #[must_use]
    pub fn log_param(&self) -> Option<&str> {
        self.log_param.as_deref()
    }

    /// The axis values.
    #[must_use]
    pub const fn cells(&self) -> &Cells {
        &self.cells
    }
}

/// The cells of a [`Table`] or [`Axis`]: where they are and how they are
/// stored and converted, or their static values; how many; and the scaling
/// shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Cells {
    source: CellSource,
    size: Option<u32>,
    flip: bool,
    scaling: Option<Arc<Scaling>>,
}

impl Cells {
    /// Where the values come from.
    #[must_use]
    pub const fn source(&self) -> &CellSource {
        &self.source
    }

    /// The address of the first cell, for cells read from the image.
    #[must_use]
    pub const fn address(&self) -> Option<u32> {
        match &self.source {
            CellSource::Image { address, .. } => *address,
            CellSource::Static(_) => None,
        }
    }

    /// How each cell is stored and converted, for cells read from the
    /// image.
    #[must_use]
    pub fn format(&self) -> Option<&Arc<CellFormat>> {
        match &self.source {
            CellSource::Image { format, .. } => Some(format),
            CellSource::Static(_) => None,
        }
    }

    /// The cell count of a 1D, 2D or switch table or of an axis (`sizey`,
    /// else `sizex`; a 3D table's are [`Table::size_x`] and
    /// [`Table::size_y`]).
    #[must_use]
    pub const fn size(&self) -> Option<u32> {
        self.size
    }

    /// Whether the values run in reverse (`flipy`, else `flipx`). `false`
    /// when unset.
    #[must_use]
    pub const fn flip(&self) -> bool {
        self.flip
    }

    /// The scaling shown; `None` for raw values.
    #[must_use]
    pub const fn scaling(&self) -> Option<&Arc<Scaling>> {
        self.scaling.as_ref()
    }
}

/// Where the values of [`Cells`] come from.
#[derive(Debug, Clone, PartialEq)]
pub enum CellSource {
    /// Read from the image.
    Image {
        /// `storageaddress`: where the first cell is, as a ROM address
        /// ([`RomId::ram_offset`] maps it to an image offset unless
        /// `before_ram`). `None` when no layer gives one (an axis, or a 2D
        /// or 3D table placed by static `<data>` only).
        address: Option<u32>,
        /// `beforeram="true"`: the address is an image offset. `false`
        /// when unset.
        before_ram: bool,
        /// How each cell is stored (C3) and converted (C4).
        format: Arc<CellFormat>,
    },
    /// The `<data>` values of a static axis or 1D table, as written: read
    /// from the definition, not the image.
    Static(Vec<String>),
}

/// The scaling a table or axis shows: a merged `<scaling>`, its `base`
/// applied ([`ScalingDoc`], whose docs give each attribute's legacy
/// meaning). The conversion it makes is the [`CellFormat`]'s.
#[derive(Debug, Clone, PartialEq)]
pub struct Scaling {
    name: Option<String>,
    category: String,
    units: Option<String>,
    to_real: Option<Formula>,
    to_raw: Option<Formula>,
    format: Option<DisplayFormat>,
    min: Option<f64>,
    max: Option<f64>,
    coarse_increment: Option<f64>,
    fine_increment: Option<f64>,
}

impl Scaling {
    /// `name`, as written.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// `category`, as written; [`DEFAULT_CATEGORY`] when unset.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }

    /// `units`, verbatim.
    #[must_use]
    pub fn units(&self) -> Option<&str> {
        self.units.as_deref()
    }

    /// `expression`: raw (`x`) to real. `None` is legacy's default, `x`.
    #[must_use]
    pub const fn to_real(&self) -> Option<&Formula> {
        self.to_real.as_ref()
    }

    /// `to_byte`: real (`x`) to raw, as the definition gives it (checked,
    /// never used to write; see [`rr_core::conversion`]).
    #[must_use]
    pub const fn to_raw(&self) -> Option<&Formula> {
        self.to_raw.as_ref()
    }

    /// `format`: the display pattern, verbatim.
    #[must_use]
    pub const fn format(&self) -> Option<&DisplayFormat> {
        self.format.as_ref()
    }

    /// `min`: the lowest real value, for display.
    #[must_use]
    pub const fn min(&self) -> Option<f64> {
        self.min
    }

    /// `max`: the highest real value, for display.
    #[must_use]
    pub const fn max(&self) -> Option<f64> {
        self.max
    }

    /// `coarseincrement`: the larger editing step.
    #[must_use]
    pub const fn coarse_increment(&self) -> Option<f64> {
        self.coarse_increment
    }

    /// `fineincrement`: the smaller editing step.
    #[must_use]
    pub const fn fine_increment(&self) -> Option<f64> {
        self.fine_increment
    }
}

/// One `<state>` of a switch [`Table`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SwitchState {
    name: String,
    data: Vec<u32>,
}

impl SwitchState {
    /// `name`, as written.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `data`: one value per cell (see
    /// [`SwitchState`](crate::romraider::SwitchState::data) for legacy's
    /// byte order).
    #[must_use]
    pub fn data(&self) -> &[u32] {
        &self.data
    }
}

/// Resolves the `<rom>`s of a [`DefinitionSet`] into [`Definition`]s,
/// sharing every equal part between them (see the [module docs](self)).
///
/// One resolver serves one version of a set: its [`RomResolver`] merges a
/// base shared by many definitions once, and the parts it has handed out
/// are kept (by the resolver value, not a global) to be handed out again.
#[derive(Debug)]
pub struct DefinitionResolver<'s> {
    roms: RomResolver<'s>,
    shared: Shared,
}

/// Parts handed out, by bucket.
type Parts<K, T> = HashMap<K, Vec<Arc<T>>>;

/// The parts a [`DefinitionResolver`] has handed out, each bucketed by a
/// cheap key and handed out again only to an equal value.
#[derive(Debug, Default)]
struct Shared {
    /// By (name, units).
    scalings: Parts<(Option<String>, Option<String>), Scaling>,
    /// By (encoding, address of the shared scaling the conversion was
    /// built from).
    formats: Parts<(Encoding, Option<usize>), CellFormat>,
    /// By (key, where the strongest definition mentioning it has it).
    tables: Parts<(String, Provenance), Table>,
}

/// The part of `bucket` of `map` equal to `value`, else `value`, kept.
fn share<K: Hash + Eq, T: PartialEq>(map: &mut Parts<K, T>, bucket: K, value: T) -> Arc<T> {
    let parts = map.entry(bucket).or_default();
    if let Some(part) = parts.iter().find(|p| ***p == value) {
        return Arc::clone(part);
    }
    let part = Arc::new(value);
    parts.push(Arc::clone(&part));
    part
}

impl<'s> DefinitionResolver<'s> {
    /// A resolver over `set`, sharing nothing yet.
    #[must_use]
    pub fn new(set: &'s DefinitionSet) -> Self {
        Self {
            roms: set.resolver(),
            shared: Shared::default(),
        }
    }

    /// Shows the scaling of `category` where a table has one, else its
    /// `Default` one ([`RomResolver::with_preferred_scaling`]).
    #[must_use]
    pub fn with_preferred_scaling(mut self, category: impl Into<String>) -> Self {
        self.roms = self.roms.with_preferred_scaling(category);
        self
    }

    /// Resolves `rom` ([`RomResolver::resolve`]) and freezes it. The
    /// diagnostics, and when the value is withheld, are the
    /// [`RomResolver`]'s.
    pub fn resolve(&mut self, rom: RomRef) -> Loaded<Option<Definition>> {
        let (resolved, diagnostics) = self.roms.resolve(rom).accept_errors();
        let definition = resolved.map(|r| self.shared.definition(&r));
        Loaded::new(definition, diagnostics)
    }
}

impl Shared {
    /// Freezes `rom`.
    fn definition(&mut self, rom: &ResolvedRom) -> Definition {
        let tables: Vec<Arc<Table>> = rom.tables().iter().map(|t| self.table(t)).collect();
        let by_key = tables
            .iter()
            .enumerate()
            .map(|(i, t)| (t.key.clone(), i))
            .collect();
        let mut seen = HashSet::new();
        let scalings = tables
            .iter()
            .flat_map(|t| {
                let axes = t.axes.iter().map(|a| &a.cells);
                std::iter::once(&t.cells).chain(axes)
            })
            .filter_map(|c| c.scaling.as_ref())
            .filter(|s| seen.insert(Arc::as_ptr(s)))
            .cloned()
            .collect();
        let doc = rom.romid();
        Definition {
            romid: rom_id(rom.xmlid(), rom.identification(), doc),
            checksum: doc
                .checksum_module
                .get()
                .map(|name| ChecksumSpecRef { name: name.clone() }),
            tables,
            by_key,
            scalings,
            unplaced: rom.unplaced().to_vec(),
        }
    }

    /// Freezes the table `t`, shared.
    fn table(&mut self, t: &ResolvedTable) -> Arc<Table> {
        let doc = &t.doc;
        let cells = self.cells(&doc.cells, t.cells.as_ref(), t.scaling.as_ref());
        let axes = t.axes.iter().map(|a| self.axis(a)).collect();
        let states = doc
            .states
            .present()
            .map(|(key, s)| SwitchState {
                name: s.name.get().cloned().unwrap_or_else(|| key.clone()),
                data: s.data.get().cloned().unwrap_or_default(),
            })
            .collect();
        let table = Table {
            key: t.key.clone(),
            name: t.name.clone(),
            kind: t.kind,
            category: doc.category.get().cloned(),
            description: doc.description.get().cloned(),
            user_level: doc.user_level.get().copied().unwrap_or(0),
            locked: doc.locked.get().copied().unwrap_or(false),
            log_param: doc.log_param.get().cloned(),
            cells,
            size_x: doc.size_x.get().copied(),
            size_y: doc.size_y.get().copied(),
            flip_x: doc.flip_x.get().copied().unwrap_or(false),
            flip_y: doc.flip_y.get().copied().unwrap_or(false),
            swap_xy: doc.swap_xy.get().copied().unwrap_or(false),
            skip_cells: doc.skip_cells.get().copied().unwrap_or(0),
            axes,
            states,
            bits: doc.bits.items().to_vec(),
            at: t.at.clone(),
        };
        share(&mut self.tables, (t.key.clone(), t.at.clone()), table)
    }

    /// Freezes the axis `a`.
    fn axis(&mut self, a: &ResolvedAxis) -> Axis {
        let AxisDoc {
            name,
            description,
            log_param,
            cells,
        } = &a.doc;
        Axis {
            name: name.get().cloned(),
            description: description.get().cloned(),
            log_param: log_param.get().cloned(),
            cells: self.cells(cells, a.cells.as_ref(), a.scaling.as_ref()),
        }
    }

    /// Freezes merged cells `doc`, typed as `format` (`None`: static) and
    /// showing `scaling`.
    fn cells(
        &mut self,
        doc: &CellsDoc,
        format: Option<&CellFormat>,
        scaling: Option<&ScalingDoc>,
    ) -> Cells {
        let scaling = scaling.map(|s| self.scaling(s));
        let source = match format {
            Some(format) => {
                let from = scaling.as_ref().map(|s| Arc::as_ptr(s).addr());
                CellSource::Image {
                    address: doc.storage_address.get().copied(),
                    before_ram: doc.before_ram.get().copied().unwrap_or(false),
                    format: share(&mut self.formats, (format.encoding, from), format.clone()),
                }
            }
            None => CellSource::Static(doc.static_values.get().cloned().unwrap_or_default()),
        };
        Cells {
            source,
            size: doc.size.get().copied(),
            flip: doc.flip.get().copied().unwrap_or(false),
            scaling,
        }
    }

    /// Freezes the merged scaling `s`, shared.
    fn scaling(&mut self, s: &ScalingDoc) -> Arc<Scaling> {
        let scaling = Scaling {
            name: s.name.get().cloned(),
            category: s
                .category
                .get()
                .cloned()
                .unwrap_or_else(|| DEFAULT_CATEGORY.to_owned()),
            units: s.units.get().cloned(),
            to_real: s.to_real.get().cloned(),
            to_raw: s.to_raw.get().cloned().flatten(),
            format: s.format.get().cloned(),
            min: s.min.get().copied(),
            max: s.max.get().copied(),
            coarse_increment: s.coarse_increment.get().copied(),
            fine_increment: s.fine_increment.get().copied(),
        };
        let bucket = (scaling.name.clone(), scaling.units.clone());
        share(&mut self.scalings, bucket, scaling)
    }
}

/// The typed [`RomId`] of the `<rom>` `xmlid`, identified by
/// `identification`, whose merged `<romid>` is `doc`.
fn rom_id(xmlid: &str, identification: Option<&Identification>, doc: &RomIdDoc) -> RomId {
    let text = |f: &rr_core::layer::Field<String>| f.get().cloned();
    RomId {
        xmlid: xmlid.to_owned(),
        identification: identification.cloned(),
        internal_id_hex: text(&doc.internal_id_hex),
        file_size: doc.file_size.get().copied(),
        ram_offset: doc.ram_offset.get().copied(),
        mem_model: text(&doc.mem_model),
        mem_model_endian: doc.mem_model_endian.get().copied(),
        obsolete: doc.obsolete.get().copied().unwrap_or(false),
        ecu_id: text(&doc.ecu_id),
        case_id: text(&doc.case_id),
        make: text(&doc.make),
        market: text(&doc.market),
        model: text(&doc.model),
        submodel: text(&doc.submodel),
        transmission: text(&doc.transmission),
        year: text(&doc.year),
        flash_method: text(&doc.flash_method),
        author: text(&doc.author),
        version: text(&doc.version),
    }
}
