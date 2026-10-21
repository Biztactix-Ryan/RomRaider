//! Definition sets: many RomRaider ECU-definition files loaded together,
//! each `<rom>` layered on its `base` chain (C1) and resolved into a
//! [`ResolvedRom`] whose tables have a typed [`Encoding`] and
//! [`Conversion`](rr_core::conversion::Conversion) (US-RR-29-8).
//!
//! See [`DefinitionSet`] for the model.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use rr_core::conversion::ToRawMismatch;
use rr_core::diag::{Diagnostic, Diagnostics, Loaded, Provenance, Span, codes};
use rr_core::encoding::{Encoding, Endian};
use rr_core::layer::{
    Document, FieldPath, Keyed, Layer, LayerId, LayerKind, Lineage, Merge as _, Name, Registry,
    ResolveError, Resolver, Scope,
};

use super::{
    AxisDoc, CellFormat, CellsDoc, DefinitionFile, Identification, RomDoc, RomEntry, RomIdDoc,
    ScalingDoc, TableDoc, TableKind, build_cells, read, scaling_base_key, select_scaling,
    table_key, to_raw_mismatch,
};

#[cfg(test)]
mod tests;

/// The key an `xmlid` (or a `base` naming one) is compared by: upper-cased,
/// as legacy compared names ignoring case. Also the registry id of the
/// first definition of the name (see [`DefinitionSet`]).
#[must_use]
pub fn xmlid_key(xmlid: &str) -> String {
    xmlid.to_uppercase()
}

/// The path of the table named `name` (any case) in a [`RomDoc`], for
/// [`Lineage::explain`]: `tables["boost target"]`. Extend it with
/// [`FieldPath::field`] to a field of the table.
#[must_use]
pub fn table_path(name: &str) -> FieldPath<RomDoc, TableDoc> {
    FieldPath::root()
        .field("tables", |r: &RomDoc| &r.tables)
        .item(table_key(name))
}

/// A `<rom>` of a [`DefinitionSet`]: which file, and which `<rom>` of it
/// ([`DefinitionFile::roms`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RomRef {
    file: usize,
    rom: usize,
}

impl RomRef {
    /// The file's index in [`DefinitionSet::files`].
    #[must_use]
    pub const fn file(self) -> usize {
        self.file
    }

    /// The `<rom>`'s index in that file's [`DefinitionFile::roms`].
    #[must_use]
    pub const fn index(self) -> usize {
        self.rom
    }
}

