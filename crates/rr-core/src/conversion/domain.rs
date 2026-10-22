//! A raw range as an ordered index space (US-RR-24-10).
//!
//! The display range (and, for US-RR-24-9, a solve) walks the raw values a
//! cell can hold in increasing order. [`Domain`] numbers them with
//! consecutive integers, lowest first, so a scan, a bisection or a
//! ternary search is plain integer arithmetic on indices whatever the
//! storage:
//!
//! * an integer range with no [`holes`](crate::encoding::IntRange::holes)
//!   is indexed by the value itself;
//! * an integer range with holes is indexed by the value with its hole bits
//!   squeezed out ([`compress`]), so index `i + 1` is the next *storable*
//!   value after index `i` and values the codec would refuse are never
//!   visited;
//! * a float range is indexed by the IEEE 754 total order of its finite
//!   values ([`key32`], [`key64`]): index `i + 1` is the next float up, so
//!   `-0.0` and `+0.0` are neighbours with the same real value.
//!
//! [`Domain::value`] is monotonic (non-decreasing) in the index: an integer
//! above `2^53` rounds to the nearest `f64`, so neighbours can share a
//! value, but never swap order.

use crate::encoding::{Raw, RawRange};

use super::raw_to_f64;

/// The storable raw values of a [`RawRange`], indexed `lo ..= hi` in
/// increasing order (see the [module docs](self)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Domain {
    lo: i128,
    hi: i128,
    kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Int { holes: u64 },
    F32,
    F64,
}

impl Domain {
    /// The domain of every value in `range`.
    pub(super) fn new(range: RawRange) -> Self {
        match range {
            RawRange::Int(r) => Self {
                lo: compress(r.min(), r.holes()),
                hi: compress(r.max(), r.holes()),
                kind: Kind::Int { holes: r.holes() },
            },
            RawRange::F32 { min, max } => Self {
                lo: key32(min),
                hi: key32(max),
                kind: Kind::F32,
            },
            RawRange::F64 { min, max } => Self {
                lo: key64(min),
                hi: key64(max),
                kind: Kind::F64,
            },
        }
    }

    /// The index of the lowest value.
    pub(super) const fn lo(self) -> i128 {
        self.lo
    }

    /// The index of the highest value.
    pub(super) const fn hi(self) -> i128 {
        self.hi
    }

    /// How many values the domain holds.
    pub(super) const fn len(self) -> u128 {
        self.hi.abs_diff(self.lo) + 1
    }

    /// `true` for an integer range.
    pub(super) const fn is_int(self) -> bool {
        matches!(self.kind, Kind::Int { .. })
    }

    /// The raw value at index `i`.
    pub(super) fn raw(self, i: i128) -> Raw {
        match self.kind {
            Kind::Int { holes } => Raw::Int(deposit(i, holes)),
            Kind::F32 => Raw::F32(from_key32(i)),
            Kind::F64 => Raw::F64(from_key64(i)),
        }
    }

    /// The index of the raw value `raw`, which must be one of the
    /// domain's (the inverse of [`raw`](Self::raw)).
    pub(super) fn index(self, raw: Raw) -> i128 {
        let holes = match self.kind {
            Kind::Int { holes } => holes,
            Kind::F32 | Kind::F64 => 0,
        };
        match raw {
            Raw::Int(v) => compress(v, holes),
            Raw::F32(f) => key32(f),
            Raw::F64(f) => key64(f),
        }
    }

    /// The raw value at index `i`, as the `f64` a conversion takes.
    pub(super) fn value(self, i: i128) -> f64 {
        raw_to_f64(self.raw(i))
    }

    /// The highest index whose value is at most `v` (not NaN), or `None`
    /// when every value is above `v`.
    pub(super) fn floor(self, v: f64) -> Option<i128> {
        // The first index whose value is above `v`, minus one.
        let above = self.partition(|i| self.value(i) > v);
        (above > self.lo).then(|| above - 1)
    }

    /// The lowest index whose value is at least `v` (not NaN), or `None`
    /// when every value is below `v`.
    pub(super) fn ceil(self, v: f64) -> Option<i128> {
        let at_least = self.partition(|i| self.value(i) >= v);
        (at_least <= self.hi).then_some(at_least)
    }

