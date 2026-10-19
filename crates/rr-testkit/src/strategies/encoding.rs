//! Strategies for Encoding (concept C3, `rr-core::encoding`; story US-RR-19,
//! task US-RR-19-12).
//!
//! * **Encodings.** Every combination [`Encoding::new`] accepts is one
//!   [`EncodingShape`]: a storage, a byte order, a bitfield kind
//!   ([`FieldKind`]: none, unsigned, signed) and a layout. [`encoding_shapes`]
//!   lists them all, written from the codec's documented rules (`U20` is
//!   big-endian only, floats take no bitfield, Bosch subtract is for whole-byte
//!   integers); the tests check that list against [`Encoding::new`] itself, so
//!   it can neither miss a valid combination nor contain an invalid one.
//!   [`any_encoding`] picks a shape uniformly and then a concrete encoding of
//!   it ([`encoding_in`]); [`cell_encoding`] keeps the shapes whose cells have
//!   a value on their own ([`Layout::Default`]), [`bosch_subtract_encoding`]
//!   the whole-table ones.
//! * **Bitfields.** [`unsigned_bitfield`] and [`signed_bitfield`] generate
//!   masks that fit a storage, contiguous and not (unsigned only), from one
//!   bit to the whole value, with the default shift or an explicit one up to
//!   the mask's trailing zeros (so a field can keep low zero bits).
//! * **Values.** [`value_for`] generates raw values the encoding can store,
//!   over its full range: uniform values plus the extremes (`min`, `max`, `0`,
//!   `±1`, `min + 1`, `max - 1`, the sign boundary, the bitfield's width
//!   boundary) and, for floats, every IEEE 754 class ([`F32_EDGE_BITS`],
//!   [`F64_EDGE_BITS`]: `±0`, subnormals, normals, `±Inf`, quiet and
//!   signalling NaNs with payloads). [`invalid_value_for`] generates values it
//!   must refuse.
//! * **Bytes.** [`bytes_for`] generates one cell's bytes: random, uniform
//!   fills, single set bits, the sign boundary and the float edge patterns in
//!   the encoding's byte order. [`table_bytes_for`] concatenates cells.
//!
//! The values are checked against the reference codec by
//! [`encoding_oracle`](crate::encoding_oracle).
//!
//! # Bosch subtract (US-RR-19-10)
//!
//! A Bosch-subtract cell has no value on its own: rr-core's single-cell codec
//! refuses it (`EncodingError::WholeTableLayout`), as does the reference
//! codec. [`any_encoding`] and [`bosch_subtract_encoding`] generate those
//! encodings, and [`table_bytes_for`] generates their tables, which rr-core's
//! table codec (`Encoding::decode_table` / `encode_table`) decodes and
//! encodes. The table properties (`crates/rr-testkit/tests/encoding_table.rs`)
//! are `bosch_subtract_encoding()` / `any_encoding()` × `table_bytes_for(..)`,
//! checked with
//! [`encoding_oracle::check_table_bytes`](crate::encoding_oracle::check_table_bytes)
//! and [`check_encode_table`](crate::encoding_oracle::check_encode_table).

use std::ops::RangeInclusive;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, Storage};

/// The bitfield kind of an encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FieldKind {
    /// No bitfield: the whole stored value is the raw value.
    Whole,
    /// An unsigned bitfield.
    Unsigned,
    /// A signed (two's complement) bitfield.
    Signed,
}

impl FieldKind {
    /// Every bitfield kind.
    pub const ALL: [Self; 3] = [Self::Whole, Self::Unsigned, Self::Signed];

    /// The kind of `bitfield`.
    #[must_use]
    pub const fn of(bitfield: Option<BitField>) -> Self {
        match bitfield {
            None => Self::Whole,
            Some(field) if field.is_signed() => Self::Signed,
            Some(_) => Self::Unsigned,
        }
    }
}

/// Every byte order.
pub const ENDIANS: [Endian; 2] = [Endian::Big, Endian::Little];

/// Every layout.
pub const LAYOUTS: [Layout; 2] = [Layout::Default, Layout::BoschSubtract];

/// One storage × endian × bitfield kind × layout combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EncodingShape {
    /// The storage type.
    pub storage: Storage,
    /// The byte order.
    pub endian: Endian,
    /// The bitfield kind.
    pub field: FieldKind,
    /// The layout.
    pub layout: Layout,
}

