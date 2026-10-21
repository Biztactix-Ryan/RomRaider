//! Unit tests for the raw-range index space (US-RR-24-10).

use super::*;
use crate::encoding::{BitField, Storage};

fn int_field(mask: u64, shift: Option<u32>, signed: bool) -> Domain {
    Domain::new(RawRange::Int(
        BitField::new(mask, shift, signed).unwrap().raw_range(),
    ))
}

#[test]
fn float_keys_follow_the_total_order_and_round_trip() {
    let f64s = [
        -f64::MAX,
        -1.0,
        -f64::MIN_POSITIVE,
        -5e-324,
        -0.0,
        0.0,
        5e-324,
        1.0,
        f64::MAX,
    ];
    let keys: Vec<i128> = f64s.iter().map(|&f| key64(f)).collect();
    assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
    assert_eq!((key64(-0.0), key64(0.0), key64(5e-324)), (-1, 0, 1));
    assert_eq!(key64(-5e-324), -2);
    for (&f, &k) in f64s.iter().zip(&keys) {
        assert_eq!(from_key64(k).to_bits(), f.to_bits());
    }

    let f32s = [-f32::MAX, -1.0, -1e-45, -0.0, 0.0, 1e-45, 1.0, f32::MAX];
    let keys: Vec<i128> = f32s.iter().map(|&f| key32(f)).collect();
    assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
    assert_eq!((key32(-0.0), key32(0.0), key32(1e-45)), (-1, 0, 1));
    assert_eq!(key32(-1e-45), -2);
    for (&f, &k) in f32s.iter().zip(&keys) {
        assert_eq!(from_key32(k).to_bits(), f.to_bits());
    }
}

#[test]
fn compress_and_deposit_squeeze_out_the_holes() {
    // Unsigned mask 0b1010 at shift 0: holes 0b0101, storable {0, 2, 8, 10}.
    for (rank, value) in [(0, 0), (1, 2), (2, 8), (3, 10)] {
        assert_eq!(deposit(rank, 0b0101), value);
        assert_eq!(compress(value, 0b0101), rank);
    }
    // Low two bits kept zero: storable multiples of 4, signed.
    for (rank, value) in [(-8, -32), (-1, -4), (0, 0), (7, 28)] {
        assert_eq!(deposit(rank, 0b11), value);
        assert_eq!(compress(value, 0b11), rank);
    }
    // No holes: the identity, across the whole i128 range a storage needs.
    for v in [i128::from(i64::MIN), -1, 0, 1, i128::from(u64::MAX)] {
        assert_eq!(deposit(v, 0), v);
        assert_eq!(compress(v, 0), v);
    }
}

#[test]
fn integer_domains_index_the_storable_values() {
    let d = Domain::new(Storage::U8.raw_range());
    assert_eq!((d.lo(), d.hi(), d.len()), (0, 255, 256));
    assert!(d.is_int());
    assert_eq!(d.raw(200), Raw::Int(200));
    assert_eq!(d.value(200), 200.0);

    let d = Domain::new(Storage::I16.raw_range());
    assert_eq!((d.lo(), d.hi(), d.len()), (-32768, 32767, 65536));

    let d = Domain::new(Storage::U64.raw_range());
    assert_eq!(d.len(), 1 << 64);
    assert_eq!(d.raw(d.hi()), Raw::Int(i128::from(u64::MAX)));

    let d = int_field(0b1010, Some(0), false);
    assert_eq!((d.lo(), d.hi(), d.len()), (0, 3, 4));
    let raws: Vec<Raw> = (d.lo()..=d.hi()).map(|i| d.raw(i)).collect();
    assert_eq!(raws, [0, 2, 8, 10].map(Raw::Int));

    // Signed, mask 0xF0 shifted by 2: multiples of 4 from -32 to 28.
    let d = int_field(0xF0, Some(2), true);
    assert_eq!((d.lo(), d.hi(), d.len()), (-8, 7, 16));
    assert_eq!(
        (d.raw(d.lo()), d.raw(d.hi())),
        (Raw::Int(-32), Raw::Int(28))
    );
}

#[test]
fn float_domains_index_every_finite_value() {
    let d = Domain::new(Storage::F32.raw_range());
    assert!(!d.is_int());
    assert_eq!(d.raw(d.lo()), Raw::F32(-f32::MAX));
    assert_eq!(d.raw(d.hi()), Raw::F32(f32::MAX));
    assert_eq!(d.len(), 2 * 0x7F7F_FFFF + 2);
    assert_eq!(d.value(key32(1.5)), 1.5);
    let Raw::F32(neg_zero) = d.raw(-1) else {
        panic!("an F32 raw value");
    };
    assert_eq!(neg_zero.to_bits(), (-0.0_f32).to_bits());

    let d = Domain::new(Storage::F64.raw_range());
    assert_eq!(d.raw(d.lo()), Raw::F64(-f64::MAX));
    assert_eq!(d.raw(d.hi()), Raw::F64(f64::MAX));
    assert_eq!(d.len(), 2 * 0x7FEF_FFFF_FFFF_FFFF + 2);
    assert_eq!(d.value(key64(-2.5)), -2.5);
}

#[test]
fn floor_and_ceil_find_the_storable_neighbours() {
    let d = Domain::new(Storage::U8.raw_range());
    assert_eq!(d.floor(-1.0), None);
    assert_eq!(d.floor(0.0), Some(0));
    assert_eq!(d.floor(10.5), Some(10));
    assert_eq!(d.floor(1e9), Some(255));
    assert_eq!(d.ceil(-1e9), Some(0));
    assert_eq!(d.ceil(10.5), Some(11));
    assert_eq!(d.ceil(255.0), Some(255));
    assert_eq!(d.ceil(255.5), None);

    // Storable {0, 2, 8, 10}: 5 lies between 2 and 8 (compress(5) would
    // give the rank of 0).
    let d = int_field(0b1010, Some(0), false);
    assert_eq!(d.floor(5.0).map(|i| d.raw(i)), Some(Raw::Int(2)));
    assert_eq!(d.floor(7.0).map(|i| d.raw(i)), Some(Raw::Int(2)));
    assert_eq!(d.ceil(3.0).map(|i| d.raw(i)), Some(Raw::Int(8)));
    assert_eq!(d.ceil(8.0).map(|i| d.raw(i)), Some(Raw::Int(8)));

    let d = Domain::new(Storage::F32.raw_range());
    assert_eq!(
        d.floor(0.1).map(|i| d.raw(i)),
        Some(Raw::F32(0.099_999_994))
    );
    assert_eq!(d.ceil(0.1).map(|i| d.raw(i)), Some(Raw::F32(0.1)));
    assert_eq!(
        d.floor(f64::MAX).map(|i| d.raw(i)),
        Some(Raw::F32(f32::MAX))
    );
    assert_eq!(d.ceil(f64::MAX), None);
}

#[test]
fn first_true_bisects_a_false_then_true_predicate() {
    assert_eq!(first_true(0, 10, |i| i >= 3), 3);
    assert_eq!(first_true(0, 10, |i| i >= 0), 0);
    assert_eq!(first_true(0, 10, |_| false), 10);
    assert_eq!(first_true(-5, -5, |_| true), -5);
    let d = Domain::new(Storage::I8.raw_range());
    assert_eq!(d.partition(|i| d.value(i) >= 0.0), 0);
    assert_eq!(d.partition(|_| false), 128);
}