/// RomRaider ECU-definition files loaded together, their `<rom>`s indexed
/// by `xmlid` with every `base` found.
///
/// # Model
///
/// A [`DefinitionSet`] holds the files a user loads together (legacy's list
/// of ECU definition files, in its order). Each named `<rom>` of each file
/// is a [`Document`] in one [`Registry`] of rr-core's resolver, its layer
/// the `<rom>` element, its one base the `<rom>` its `base` names; a
/// [`RomResolver`] linearises the chain with rr-core's [`Resolver`]
/// (cycle detection, an unknown base, the depth limit: `LAY` codes) and
/// merges it, field by field ([`RomDoc`]): tables merge by name, a child's
/// attribute overriding only that attribute; `omit="true"` removes the one
/// inherited table it names and nothing else (MF-07); every field is
/// inherited, signedness included (MF-06).
///
/// # Which `base` a name finds
///
/// An `xmlid` is compared ignoring case, as legacy compared it
/// (`equalsIgnoreCase`, `xml/DOMRomUnmarshaller.java:127`): the set folds
/// every `xmlid` and `base` with [`xmlid_key`]. A `base` is looked up:
///
/// 1. in the `<rom>`'s own file, the first `<rom>` of that name, as legacy
///    looked it up (`getBaseRom` searches the same document,
///    `xml/DOMRomUnmarshaller.java:149-152`, `findRomNodeMatch` takes the
///    first match, `:97-138`);
/// 2. else in the other files, the first file in set order that defines it
///    (MF-17: legacy could not reach another file at all).
///
/// A `<rom>` whose `base` is its own `xmlid` skips itself, so it extends the
/// definition of that name in another file, as a local document naming
/// its own id extends the outer one in rr-core's resolver; legacy found
/// itself and recursed until the stack overflowed. No `<rom>` of that name
/// anywhere else, and every other name no file defines, is an
/// [`UNKNOWN_BASE`](codes::UNKNOWN_BASE) Error at the `base` attribute. A
/// chain that loops is an [`INHERITANCE_CYCLE`](codes::INHERITANCE_CYCLE)
/// Error naming every definition on it.
///
/// Looking in the own file first is what lets RomRaider's own files load
/// together: each per-ROM file of the pinned corpus carries its own copy of
/// `32BITBASE` (about 250 copies), and legacy read each file on its own.
/// So two `<rom>`s of one name are not the hard
/// [`DUPLICATE_DOCUMENT`](codes::DUPLICATE_DOCUMENT) Error the resolver
/// reports for a name it cannot tell apart, but a
/// [`DUPLICATE_XMLID`](codes::DUPLICATE_XMLID) Warning naming both places:
///
/// * in one file, the first is used and the later one ignored (legacy's
///   first match);
/// * in different files, both are kept, each serving its own file; the one
///   in the first file is what other files and [`DefinitionSet::find`] get
///   (legacy took the first definition file that matched,
///   `editor/ecu/OpenImageWorker.java:219-250`).
///
/// In the registry, the first definition of a name is the layer id
/// [`xmlid_key`] (`32BITBASE`); a later file's own copy is
/// `32BITBASE@<its file>`, which is what provenance shows for it.
///
/// # Resolving one ROM
///
/// [`RomResolver::resolve`] merges the chain, then, for each table present:
///
/// * a table `base` (another table of the ROM it starts as a copy of,
///   `xml/TableScaleUnmarshaller.java:87-92`) is resolved with rr-core's
///   resolver over the ROM's merged tables: the named table, merged, under
///   this one (MF-18: shared, not a serialization deep copy). A `base`
///   naming the table itself changes nothing (legacy copied the table it
///   already had);
/// * a table with neither a `storageaddress` nor static `<data>` in any
///   layer is not on this ROM: it is listed in [`ResolvedRom::unplaced`],
///   not resolved, as legacy dropped such a table when it populated the
///   ROM (`maps/Rom.java:303-321`; MF-23: there by removing it from the map
///   while iterating over it);
/// * a placed table needs a kind: one that no layer types (usually a child
///   changing a table its base does not define) is left out with an
///   [`UNTYPED_TABLE`](codes::UNTYPED_TABLE) Warning at it, as legacy
///   logged "Table type unspecified" and skipped it;
/// * its axes and storage are required: a table without them after the
///   merge is an Error at the table ([`MISSING_AXIS`](codes::MISSING_AXIS),
///   [`MISSING_STORAGE_TYPE`](codes::MISSING_STORAGE_TYPE), the `ENC` and
///   `CNV` codes of [`build_cells`]), and that table alone is left out;
///   every other table resolves (MF-23: legacy caught the
///   `NullPointerException` such a table caused and dropped it);
/// * the scaling shown is chosen with [`select_scaling`] (the
///   [preferred category](RomResolver::with_preferred_scaling), else
///   `Default`) and its `base` resolved against the `<scalingbase>`s of the
///   files of the chain, strongest first, then of the set
///   ([`UNKNOWN_SCALING`](codes::UNKNOWN_SCALING) when none has it);
/// * the cells' [`Encoding`] and conversion are built with [`build_cells`]
///   (big-endian unless the table or the ROM's `<memmodel endian>` says
///   otherwise; a switch table's storage defaults to `uint8`), and a
///   `to_byte` is checked against the `expression` once per distinct
///   (formulas, encoding) ([`to_raw_mismatch`], `CNV0005`).
///
/// A table's diagnostics point at the `<table>` element of the strongest
/// definition that mentions it, naming that definition's layer.
///
/// # Provenance
///
/// [`ResolvedRom::lineage`] is the stack every value came from; its
/// [`explain`](Lineage::explain) says which definition, in which file, set
/// a field and what it overrode, for a path such as
/// [`table_path`]`("Boost Target").field("cells", ..)`.
#[derive(Debug, Clone)]
pub struct DefinitionSet {
    files: Vec<Arc<DefinitionFile>>,
    /// The registry id of each `<rom>`, per file; `None` for one ignored as
    /// a repeat of an earlier `<rom>` of its file.
    ids: Vec<Vec<Option<LayerId>>>,
    by_id: BTreeMap<LayerId, RomRef>,
    /// The registered `<rom>`s of each [`xmlid_key`], in set order.
    by_key: BTreeMap<String, Vec<RomRef>>,
    roms: Registry<RomDoc>,
    /// Each file's `<scalingbase>`s, in the scope [`file_scope`].
    scaling_bases: Registry<ScalingDoc>,
}

