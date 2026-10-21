//! `<scaling>` and `<scalingbase>`: how a table's raw values convert to
//! real ones and are shown, read as partial documents (C1) — see
//! [`ScalingDoc`]; and, once the layers are merged, the typed [`Encoding`]
//! and [`Conversion`] of a table's or axis's cells ([`build_cells`]).

use rr_core::conversion::{
    Conversion, ConversionError, DisplayFormat, ExprForm, Form, Linear, ToRawMismatch,
};
use rr_core::diag::{Diagnostic, Diagnostics, Loaded, Provenance, Span, codes};
use rr_core::encoding::{Encoding, Endian, Layout, Storage};
use rr_core::expr::Expr;
use rr_core::layer::{Field, Keyed, Nested};

use super::ctx::Ctx;
use super::table::{CellsDoc, TableKind};
use super::value::parse_java_double;
use crate::xml::{Attribute, Element};

/// The category of a scaling that names none (legacy's default,
/// `xml/TableScaleUnmarshaller.java:429`).
pub const DEFAULT_CATEGORY: &str = "Default";

/// A formula as written and as parsed: a scaling's `expression` or
/// `to_byte`, which uses only `x`.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    text: String,
    expr: Expr,
}

impl Formula {
    /// The formula text, as written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The parsed formula.
    #[must_use]
    pub const fn expr(&self) -> &Expr {
        &self.expr
    }
}