impl EncodingShape {
    /// The shape of `encoding`.
    #[must_use]
    pub const fn of(encoding: Encoding) -> Self {
        Self {
            storage: encoding.storage(),
            endian: encoding.endian(),
            field: FieldKind::of(encoding.bitfield()),
            layout: encoding.layout(),
        }
    }

    /// `true` if [`Encoding::new`] accepts this combination (with a bitfield
    /// that fits the storage). Written from the codec's documented rules, not
    /// by calling it; the tests check the two agree.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        let u20 = matches!(self.storage, Storage::U20);
        let float = self.storage.is_float();
        let little_u20 = u20 && matches!(self.endian, Endian::Little);
        let bosch_bad = matches!(self.layout, Layout::BoschSubtract) && (u20 || float);
        let field_bad = float && !matches!(self.field, FieldKind::Whole);
        !(little_u20 || bosch_bad || field_bad)
    }

    /// `true` if one cell of this shape has a value on its own (the
    /// single-cell codec decodes it).
    #[must_use]
    pub const fn is_single_cell(self) -> bool {
        matches!(self.layout, Layout::Default)
    }
}

/// Every valid shape ([`EncodingShape::is_valid`]): the full storage × endian
/// × bitfield kind × layout product, invalid combinations removed.
#[must_use]
pub fn encoding_shapes() -> Vec<EncodingShape> {
    let mut out = Vec::new();
    for layout in LAYOUTS {
        for storage in Storage::ALL {
            for endian in ENDIANS {
                for field in FieldKind::ALL {
                    let shape = EncodingShape {
                        storage,
                        endian,
                        field,
                        layout,
                    };
                    if shape.is_valid() {
                        out.push(shape);
                    }
                }
            }
        }
    }
    out
}

/// A mask of the low `bits` bits (`1..=64`).
const fn low_bits(bits: u32) -> u64 {
    u64::MAX >> (u64::BITS - bits)
}

/// A run of `1..=bits - lo` set bits starting at bit `lo`, of at least
/// `min_width` bits, inside a `bits`-bit value.
fn contiguous_mask(bits: u32, min_width: u32) -> impl Strategy<Value = u64> {
    (0..=bits - min_width)
        .prop_flat_map(move |lo| (Just(lo), min_width..=bits - lo))
        .prop_map(|(lo, width)| low_bits(width) << lo)
}

/// A bitfield on `mask`: default shift, no shift, the full shift (the mask's
/// trailing zeros) or any shift in between.
fn bitfield_on(
    mask: impl Strategy<Value = u64> + 'static,
    signed: bool,
) -> BoxedStrategy<BitField> {
    (mask, 0_u8..4, any::<u32>())
        .prop_filter_map(
            "a non-zero mask with shift <= trailing zeros is a valid bitfield",
            move |(mask, pick, any_shift)| {
                let lowest = mask.trailing_zeros();
                let shift = match pick {
                    0 => None,
                    1 => Some(0),
                    2 => Some(lowest),
                    _ => Some(any_shift % (lowest + 1)),
                };
                BitField::new(mask, shift, signed).ok()
            },
        )
        .boxed()
}

/// An unsigned bitfield that fits integer `storage`: contiguous runs, any
/// non-zero (possibly non-contiguous) mask, and the edge masks (bit 0 only,
/// the top bit only, the whole value, the whole value but bit 0).
pub fn unsigned_bitfield(storage: Storage) -> BoxedStrategy<BitField> {
    let bits = storage.value_bits();
    let full = low_bits(bits);
    let mask = prop_oneof![
        2 => contiguous_mask(bits, 1),
        2 => any::<u64>().prop_map(move |m| m & full).prop_filter("non-zero", |m| *m != 0),
        1 => select(vec![1, 1 << (bits - 1), full, full ^ 1]),
    ];
    bitfield_on(mask, false)
}

/// A signed bitfield that fits integer `storage`: contiguous runs of at least
/// 2 bits, and the edge masks (low 2 bits, top 2 bits, the whole value).
pub fn signed_bitfield(storage: Storage) -> BoxedStrategy<BitField> {
    let bits = storage.value_bits();
    let full = low_bits(bits);
    let mask = prop_oneof![
        3 => contiguous_mask(bits, 2),
        1 => select(vec![0b11, 0b11 << (bits - 2), full]),
    ];
    bitfield_on(mask, true)
}