/// The scope of the `<scalingbase>`s of the file `index` (whose source is
/// `source`), in [`DefinitionSet::scaling_bases`].
fn file_scope(index: usize, source: &str) -> Scope {
    Scope::global().child(format!("{index}:{source}"))
}

/// `source:line:col`.
fn at(source: &str, span: Span) -> String {
    format!("{source}{span}")
}

impl DefinitionSet {
    /// The set of `files`, in that order (the order decides which of two
    /// files defining one name other files get; see the
    /// [type docs](Self)).
    ///
    /// Diagnostics: a [`DUPLICATE_XMLID`](codes::DUPLICATE_XMLID) Warning per
    /// repeated name; a [`DUPLICATE_DOCUMENT`](codes::DUPLICATE_DOCUMENT)
    /// Error only for two `<scalingbase>` scopes, or two later copies of a
    /// name, that collide because two files have one source name.
    pub fn new<F: Into<Arc<DefinitionFile>>>(files: impl IntoIterator<Item = F>) -> Loaded<Self> {
        let files: Vec<Arc<DefinitionFile>> = files.into_iter().map(Into::into).collect();
        let mut diags = Diagnostics::new();
        let mut ids = Vec::with_capacity(files.len());
        let mut by_id = BTreeMap::new();
        let mut by_key: BTreeMap<String, Vec<RomRef>> = BTreeMap::new();
        for (fi, file) in files.iter().enumerate() {
            let mut file_ids = Vec::with_capacity(file.roms().len());
            let mut first_here: BTreeMap<String, &RomEntry> = BTreeMap::new();
            for (ri, rom) in file.roms().iter().enumerate() {
                let key = xmlid_key(rom.xmlid());
                let provenance = Provenance::new(file.source(), rom.span());
                if let Some(kept) = first_here.get(&key) {
                    diags.warning(
                        codes::DUPLICATE_XMLID,
                        format!(
                            "`{}` is defined again in this file (first at {}); this later \
                             <rom> is ignored, as legacy RomRaider used the first",
                            rom.xmlid(),
                            at(file.source(), kept.span())
                        ),
                        provenance,
                    );
                    file_ids.push(None);
                    continue;
                }
                first_here.insert(key.clone(), rom);
                let refs = by_key.entry(key.clone()).or_default();
                let id = match refs.first().and_then(|r| Some((r, files.get(r.file)?))) {
                    None => LayerId::from(key.as_str()),
                    Some((first, first_file)) => {
                        let kept = first_file
                            .roms()
                            .get(first.rom)
                            .map_or(Span::line_col(1, 1), RomEntry::span);
                        diags.warning(
                            codes::DUPLICATE_XMLID,
                            format!(
                                "`{}` is also defined in {}; a `base` in this file means this \
                                 one, a `base` in a file without one, and a lookup by name, \
                                 the first",
                                rom.xmlid(),
                                at(first_file.source(), kept)
                            ),
                            provenance,
                        );
                        let mut id = format!("{key}@{}", file.source());
                        if by_id.contains_key(&LayerId::from(id.as_str())) {
                            id = format!("{id}#{fi}");
                        }
                        LayerId::from(id)
                    }
                };
                let me = RomRef { file: fi, rom: ri };
                refs.push(me);
                by_id.insert(id.clone(), me);
                file_ids.push(Some(id));
            }
            ids.push(file_ids);
        }
        let mut set = Self {
            files,
            ids,
            by_id,
            by_key,
            roms: Registry::new(),
            scaling_bases: Registry::new(),
        };
        set.register(&mut diags);
        diags.finish(set)
    }