    /// The first index in `lo ..= hi` where `pred` holds, or `hi + 1` when
    /// it holds nowhere. `pred` must be false then true along the indices.
    pub(super) fn partition(self, pred: impl Fn(i128) -> bool) -> i128 {
        first_true(self.lo, self.hi + 1, pred)
    }
}

/// The first index in `lo .. end` where `pred` holds, or `end`, for a
/// `pred` that is false then true along the indices (a bisection).
pub(super) fn first_true(mut lo: i128, mut end: i128, pred: impl Fn(i128) -> bool) -> i128 {
    // Each round halves `end - lo`, which no domain puts above 2^66; the
    // bound only keeps a broken predicate from looping.
    for _ in 0..BISECTIONS {
        if lo == end {
            break;
        }
        let mid = lo + (end - lo) / 2;
        if pred(mid) {
            end = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

/// The rank of the storable integer `v` among the storable integers: its
/// low 64 bits with the `holes` bits squeezed out, the bits above them
/// (the sign) shifted down to follow. Order-preserving on storable values;
/// the inverse of [`deposit`].
fn compress(v: i128, holes: u64) -> i128 {
    // No holes (every whole-cell format): the rank is the value. The loop
    // below gives the same, one bit at a time, which is most of the cost
    // of a scan over a 16-bit cell's every raw value.
    if holes == 0 {
        return v;
    }
    let mut out = 0_i128;
    let mut taken = 0_u32;
    for bit in 0..64 {
        if holes >> bit & 1 == 0 {
            out |= (v >> bit & 1) << taken;
            taken += 1;
        }
    }
    // The two parts' bits do not overlap, so the sum is their union.
    out + ((v >> 64) << taken)
}

/// The storable integer of rank `i`: the inverse of [`compress`], spreading
/// `i`'s bits over the low 64 bit positions that are not `holes`, the
/// rest (the sign) above them.
fn deposit(i: i128, holes: u64) -> i128 {
    // No holes: the value is the rank, as in `compress`.
    if holes == 0 {
        return i;
    }
    let mut out = 0_i128;
    let mut taken = 0_u32;
    for bit in 0..64 {
        if holes >> bit & 1 == 0 {
            out |= (i >> taken & 1) << bit;
            taken += 1;
        }
    }
    out + ((i >> taken) << 64)
}

/// The position of a finite `f32` in the IEEE 754 total order, `+0.0` at 0
/// and `-0.0` at -1.
fn key32(f: f32) -> i128 {
    let bits = f.to_bits();
    let magnitude = i128::from(bits & !SIGN32);
    if bits & SIGN32 == 0 {
        magnitude
    } else {
        -magnitude - 1
    }
}

/// The `f32` at position `k` of the total order ([`key32`]).
fn from_key32(k: i128) -> f32 {
    // Within the domain every key fits the 31 magnitude bits.
    if k >= 0 {
        f32::from_bits(k as u32) // no-wrap: a domain key fits 31 bits
    } else {
        f32::from_bits((-k - 1) as u32 + SIGN32) // no-wrap: a domain key fits 31 bits
    }
}

/// The position of a finite `f64` in the IEEE 754 total order, `+0.0` at 0
/// and `-0.0` at -1.
fn key64(f: f64) -> i128 {
    let bits = f.to_bits();
    let magnitude = i128::from(bits & !SIGN64);
    if bits & SIGN64 == 0 {
        magnitude
    } else {
        -magnitude - 1
    }
}

/// The `f64` at position `k` of the total order ([`key64`]).
fn from_key64(k: i128) -> f64 {
    if k >= 0 {
        f64::from_bits(k as u64) // no-wrap: a domain key fits 63 bits
    } else {
        f64::from_bits((-k - 1) as u64 + SIGN64) // no-wrap: a domain key fits 63 bits
    }
}

/// More halvings than any `i128` span needs.
const BISECTIONS: u32 = 130;

const SIGN32: u32 = 1 << 31;
const SIGN64: u64 = 1 << 63;

#[cfg(test)]
mod tests;