/// An encoding of `shape`, which must be [valid](EncodingShape::is_valid)
/// (take it from [`encoding_shapes`]); an invalid shape generates nothing and
/// the test fails with "too many rejects".
pub fn encoding_in(shape: EncodingShape) -> BoxedStrategy<Encoding> {
    let EncodingShape {
        storage,
        endian,
        field,
        layout,
    } = shape;
    let bitfield = match field {
        FieldKind::Whole => Just(None).boxed(),
        FieldKind::Unsigned if storage.is_integer() => {
            unsigned_bitfield(storage).prop_map(Some).boxed()
        }
        FieldKind::Signed if storage.is_integer() => {
            signed_bitfield(storage).prop_map(Some).boxed()
        }
        // A float bitfield is invalid: any mask is refused below.
        FieldKind::Unsigned | FieldKind::Signed => Just(BitField::new(1, None, false).ok()).boxed(),
    };
    bitfield
        .prop_filter_map("the shape is valid", move |bitfield| {
            Encoding::new(storage, endian, bitfield, layout).ok()
        })
        .boxed()
}

fn encoding_of_shapes(shapes: Vec<EncodingShape>) -> BoxedStrategy<Encoding> {
    select(shapes).prop_flat_map(encoding_in).boxed()
}

/// Any encoding [`Encoding::new`] accepts: a uniformly chosen
/// [shape](encoding_shapes), then a concrete encoding of it. Includes
/// Bosch-subtract encodings, whose single cells both codecs refuse.
pub fn any_encoding() -> BoxedStrategy<Encoding> {
    encoding_of_shapes(encoding_shapes())
}

/// Any encoding whose cells have a value on their own ([`Layout::Default`]):
/// every storage, both byte orders (`U20` big-endian only), with no,
/// unsigned or signed bitfield.
pub fn cell_encoding() -> BoxedStrategy<Encoding> {
    encoding_of_shapes(
        encoding_shapes()
            .into_iter()
            .filter(|s| s.is_single_cell())
            .collect(),
    )
}

/// Any Bosch-subtract encoding (a whole-table layout; see the module docs).
pub fn bosch_subtract_encoding() -> BoxedStrategy<Encoding> {
    encoding_of_shapes(
        encoding_shapes()
            .into_iter()
            .filter(|s| !s.is_single_cell())
            .collect(),
    )
}

/// IEEE 754 binary32 edge patterns: `+0`, `-0`, the smallest subnormal, the
/// largest negative subnormal, the smallest normal, `MAX`, `MIN`, `1`, `-1`,
/// `EPSILON`, `+Inf`, `-Inf`, the default quiet NaN, a negative quiet NaN with
/// a payload, the smallest signalling NaN and a negative signalling NaN with
/// the largest payload.
pub const F32_EDGE_BITS: [u32; 16] = [
    0x0000_0000,
    0x8000_0000,
    0x0000_0001,
    0x807F_FFFF,
    0x0080_0000,
    0x7F7F_FFFF,
    0xFF7F_FFFF,
    0x3F80_0000,
    0xBF80_0000,
    0x3400_0000,
    0x7F80_0000,
    0xFF80_0000,
    0x7FC0_0000,
    0xFFC0_0001,
    0x7F80_0001,
    0xFFBF_FFFF,
];

/// IEEE 754 binary64 edge patterns, the same list as [`F32_EDGE_BITS`].
pub const F64_EDGE_BITS: [u64; 16] = [
    0x0000_0000_0000_0000,
    0x8000_0000_0000_0000,
    0x0000_0000_0000_0001,
    0x800F_FFFF_FFFF_FFFF,
    0x0010_0000_0000_0000,
    0x7FEF_FFFF_FFFF_FFFF,
    0xFFEF_FFFF_FFFF_FFFF,
    0x3FF0_0000_0000_0000,
    0xBFF0_0000_0000_0000,
    0x3CB0_0000_0000_0000,
    0x7FF0_0000_0000_0000,
    0xFFF0_0000_0000_0000,
    0x7FF8_0000_0000_0000,
    0xFFF8_0000_0000_0001,
    0x7FF0_0000_0000_0001,
    0xFFF7_FFFF_FFFF_FFFF,
];

/// The range of raw integers an encoding can store, as `(min, max, step)`:
/// every stored value is `x * step` for `x` in `min..=max` with
/// `x & holes == 0` (`holes`: the bits a non-contiguous unsigned mask lacks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntDomain {
    min: i128,
    max: i128,
    step: i128,
    holes: u64,
}