    /// Fills the registries once every `<rom>` has its id.
    fn register(&mut self, diags: &mut Diagnostics) {
        let mut roms = Registry::new();
        let mut scaling_bases = Registry::new();
        for (fi, file) in self.files.iter().enumerate() {
            for (ri, rom) in file.roms().iter().enumerate() {
                let Some(id) = self.id(RomRef { file: fi, rom: ri }) else {
                    continue;
                };
                let layer = Layer::new(id.clone(), LayerKind::File, file.source(), rom.span());
                let mut doc = Document::new(layer, Arc::clone(rom.shared_doc()));
                if let Some(base) = rom.base() {
                    doc = doc.with_base(self.base_id(RomRef { file: fi, rom: ri }, base.name()));
                }
                if let Err(e) = roms.insert(doc) {
                    diags.push(e.to_diagnostic());
                }
            }
            let scope = file_scope(fi, file.source());
            for (key, state) in file.scaling_bases().entries() {
                let Some(scaling) = state.get() else { continue };
                let span = file.scaling_base_span(key).unwrap_or(Span::line_col(1, 1));
                let layer = Layer::new(
                    LayerId::from(key.as_str()),
                    LayerKind::File,
                    file.source(),
                    span,
                );
                let mut doc = Document::new(layer, scaling.clone()).in_scope(scope.clone());
                if let Some(base) = scaling.base.get() {
                    doc = doc.with_base(scaling_base_key(base));
                }
                if let Err(e) = scaling_bases.insert(doc) {
                    diags.push(e.to_diagnostic());
                }
            }
        }
        self.roms = roms;
        self.scaling_bases = scaling_bases;
    }

    /// The registry id of the `<rom>` the `base` `name` of `from` finds: in
    /// `from`'s file first, else in the first file defining it; the folded
    /// name itself (which no document has, or `from`'s own id) when no other
    /// `<rom>` has it, for the resolver to report.
    fn base_id(&self, from: RomRef, name: &str) -> LayerId {
        let key = xmlid_key(name);
        let candidates = self.by_key.get(&key).map_or(&[][..], Vec::as_slice);
        candidates
            .iter()
            .find(|r| r.file == from.file && **r != from)
            .or_else(|| candidates.iter().find(|r| **r != from))
            .and_then(|r| self.id(*r).cloned())
            .unwrap_or_else(|| LayerId::from(key))
    }

    /// Reads each `(source, bytes)` as a RomRaider ECU-definition file
    /// ([`read`]) and loads them as a set, in that order. A file that is not
    /// one (not well-formed XML, or another kind of definition) is left
    /// out; every file's diagnostics are kept, so an Error in any file
    /// withholds the set ([`Loaded`]) unless the caller accepts it.
    pub fn read_all<'a>(inputs: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Loaded<Self> {
        let mut diags = Vec::new();
        let mut files = Vec::new();
        for (source, bytes) in inputs {
            let (file, more) = read(source, bytes).accept_errors();
            diags.extend(more);
            files.extend(file);
        }
        let (set, more) = Self::new(files).accept_errors();
        diags.extend(more);
        Loaded::new(set, diags)
    }

    /// The files, in set order.
    #[must_use]
    pub fn files(&self) -> &[Arc<DefinitionFile>] {
        &self.files
    }

    /// The `<rom>` `rom` refers to.
    #[must_use]
    pub fn rom(&self, rom: RomRef) -> Option<&RomEntry> {
        self.files.get(rom.file)?.roms().get(rom.rom)
    }

    /// The registry (and layer) id of `rom`: its [`xmlid_key`] for the first
    /// definition of the name, `KEY@file` for a later file's own copy.
    /// `None` for a `<rom>` ignored as a repeat in its file.
    #[must_use]
    pub fn id(&self, rom: RomRef) -> Option<&LayerId> {
        self.ids.get(rom.file)?.get(rom.rom)?.as_ref()
    }

    /// Every `<rom>` that can be resolved (all but those ignored as repeats
    /// in their file), in set order.
    pub fn roms(&self) -> impl Iterator<Item = (RomRef, &RomEntry)> {
        self.files.iter().enumerate().flat_map(move |(fi, file)| {
            file.roms().iter().enumerate().filter_map(move |(ri, rom)| {
                let r = RomRef { file: fi, rom: ri };
                self.id(r).map(|_| (r, rom))
            })
        })
    }

    /// The definition `xmlid` (any case) names from outside the files: the
    /// first file's that defines it.
    #[must_use]
    pub fn find(&self, xmlid: &str) -> Option<RomRef> {
        self.by_key.get(&xmlid_key(xmlid))?.first().copied()
    }

    /// The first `<rom>`, in set order, whose own identification matches
    /// `image`, as legacy picked the definition of an image it opened
    /// (`editor/ecu/OpenImageWorker.java:219-250`).
    #[must_use]
    pub fn identify(&self, image: &[u8]) -> Option<RomRef> {
        self.roms()
            .find(|(_, rom)| rom.identification().is_some_and(|i| i.matches(image)))
            .map(|(r, _)| r)
    }

    /// The registry of every `<rom>` of the set, for rr-core's resolver.
    #[must_use]
    pub const fn registry(&self) -> &Registry<RomDoc> {
        &self.roms
    }

    /// A resolver over the set, with an empty cache: a base shared by many
    /// definitions is merged once per resolver.
    #[must_use]
    pub fn resolver(&self) -> RomResolver<'_> {
        RomResolver {
            set: self,
            roms: self.roms.resolver(),
            scalings: self.scaling_bases.resolver(),
            preferred: None,
            checked: HashMap::new(),
        }
    }
}