/// One `<scaling>` (or `<scalingbase>`), as a partial document (C1): every
/// field is [`Field::Unset`] unless the element says it.
///
/// # The format
///
/// Legacy RomRaider (`xml/TableScaleUnmarshaller.java:411-461`,
/// `maps/Scale.java`) reads a `<scaling>` child of a `<table>` (or of an
/// axis `<table>`), and a `<scalingbase>` child of `<roms>`, with these
/// attributes:
///
/// | Attribute | Field | Legacy |
/// |-----------|-------|--------|
/// | `base` | [`ScalingDoc::base`]: a `<scalingbase>` to start from | `:413-425` |
/// | `category` | [`ScalingDoc::category`], and the scaling's key in its table ([`scaling_key`]; `Default` when absent) | `:428-430` |
/// | `name` | [`ScalingDoc::name`] | `:435-438` |
/// | `units` | [`ScalingDoc::units`] | `:448` |
/// | `expression` | [`ScalingDoc::to_real`], a parsed [`Formula`] | `:449` |
/// | `to_byte` | [`ScalingDoc::to_raw`], a parsed [`Formula`]; empty is none | `:440` |
/// | `format` | [`ScalingDoc::format`], a [`DisplayFormat`] | `:450` |
/// | `min`, `max` | [`ScalingDoc::min`], [`ScalingDoc::max`] | `:451-452` |
/// | `coarseincrement` (or `increment`), `fineincrement` | [`ScalingDoc::coarse_increment`], [`ScalingDoc::fine_increment`] | `:453-455` |
///
/// `units`, `expression`, `format`, `min`, `max` and the increments are
/// compared ignoring ASCII case, as legacy's `equalsIgnoreCase` loop over
/// the attributes did; `base`, `category`, `name` and `to_byte` are
/// case-sensitive, as legacy's `getNamedItem` was. A value of `NaN` is an
/// absent attribute, as in the table reader. A scaling has no child
/// elements or text. Every other attribute (`storagetype` and `endian`
/// included: legacy reads those on the `<table>` only, never on a
/// `<scaling>`) and every child element is a warning, and ignored.
///
/// # Typed at read time
///
/// * `expression` and `to_byte` are parsed once, here, with the JEP-compatible
///   parser ([`rr_core::expr`]) and checked to use only `x`
///   ([`ExprForm`]). An `expression` that does not parse keeps its `EXP`
///   code, one that names a parameter is `CNV0001`, both Errors at the
///   attribute's value, naming the character at fault. A `to_byte` that
///   cannot be used is a warning (`DEF0025`, naming the `EXP` or `CNV` code)
///   and is ignored, as if absent: a definition's `to_byte` is only ever
///   checked, never used to write (values are stored with the inverse of
///   `expression`, see [`rr_core::conversion`]), and legacy loaded such a
///   definition (five pinned definitions have a `to_byte` with an unbalanced
///   parenthesis).
/// * `format` is a [`DisplayFormat`], kept as written (D-012).
/// * `min`, `max` and the increments are `f64`, read as legacy's
///   `Double.parseDouble` did; anything else is an Error (`DEF0023`; legacy
///   failed the whole file), and `Infinity` a warning, ignored.
///
/// # Keys and repeats
///
/// A table's scalings are a [`Keyed`] list ([`CellsDoc::scalings`]) keyed
/// by category, lower-cased: legacy kept one scale per category, compared
/// ignoring case (`maps/Table.java:207-229`), and a `<scaling>` of a
/// category the table already had (inherited, or earlier in the same
/// table) changed it (`xml/TableScaleUnmarshaller.java:273-277`). A repeat
/// within one table merges into the earlier one with a `DEF0008` warning
/// naming both places. `<scalingbase>`s are keyed by name lower-cased
/// ([`DefinitionFile::scaling_bases`](super::DefinitionFile::scaling_bases));
/// one naming an earlier one replaces it whole, as legacy's map `put` did,
/// with a `DEF0008` warning naming both.
///
/// # Legacy forms not kept
///
/// * Legacy reset `to_byte` on every `<scaling>` without one; here it is
///   inherited like every other field (as `dataLayout` is, see
///   [`CellsDoc::layout`]).
/// * A `<scaling base="...">` naming a `<scalingbase>` took that base's
///   category and ignored its own `category`; here the `category` written
///   on the `<scaling>` keys it (no pinned definition has a `base` on a
///   scaling, or a `<scalingbase>`).
/// * Legacy also resolved a `base` against every scaling read before it
///   (table scalings went into the same map, by name); resolving `base` is
///   [`DefinitionSet`](super::DefinitionSet)'s, against the
///   [`DefinitionFile::scaling_bases`](super::DefinitionFile::scaling_bases) of the files of the
///   ROM's chain.
///
/// # After the merge
///
/// [`build_cells`] is the pure function a [`RomResolver`](super::RomResolver) calls once a table (or
/// axis) and its scaling are merged along the `base` chain: it builds the
/// cells' [`Encoding`] (C3) and [`Conversion`] (C4).
/// [`select_scaling`] picks the scaling shown, as legacy did, and
/// [`to_raw_mismatch`] checks a definition's `to_byte` against its
/// `expression` (`CNV0005`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScalingDoc {
    /// `base`: the name of a `<scalingbase>` this scaling starts as a copy
    /// of (resolved by a [`RomResolver`](super::RomResolver)). `none`, in any
    /// case, is no base.
    pub base: Field<String>,
    /// `name`, as written. (Legacy named an unnamed scaling by its
    /// `units`, or `none`.)
    pub name: Field<String>,
    /// `category`, as written: the scaling's group (`Metric`, `Imperial`);
    /// the table's key for it is [`scaling_key`]. Unset is
    /// [`DEFAULT_CATEGORY`].
    pub category: Field<String>,
    /// `units`: the unit of the real values, verbatim.
    pub units: Field<String>,
    /// `expression`: raw (`x`) to real. Unset after the merge is legacy's
    /// default, `x`.
    pub to_real: Field<Formula>,
    /// `to_byte`: real (`x`) to raw; `Set(None)` for an empty `to_byte`
    /// (legacy's "none").
    pub to_raw: Field<Option<Formula>>,
    /// `format`: the display pattern, verbatim.
    pub format: Field<DisplayFormat>,
    /// `min`: the lowest real value, for display.
    pub min: Field<f64>,
    /// `max`: the highest real value, for display.
    pub max: Field<f64>,
    /// `coarseincrement` (or `increment`): the larger editing step.
    pub coarse_increment: Field<f64>,
    /// `fineincrement`: the smaller editing step.
    pub fine_increment: Field<f64>,
}