impl IntDomain {
    /// The domain of integer `encoding`, or `None` for a float storage.
    fn of(encoding: Encoding) -> Option<Self> {
        let (min, max) = encoding.storage().int_range()?;
        let Some(field) = encoding.bitfield() else {
            return Some(Self {
                min,
                max,
                step: 1,
                holes: 0,
            });
        };
        let lowest = field.mask().trailing_zeros();
        // The field's own bits, lowest one at bit 0.
        let bits = field.mask() >> lowest;
        // The value keeps `lowest - shift` low zero bits.
        let step = 1_i128 << (lowest - field.shift());
        let width = bits.count_ones();
        Some(if field.is_signed() {
            Self {
                min: -(1_i128 << (width - 1)),
                max: (1_i128 << (width - 1)) - 1,
                step,
                holes: 0,
            }
        } else {
            Self {
                min: 0,
                max: i128::from(bits),
                step,
                holes: !bits,
            }
        })
    }

    /// The domain's edge values (before scaling by `step`).
    fn edges(self) -> Vec<i128> {
        let mut out = vec![self.min, self.min + 1, -1, 0, 1, self.max - 1, self.max];
        if self.min == 0 {
            // Unsigned: around the top bit, where sign confusion shows.
            let top = 1_i128 << (127 - self.max.leading_zeros());
            out.extend([top - 1, top, top + 1]);
        }
        out.retain(|x| (self.min..=self.max).contains(x) && self.fits_holes(*x));
        out
    }

    fn fits_holes(self, x: i128) -> bool {
        u64::try_from(x).map_or(true, |x| x & self.holes == 0)
    }

    fn values(self) -> BoxedStrategy<i128> {
        let Self {
            min,
            max,
            step,
            holes,
        } = self;
        let uniform = if holes == 0 {
            (min..=max).boxed()
        } else {
            // A non-contiguous unsigned field: any subset of its bits.
            let bits = !holes;
            any::<u64>().prop_map(move |r| i128::from(r & bits)).boxed()
        };
        prop_oneof![
            1 => select(self.edges()),
            3 => uniform,
        ]
        .prop_map(move |x| x * step)
        .boxed()
    }
}

/// Raw values `encoding` can store, over its full range: uniform values plus
/// the edges (`min`, `max`, `0`, `±1`, `min + 1`, `max - 1`, the unsigned sign
/// boundary, the bitfield's width boundary), and for floats every edge
/// pattern of [`F32_EDGE_BITS`] / [`F64_EDGE_BITS`] besides random bits.
/// Encoding any of them succeeds (for a [single-cell](cell_encoding)
/// encoding).
pub fn value_for(encoding: &Encoding) -> BoxedStrategy<Raw> {
    match encoding.storage() {
        Storage::F32 => prop_oneof![
            1 => select(F32_EDGE_BITS.to_vec()),
            2 => any::<u32>(),
        ]
        .prop_map(|bits| Raw::F32(f32::from_bits(bits)))
        .boxed(),
        Storage::F64 => prop_oneof![
            1 => select(F64_EDGE_BITS.to_vec()),
            2 => any::<u64>(),
        ]
        .prop_map(|bits| Raw::F64(f64::from_bits(bits)))
        .boxed(),
        _ => match IntDomain::of(*encoding) {
            Some(domain) => domain.values().prop_map(Raw::Int).boxed(),
            None => Just(Raw::Int(0)).boxed(),
        },
    }
}