/// One ROM definition resolved: its whole `base` chain merged, and every
/// table on the ROM typed.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRom {
    rom: RomRef,
    xmlid: String,
    lineage: Arc<Lineage<RomDoc>>,
    identification: Option<Identification>,
    tables: Vec<ResolvedTable>,
    unplaced: Vec<String>,
}

impl ResolvedRom {
    /// Which `<rom>` this is.
    #[must_use]
    pub const fn rom(&self) -> RomRef {
        self.rom
    }

    /// Its `xmlid`, as written.
    #[must_use]
    pub fn xmlid(&self) -> &str {
        &self.xmlid
    }

    /// The chain it was merged from, weakest first, for
    /// [`explain`](Lineage::explain).
    #[must_use]
    pub const fn lineage(&self) -> &Arc<Lineage<RomDoc>> {
        &self.lineage
    }

    /// The merged document: every layer of the chain applied.
    #[must_use]
    pub fn doc(&self) -> &RomDoc {
        self.lineage.merged()
    }

    /// The merged `<romid>`.
    #[must_use]
    pub fn romid(&self) -> &RomIdDoc {
        &self.doc().romid
    }

    /// How an image is recognised as this ROM: from its own first
    /// `<romid>`, never an inherited one, as legacy.
    #[must_use]
    pub const fn identification(&self) -> Option<&Identification> {
        self.identification.as_ref()
    }

    /// The tables on the ROM, in the merged order (inherited tables first,
    /// in their base's order; new ones after), without those that had an
    /// Error.
    #[must_use]
    pub fn tables(&self) -> &[ResolvedTable] {
        &self.tables
    }

    /// The table named `name` (any case).
    #[must_use]
    pub fn table(&self, name: &str) -> Option<&ResolvedTable> {
        let key = table_key(name);
        self.tables.iter().find(|t| t.key == key)
    }

    /// The names of the tables present in the merged document that are not
    /// on this ROM: no layer gives a `storageaddress` (or static values).
    #[must_use]
    pub fn unplaced(&self) -> &[String] {
        &self.unplaced
    }
}

/// One table of a [`ResolvedRom`]: merged, its kind, axes and cells typed.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTable {
    /// The key ([`table_key`]).
    pub key: String,
    /// The name, as the strongest layer wrote it.
    pub name: String,
    /// The kind.
    pub kind: TableKind,
    /// The merged document (a table `base` applied).
    pub doc: TableDoc,
    /// The `<table>` element of the strongest definition that mentions the
    /// table, with that definition's layer.
    pub at: Provenance,
    /// How the cells are stored and converted; `None` for a 1D table of
    /// static values.
    pub cells: Option<CellFormat>,
    /// The scaling shown, its `base` applied; `None` for raw values.
    pub scaling: Option<ScalingDoc>,
    /// The axes: none for a 1D or switch table, one for 2D, X then Y for
    /// 3D.
    pub axes: Vec<ResolvedAxis>,
}