rr_core::merge_fields!(ScalingDoc {
    base,
    name,
    category,
    units,
    to_real,
    to_raw,
    format,
    min,
    max,
    coarse_increment,
    fine_increment,
});

/// The key of the scaling of category `category` in
/// [`CellsDoc::scalings`]: the category lower-cased, as legacy compared
/// categories ignoring case.
#[must_use]
pub fn scaling_key(category: &str) -> String {
    category.to_lowercase()
}

/// The key of the `<scalingbase>` named `name` in
/// [`DefinitionFile::scaling_bases`](super::DefinitionFile::scaling_bases):
/// the name lower-cased, as legacy's `toLowerCase` keys.
#[must_use]
pub fn scaling_base_key(name: &str) -> String {
    name.to_lowercase()
}

/// The attributes compared ignoring ASCII case, and the canonical name of
/// each.
const CASELESS: &[&str] = &[
    "units",
    "expression",
    "format",
    "max",
    "min",
    "coarseincrement",
    "increment",
    "fineincrement",
];

/// Reads one `<scaling>` or `<scalingbase>` element.
pub(super) fn read_scaling(ctx: &mut Ctx<'_>, el: &Element) -> ScalingDoc {
    let mut doc = ScalingDoc::default();
    let mut coarse: Option<&Attribute> = None;
    for attr in el.attributes() {
        let value = attr.value();
        let caseless = CASELESS
            .iter()
            .find(|n| attr.name().eq_ignore_ascii_case(n))
            .copied();
        if value == "NaN" {
            // Legacy read a `NaN` value as an absent attribute; an unknown
            // attribute is still reported, whatever its value.
            let known = caseless.is_some()
                || ["base", "name", "category", "to_byte"].contains(&attr.name());
            if !known {
                ctx.unknown_attribute(attr, &format!("<{}>", el.name()));
            }
            continue;
        }
        match (attr.name(), caseless) {
            ("base", _) => {
                if !value.eq_ignore_ascii_case("none") {
                    doc.base = Field::Set(value.to_owned());
                }
            }
            ("name", _) => doc.name = Field::Set(value.to_owned()),
            ("category", _) => doc.category = Field::Set(value.to_owned()),
            ("to_byte", _) => {
                if value.is_empty() {
                    doc.to_raw = Field::Set(None);
                } else if let Some(f) = read_formula(ctx, attr, Which::ToRaw) {
                    doc.to_raw = Field::Set(Some(f));
                }
            }
            (_, Some("units")) => doc.units = Field::Set(value.to_owned()),
            (_, Some("expression")) => {
                if let Some(f) = read_formula(ctx, attr, Which::ToReal) {
                    doc.to_real = Field::Set(f);
                }
            }
            (_, Some("format")) => doc.format = Field::Set(DisplayFormat::new(value)),
            (_, Some("min")) => set_number(ctx, attr, &mut doc.min),
            (_, Some("max")) => set_number(ctx, attr, &mut doc.max),
            (_, Some("fineincrement")) => set_number(ctx, attr, &mut doc.fine_increment),
            (_, Some(_)) => {
                // `coarseincrement` or `increment`: one field.
                if let Some(earlier) = coarse {
                    ctx.warning(
                        codes::CONTENT_IGNORED,
                        format!(
                            "`{}` and `{}` on one <{}> both give the coarse increment; the \
                             later, `{}`, is read",
                            earlier.name(),
                            attr.name(),
                            el.name(),
                            attr.name()
                        ),
                        attr.span(),
                    );
                }
                coarse = Some(attr);
                set_number(ctx, attr, &mut doc.coarse_increment);
            }
            _ => ctx.unknown_attribute(attr, &format!("<{}>", el.name())),
        }
    }
    ctx.no_text(el);
    for child in el.elements() {
        ctx.unknown_element(child, el.name());
    }
    doc
}

/// Which formula an attribute holds.
#[derive(Debug, Clone, Copy)]
enum Which {
    ToReal,
    ToRaw,
}