/// Raw values `encoding` must refuse to store: integers just and far outside
/// its range, integers with bits a bitfield cannot hold, and values of the
/// wrong kind (a float for integer storage, an integer or the other float
/// width for float storage).
pub fn invalid_value_for(encoding: &Encoding) -> BoxedStrategy<Raw> {
    let wrong_kind = match encoding.storage() {
        Storage::F32 => prop_oneof![
            any::<i64>().prop_map(|v| Raw::Int(i128::from(v))),
            any::<u64>().prop_map(|b| Raw::F64(f64::from_bits(b))),
        ]
        .boxed(),
        Storage::F64 => prop_oneof![
            any::<i64>().prop_map(|v| Raw::Int(i128::from(v))),
            any::<u32>().prop_map(|b| Raw::F32(f32::from_bits(b))),
        ]
        .boxed(),
        _ => prop_oneof![
            any::<u32>().prop_map(|b| Raw::F32(f32::from_bits(b))),
            any::<u64>().prop_map(|b| Raw::F64(f64::from_bits(b))),
        ]
        .boxed(),
    };
    let Some(domain) = IntDomain::of(*encoding) else {
        return wrong_kind;
    };
    let low = domain.min * domain.step;
    let high = domain.max * domain.step;
    let mut misfits = vec![
        (1_i128..=1 << 70)
            .prop_map(move |d| Raw::Int(low - d))
            .boxed(),
        (1_i128..=1 << 70)
            .prop_map(move |d| Raw::Int(high + d))
            .boxed(),
    ];
    if domain.step > 1 {
        // Inside the range but with a bit below the field's lowest one.
        let step = domain.step;
        misfits.push(
            (domain.min..domain.max)
                .prop_map(move |x| Raw::Int(x * step + 1))
                .boxed(),
        );
    }
    // The holes of a non-contiguous unsigned field below its top bit (its
    // `max` is then at least 1, so the reach is 1..=64).
    let reach = 128 - domain.max.leading_zeros();
    let holes = domain.holes & low_bits(reach.clamp(1, 64));
    if domain.min == 0 && holes != 0 {
        // Inside the range but with a bit the mask does not have.
        let (step, bits) = (domain.step, !domain.holes);
        misfits.push(
            (any::<u64>(), select(hole_bits(holes)))
                .prop_map(move |(r, hole)| Raw::Int(i128::from((r & bits) | hole) * step))
                .boxed(),
        );
    }
    prop_oneof![
        3 => proptest::strategy::Union::new(misfits),
        1 => wrong_kind,
    ]
    .boxed()
}

/// Each set bit of `bits` on its own.
fn hole_bits(bits: u64) -> Vec<u64> {
    (0..u64::BITS)
        .map(|k| 1 << k)
        .filter(|bit| bits & bit != 0)
        .collect()
}

/// The `width` bytes of `pattern`'s low `width * 8` bits in `endian` order.
fn bytes_of(pattern: u64, width: usize, endian: Endian) -> Vec<u8> {
    let be = pattern.to_be_bytes();
    let cell = be.get(8 - width..).unwrap_or(&be);
    match endian {
        Endian::Big => cell.to_vec(),
        Endian::Little => cell.iter().rev().copied().collect(),
    }
}

/// One cell's bytes for `encoding` ([`cell_bytes`](Storage::cell_bytes)
/// long): random bytes, uniform fills (`00`, `FF`, `55`, `AA`, `7F`, `80`), a
/// single set bit, the cell's sign boundary (top bit only, all but the top
/// bit) and, for floats, the [edge patterns](F32_EDGE_BITS) in the
/// encoding's byte order. Every one decodes (for a
/// [single-cell](cell_encoding) encoding).
pub fn bytes_for(encoding: &Encoding) -> BoxedStrategy<Vec<u8>> {
    let width = encoding.storage().cell_bytes();
    let endian = encoding.endian();
    let bits = u32::try_from(width * 8).unwrap_or(64);
    let top = 1_u64 << (bits - 1);
    let patterns: Vec<u64> = match encoding.storage() {
        Storage::F32 => F32_EDGE_BITS.iter().map(|b| u64::from(*b)).collect(),
        Storage::F64 => F64_EDGE_BITS.to_vec(),
        _ => vec![top, top - 1, top | 1],
    };
    prop_oneof![
        4 => vec(any::<u8>(), width),
        1 => select(vec![0x00_u8, 0xFF, 0x55, 0xAA, 0x7F, 0x80]).prop_map(move |b| vec![b; width]),
        1 => (0..bits).prop_map(move |k| bytes_of(1 << k, width, endian)),
        2 => select(patterns).prop_map(move |p| bytes_of(p, width, endian)),
    ]
    .boxed()
}

/// A table of `cells` consecutive cells of `encoding`, each from
/// [`bytes_for`]. For the whole-table layouts (Bosch subtract; see the module
/// docs).
pub fn table_bytes_for(
    encoding: &Encoding,
    cells: RangeInclusive<usize>,
) -> BoxedStrategy<Vec<u8>> {
    vec(bytes_for(encoding), cells)
        .prop_map(|cells| cells.concat())
        .boxed()
}