/// One axis of a [`ResolvedTable`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAxis {
    /// The merged axis.
    pub doc: AxisDoc,
    /// How its values are stored and converted; `None` for a static axis.
    pub cells: Option<CellFormat>,
    /// The scaling shown, its `base` applied; `None` for raw values.
    pub scaling: Option<ScalingDoc>,
}

/// The formulas and encoding a `to_byte` check depends on.
type CheckKey = (String, String, Encoding);

/// Resolves the `<rom>`s of a [`DefinitionSet`] ([`DefinitionSet::resolver`]).
#[derive(Debug)]
pub struct RomResolver<'s> {
    set: &'s DefinitionSet,
    roms: Resolver<'s, RomDoc>,
    scalings: Resolver<'s, ScalingDoc>,
    preferred: Option<String>,
    checked: HashMap<CheckKey, Option<ToRawMismatch>>,
}

impl RomResolver<'_> {
    /// Shows the scaling of `category` (legacy's "default scale" setting,
    /// such as `Metric`) where a table has one, else its `Default` one
    /// ([`select_scaling`]). Unset, every table shows its `Default` scaling.
    #[must_use]
    pub fn with_preferred_scaling(mut self, category: impl Into<String>) -> Self {
        self.preferred = Some(category.into());
        self
    }

    /// Resolves `rom`. `None` with an Error when its chain cannot be built
    /// (an unknown base, a cycle), and `None` with no diagnostic for a `rom`
    /// that is not one of the set's [`roms`](DefinitionSet::roms). A table
    /// with an Error is left out of the value, which is then withheld
    /// unless the caller accepts the errors ([`Loaded`]); a table left out
    /// with a Warning (no type, [`UNTYPED_TABLE`](codes::UNTYPED_TABLE))
    /// does not withhold it.
    pub fn resolve(&mut self, rom: RomRef) -> Loaded<Option<ResolvedRom>> {
        let mut diags = Diagnostics::new();
        let set = self.set;
        let (Some(entry), Some(id)) = (set.rom(rom), set.id(rom)) else {
            return diags.finish(None);
        };
        let lineage = match self.roms.lineage(&Name::global(id.clone())) {
            Ok(lineage) => lineage,
            Err(e) => {
                diags.push(self.chain_error(&e, entry));
                return diags.finish(None);
            }
        };
        let merged = Arc::clone(lineage.merged());
        let endian = merged.romid.mem_model_endian.get().copied();
        let based = self.table_bases(&merged, &lineage, &mut diags);
        let mut tables = Vec::new();
        let mut unplaced = Vec::new();
        for (key, table) in merged.tables.present() {
            let table = based.get(key).unwrap_or(table);
            let name = table.name.get().cloned().unwrap_or_else(|| key.clone());
            let cells = &table.cells;
            if cells.storage_address.is_unset() && cells.static_values.get().is_none() {
                unplaced.push(name);
                continue;
            }
            let at = self.table_at(&lineage, key);
            let mut table_diags = Diagnostics::new();
            let resolved = self.table(key, name, table, &lineage, endian, at, &mut table_diags);
            let failed = table_diags.has_errors();
            diags.extend(table_diags);
            if let Some(resolved) = resolved.filter(|_| !failed) {
                tables.push(resolved);
            }
        }
        diags.finish(Some(ResolvedRom {
            rom,
            xmlid: entry.xmlid().to_owned(),
            lineage,
            identification: entry.identification().cloned(),
            tables,
            unplaced,
        }))
    }

    /// The diagnostic for a chain that cannot be built, resolving `entry`.
    /// An unknown base is reported at the `base` attribute that names it.
    fn chain_error(&self, error: &ResolveError, entry: &RomEntry) -> Diagnostic {
        let set = self.set;
        let request = Provenance::new(entry.source(), entry.span());
        let ResolveError::UnknownBase { document, .. } = error else {
            return error.to_diagnostic(request);
        };
        let Some(at) = set.by_id.get(document.id()).and_then(|r| set.rom(*r)) else {
            return error.to_diagnostic(request);
        };
        let Some(base) = at.base() else {
            return error.to_diagnostic(request);
        };
        let mut message = if xmlid_key(base.name()) == xmlid_key(at.xmlid()) {
            format!(
                "`{}` names itself as its base, and no other loaded definition file defines \
                 `{}` for it to extend",
                at.xmlid(),
                base.name()
            )
        } else {
            format!(
                "`{}` names base `{}`, which no loaded definition file defines",
                at.xmlid(),
                base.name()
            )
        };
        if !std::ptr::eq(at, entry) {
            message.push_str(&format!("; `{}` inherits from it", entry.xmlid()));
        }
        let provenance =
            Provenance::new(at.source(), base.span()).with_layer(document.id().as_str());
        Diagnostic::error(error.code(), message, provenance)
    }

    /// The `<table>` element of the strongest layer of `lineage` that
    /// mentions the table `key`, with that layer; the ROM's own `<rom>`
    /// when none does.
    fn table_at(&self, lineage: &Lineage<RomDoc>, key: &str) -> Provenance {
        let set = self.set;
        lineage
            .names()
            .iter()
            .rev()
            .find_map(|name| {
                let entry = set.rom(*set.by_id.get(name.id())?)?;
                let span = entry.table_span(key)?;
                Some(Provenance::new(entry.source(), span).with_layer(name.id().as_str()))
            })
            .unwrap_or_else(|| lineage.layer().provenance())
    }

    /// The tables of `merged` that name a table `base` (other than
    /// themselves), each with that base resolved under it, by key.
    fn table_bases(
        &self,
        merged: &RomDoc,
        lineage: &Lineage<RomDoc>,
        diags: &mut Diagnostics,
    ) -> BTreeMap<String, TableDoc> {
        let has_base =
            |(key, t): &(&String, &TableDoc)| t.base.get().is_some_and(|b| table_key(b) != **key);
        if !merged.tables.present().any(|e| has_base(&e)) {
            return BTreeMap::new();
        }
        let mut registry = Registry::new();
        for (key, table) in merged.tables.present() {
            let at = self.table_at(lineage, key);
            let layer = Layer::new(
                LayerId::from(key.as_str()),
                LayerKind::File,
                at.source(),
                at.span(),
            );
            let mut doc = Document::new(layer, table.clone());
            if has_base(&(key, table))
                && let Some(base) = table.base.get()
            {
                doc = doc.with_base(table_key(base));
            }
            if let Err(e) = registry.insert(doc) {
                diags.push(e.to_diagnostic());
            }
        }
        let mut resolver = registry.resolver();
        let mut out = BTreeMap::new();
        for (key, table) in merged.tables.present() {
            if !has_base(&(key, table)) {
                continue;
            }
            match resolver.lineage(&Name::global(key.as_str())) {
                Ok(l) => {
                    out.insert(key.clone(), (**l.merged()).clone());
                }
                Err(e) => {
                    let at = self.table_at(lineage, key);
                    diags.push(e.to_diagnostic(at));
                }
            }
        }
        out
    }

    /// Resolves the placed table `key` (its document `table`, merged), or
    /// `None` with an Error.
    #[expect(clippy::too_many_arguments, reason = "one private step of `resolve`")]
    fn table(
        &mut self,
        key: &str,
        name: String,
        table: &TableDoc,
        lineage: &Lineage<RomDoc>,
        endian: Option<Endian>,
        at: Provenance,
        diags: &mut Diagnostics,
    ) -> Option<ResolvedTable> {
        let Some(&kind) = table.kind.get() else {
            diags.warning(
                codes::UNTYPED_TABLE,
                format!(
                    "table `{name}` has an address but no type, here or in any definition it \
                     inherits from (does its base define a table of this name?); it is not \
                     loaded, as legacy RomRaider skipped it"
                ),
                at,
            );
            return None;
        };
        let scaling = self.scaling(&table.cells.scalings, lineage, &at, diags);
        let is_static = kind == TableKind::OneD && table.cells.static_values.get().is_some();
        let cells = if is_static {
            None
        } else {
            self.cells(
                &table.cells,
                Some(kind),
                scaling.as_ref(),
                endian,
                &at,
                diags,
            )
        };
        let axes: Vec<&AxisDoc> = match kind {
            TableKind::TwoD => vec![self.axis_of(&table.axis, &name, "2D", "axis", &at, diags)?],
            TableKind::ThreeD => {
                let x = self.axis_of(&table.x_axis, &name, "3D", "X Axis", &at, diags);
                let y = self.axis_of(&table.y_axis, &name, "3D", "Y Axis", &at, diags);
                vec![x?, y?]
            }
            TableKind::OneD | TableKind::Switch | TableKind::BitwiseSwitch => Vec::new(),
        };
        let axes = axes
            .into_iter()
            .map(|axis| {
                let scaling = self.scaling(&axis.cells.scalings, lineage, &at, diags);
                let cells = if axis.cells.static_values.get().is_some() {
                    None
                } else {
                    self.cells(&axis.cells, None, scaling.as_ref(), endian, &at, diags)
                };
                ResolvedAxis {
                    doc: axis.clone(),
                    cells,
                    scaling,
                }
            })
            .collect();
        Some(ResolvedTable {
            key: key.to_owned(),
            name,
            kind,
            doc: table.clone(),
            at,
            cells,
            scaling,
            axes,
        })
    }

    /// The axis in `slot`, or `None` with a
    /// [`MISSING_AXIS`](codes::MISSING_AXIS) Error.
    fn axis_of<'d>(
        &self,
        slot: &'d rr_core::layer::Nested<AxisDoc>,
        name: &str,
        kind: &str,
        which: &str,
        at: &Provenance,
        diags: &mut Diagnostics,
    ) -> Option<&'d AxisDoc> {
        let axis = slot.get();
        if axis.is_none() {
            diags.error(
                codes::MISSING_AXIS,
                format!(
                    "{kind} table `{name}` has no {which}, here or in any definition it \
                     inherits from"
                ),
                at.clone(),
            );
        }
        axis
    }

    /// [`build_cells`], then the `to_byte` check of `scaling` against the
    /// result, once per distinct (formulas, encoding).
    fn cells(
        &mut self,
        cells: &CellsDoc,
        kind: Option<TableKind>,
        scaling: Option<&ScalingDoc>,
        endian: Option<Endian>,
        at: &Provenance,
        diags: &mut Diagnostics,
    ) -> Option<CellFormat> {
        let (built, more) = build_cells(cells, kind, scaling, endian, at).accept_errors();
        diags.extend(more);
        let built = built?;
        if let Some(scaling) = scaling
            && let Some(Some(to_raw)) = scaling.to_raw.get()
        {
            let to_real = scaling.to_real.get().map_or("x", |f| f.text());
            let key = (to_real.to_owned(), to_raw.text().to_owned(), built.encoding);
            let mismatch = *self
                .checked
                .entry(key)
                .or_insert_with(|| to_raw_mismatch(&built));
            if let Some(m) = mismatch {
                diags.push(m.to_diagnostic(at.clone()));
            }
        }
        Some(built)
    }

    /// The scaling of `scalings` shown ([`select_scaling`]), its `base`
    /// resolved; `None` for raw values, or (with an Error) for a `base` that
    /// cannot be resolved.
    fn scaling(
        &mut self,
        scalings: &Keyed<String, ScalingDoc>,
        lineage: &Lineage<RomDoc>,
        at: &Provenance,
        diags: &mut Diagnostics,
    ) -> Option<ScalingDoc> {
        let chosen = select_scaling(scalings, self.preferred.as_deref())?;
        let Some(base) = chosen.base.get() else {
            return Some(chosen.clone());
        };
        let set = self.set;
        let key = scaling_base_key(base);
        let chain = lineage
            .names()
            .iter()
            .rev()
            .filter_map(|n| set.by_id.get(n.id()).map(|r| r.file));
        let file = chain.chain(0..set.files.len()).find(|&f| {
            set.files.get(f).is_some_and(|file| {
                file.scaling_bases()
                    .get(&key)
                    .is_some_and(|s| s.get().is_some())
            })
        });
        let Some((fi, file)) = file.and_then(|f| Some((f, set.files.get(f)?))) else {
            diags.error(
                codes::UNKNOWN_SCALING,
                format!(
                    "a scaling names base `{base}`, which no <scalingbase> of the loaded \
                     definition files defines"
                ),
                at.clone(),
            );
            return None;
        };
        let name = Name::new(file_scope(fi, file.source()), key.as_str());
        match self.scalings.lineage(&name) {
            Ok(l) => Some(l.merged().merged(chosen)),
            Err(e) => {
                diags.push(e.to_diagnostic(at.clone()));
                None
            }
        }
    }
}