/// The formula in `attr`, parsed and checked; `None` when it is not one,
/// reported at the attribute's value: an `expression` as an Error with its
/// `EXP` code (or `CNV0001`), a `to_byte` as a `DEF0025` warning.
fn read_formula(ctx: &mut Ctx<'_>, attr: &Attribute, which: Which) -> Option<Formula> {
    let text = attr.value();
    let checked = match which {
        Which::ToReal => ExprForm::parse(text, None).map(|form| Some(form.to_real_expr().clone())),
        // Parsed as the `to_raw` of an identity, so an error names the
        // `to_raw` formula.
        Which::ToRaw => ExprForm::parse("x", Some(text)).map(|form| form.to_raw().cloned()),
    };
    match checked {
        Ok(Some(expr)) => Some(Formula {
            text: text.to_owned(),
            expr,
        }),
        // `ExprForm::parse` keeps a given `to_raw`; never reached.
        Ok(None) => None,
        Err(error) => {
            report_formula_error(ctx, attr, &error, which);
            None
        }
    }
}

/// Reports `error`, in the formula of `attr`, at the attribute's value.
fn report_formula_error(
    ctx: &mut Ctx<'_>,
    attr: &Attribute,
    error: &ConversionError,
    which: Which,
) {
    let text = attr.value();
    // The value is unescaped, so a byte offset in it is not a column of the
    // file; the character is named in the message instead.
    let at = error
        .span()
        .and_then(|(_, span)| usize::try_from(span.start()).ok())
        .and_then(|start| text.get(..start))
        .map(|before| before.chars().count() + 1);
    let mut message = format!("`{}` {text:?}: {error}", attr.name());
    if let Some(at) = at {
        message.push_str(&format!(" (at character {at})"));
    }
    let provenance = ctx.provenance(attr.value_span());
    let diagnostic = match which {
        Which::ToReal => Diagnostic::error(error.code(), message, provenance),
        Which::ToRaw => {
            message.push_str(&format!(
                " [{}]; ignored: values are stored with the inverse of the expression",
                error.code()
            ));
            Diagnostic::warning(codes::UNUSABLE_TO_BYTE, message, provenance)
        }
    };
    ctx.diags.push(diagnostic);
}

/// Reads `attr` as a decimal number into `field`: an Error when it is not
/// one (`DEF0023`), a warning when it is infinite (ignored).
fn set_number(ctx: &mut Ctx<'_>, attr: &Attribute, field: &mut Field<f64>) {
    match parse_java_double(attr.value()) {
        Some(v) if v.is_finite() => *field = Field::Set(v),
        Some(_) => ctx.warning(
            codes::CONTENT_IGNORED,
            format!(
                "{} {:?} is not a finite number; ignored",
                attr.name(),
                attr.value()
            ),
            attr.value_span(),
        ),
        None => ctx.error(
            codes::INVALID_SCALING_NUMBER,
            format!(
                "{} {:?} is not a decimal number (such as `.01`, `-40` or `1e3`)",
                attr.name(),
                attr.value()
            ),
            attr.value_span(),
        ),
    }
}

/// The key a `<scaling>` is filed under in its table: its `category`
/// lower-cased, or [`DEFAULT_CATEGORY`]'s.
fn key_of(doc: &ScalingDoc) -> String {
    scaling_key(doc.category.get().map_or(DEFAULT_CATEGORY, String::as_str))
}

/// The scalings read so far in one table (or axis), with where each
/// category was first given, to name both places of a repeat.
#[derive(Debug, Default)]
pub(super) struct ScalingsRead {
    first: Vec<(String, Span)>,
}

impl ScalingsRead {
    /// Reads the `<scaling>` `el` of the element described as `what` into
    /// `scalings`, merging into an earlier one of its category with a
    /// warning.
    pub(super) fn read(
        &mut self,
        ctx: &mut Ctx<'_>,
        el: &Element,
        what: &str,
        scalings: &mut Keyed<String, ScalingDoc>,
    ) {
        let doc = read_scaling(ctx, el);
        let key = key_of(&doc);
        if let Some((_, first)) = self.first.iter().find(|(k, _)| *k == key) {
            let category = doc.category.get().map_or(DEFAULT_CATEGORY, String::as_str);
            ctx.warning(
                codes::DUPLICATE_ELEMENT,
                format!(
                    "a second <{}> of category `{category}` in {what} (the first is at {}); read \
                     as a change to the first, this later one's values winning, as in legacy",
                    el.name(),
                    line_col(*first)
                ),
                el.span(),
            );
        } else {
            self.first.push((key.clone(), el.span()));
        }
        scalings.patch(key, doc);
    }
}

/// `line:col` of a text span.
fn line_col(span: Span) -> String {
    match span {
        Span::LineCol { line, col } => format!("{line}:{col}"),
        Span::ByteOffset(o) => format!("@0x{o:X}"),
    }
}

/// Reads the `<scalingbase>` `el` into `bases`; `first` remembers where
/// each name was first given.
pub(super) fn read_scaling_base(
    ctx: &mut Ctx<'_>,
    el: &Element,
    bases: &mut Keyed<String, ScalingDoc>,
    first: &mut Vec<(String, Span)>,
) {
    let doc = read_scaling(ctx, el);
    // Legacy filed an unnamed scaling base under its units, or `none`
    // (`xml/TableScaleUnmarshaller.java:435-438`, `:459`).
    let name = doc
        .name
        .get()
        .or_else(|| doc.units.get())
        .map_or("none", String::as_str);
    let key = scaling_base_key(name);
    if let Some((_, at)) = first.iter().find(|(k, _)| *k == key) {
        ctx.warning(
            codes::DUPLICATE_ELEMENT,
            format!(
                "<{}> `{name}` is defined again (first at {}); this later one replaces it, as \
                 in legacy",
                el.name(),
                line_col(*at)
            ),
            el.span(),
        );
        bases.insert(key, Nested::Replace(doc));
    } else {
        first.push((key.clone(), el.span()));
        bases.patch(key, doc);
    }
}

/// The scaling a table shows, as legacy chose its current scale
/// (`maps/Table.java:220-240`): the one of category `preferred` (legacy's
/// "default scale" setting, `Metric` unless changed) if the table has
/// one, else the one of category [`DEFAULT_CATEGORY`]; `None` (raw values)
/// otherwise, as legacy then showed its built-in `Raw Value` scale.
#[must_use]
pub fn select_scaling<'a>(
    scalings: &'a Keyed<String, ScalingDoc>,
    preferred: Option<&str>,
) -> Option<&'a ScalingDoc> {
    let find = |category: &str| scalings.get(&scaling_key(category)).and_then(Nested::get);
    preferred.and_then(find).or_else(|| find(DEFAULT_CATEGORY))
}

/// The typed cells of a table or axis: how each is stored and how its raw
/// value converts to a real one.
#[derive(Debug, Clone, PartialEq)]
pub struct CellFormat {
    /// How a cell is stored (C3).
    pub encoding: Encoding,
    /// How a raw value becomes a real one, its unit and display format (C4).
    pub conversion: Conversion,
}

/// Builds the [`Encoding`] and [`Conversion`] of a table's (or axis's)
/// cells, once its layers are merged ([`RomResolver`](super::RomResolver)): `cells` is the merged
/// [`CellsDoc`], `kind` the table's merged kind (`None` for an axis),
/// `scaling` the merged scaling shown ([`select_scaling`], its `base`
/// already resolved; `None` for raw values) and `mem_model_endian` the ROM's
/// merged [`RomIdDoc::mem_model_endian`](super::RomIdDoc::mem_model_endian).
/// Diagnostics are reported at `at`.
///
/// | Part | From | Default (legacy) |
/// |------|------|------------------|
/// | storage | [`CellsDoc::storage`] | `uint8` for a `Switch` or `BitwiseSwitch` table (`maps/TableSwitch.java:39`); none otherwise: `DEF0024` |
/// | byte order | `mem_model_endian`, which overrides the table's (`xml/TableScaleUnmarshaller.java:162-170`), else [`CellsDoc::endian`] | big (`maps/Table.java:56`) |
/// | bitfield | [`CellsDoc::bitfield`] | none |
/// | layout | [`CellsDoc::layout`] | [`Layout::Default`] |
/// | conversion | [`ScalingDoc::to_real`] and [`ScalingDoc::to_raw`], a [`Form::Expr`] | `x` (`maps/Scale.java:35`); no scaling: `x` as a [`Form::Linear`], no unit, no format |
/// | unit, format | [`ScalingDoc::units`], [`ScalingDoc::format`] | none (D-012: no `#.##` default) |
///
/// A combination the codec refuses (a bitfield wider than the storage,
/// `movi20` little-endian) is its `ENC` Error. The value is `Some` exactly
/// when there is no Error. A static axis (one with
/// [`CellsDoc::static_values`]) is not read from the image and has no
/// encoding: do not call this for one.
pub fn build_cells(
    cells: &CellsDoc,
    kind: Option<TableKind>,
    scaling: Option<&ScalingDoc>,
    mem_model_endian: Option<Endian>,
    at: &Provenance,
) -> Loaded<Option<CellFormat>> {
    let mut diags = Diagnostics::new();
    let storage = match (cells.storage.get(), kind) {
        (Some(&s), _) => Some(s),
        (None, Some(TableKind::Switch | TableKind::BitwiseSwitch)) => Some(Storage::U8),
        (None, _) => {
            diags.error(
                codes::MISSING_STORAGE_TYPE,
                "no storagetype, here or in any definition this one inherits from: its cells \
                 cannot be decoded"
                    .to_owned(),
                at.clone(),
            );
            None
        }
    };
    let endian = mem_model_endian
        .or_else(|| cells.endian.get().copied())
        .unwrap_or(Endian::Big);
    let bitfield = cells.bitfield.get().copied();
    let layout = cells.layout.get().copied().unwrap_or(Layout::Default);
    let encoding = storage.and_then(|storage| {
        Encoding::new(storage, endian, bitfield, layout)
            .map_err(|e| diags.push(e.to_diagnostic(at.clone())))
            .ok()
    });
    let conversion = build_conversion(scaling)
        .map_err(|e| diags.push(e.to_diagnostic(at.clone())))
        .ok();
    let value = encoding
        .zip(conversion)
        .map(|(encoding, conversion)| CellFormat {
            encoding,
            conversion,
        });
    diags.finish(value)
}

/// The conversion of a merged scaling (see [`build_cells`]).
fn build_conversion(scaling: Option<&ScalingDoc>) -> Result<Conversion, ConversionError> {
    let Some(scaling) = scaling else {
        return Ok(Conversion::new(
            Form::Linear(Linear::new(1.0, 0.0)?),
            None,
            None,
        ));
    };
    let to_real = match scaling.to_real.get() {
        Some(f) => f.expr.clone(),
        // Legacy's default expression (`maps/Scale.java:35`).
        None => ExprForm::parse("x", None)?.to_real_expr().clone(),
    };
    let to_raw = scaling.to_raw.get().cloned().flatten().map(|f| f.expr);
    let form = ExprForm::new(to_real, to_raw)?;
    Ok(Conversion::new(
        Form::Expr(form),
        scaling.units.get().cloned(),
        scaling.format.get().cloned(),
    ))
}

/// Where a scaling's given `to_byte` disagrees with its `expression` over
/// the cells' raw range ([`ExprForm::to_raw_mismatch`], reported as
/// `CNV0005`, a warning; edits use the inverse of `expression`). `None`
/// when there is no `to_byte`, when the two agree, for float storage and
/// for a whole-table layout.
///
/// It evaluates both formulas at up to 65 536 raw values, so a caller
/// checking many tables checks each distinct (scaling, encoding) once.
#[must_use]
pub fn to_raw_mismatch(cells: &CellFormat) -> Option<ToRawMismatch> {
    let Form::Expr(form) = cells.conversion.form() else {
        return None;
    };
    let range = cells.encoding.raw_range().ok()?;
    form.to_raw_mismatch(range)
}

#[cfg(test)]
mod tests;
