//! MF-04 (US-RR-19-3): two images with different byte orders decode and
//! encode correctly in the same process, at the same time.
//!
//! Legacy kept the byte order in a `static` field shared by every open ROM
//! (`maps/Table.java:40`, `protected static Settings.Endian memModelEndian`).
//! Every table's instance setter wrote it (`Table.java:761-763`) as each
//! definition loaded (`xml/TableScaleUnmarshaller.java:162-169`), and every
//! float cell of every ROM read it back (`maps/DataCell.java:167`, `:304`,
//! through `xml/RomAttributeParser.java:252-283`). So opening a second ROM
//! with the other byte order changed how the first one decoded.
//!
//! Here an [`Encoding`] carries its [`Endian`] by value and the encoding
//! module has no static state at all (`cargo xtask check-globals`, which
//! bans every `static` in that module, `xtask/src/globals.rs`
//! `STATELESS_PATHS`). This file shows the behaviour:
//!
//! * Two definitions are parsed from legacy attribute strings, one `big`, one
//!   `little`, each with the same tables: every integer storage, `float`,
//!   unsigned bitfields and Bosch-subtract tables (`F64` too, which legacy
//!   definitions cannot spell). `movi20` is left out because it has no
//!   little-endian form.
//! * The big-endian image is pseudo-random bytes. The values it holds are
//!   decoded by the independent reference codec, and the little-endian image
//!   is those same values written by the reference codec in the other byte
//!   order. For whole-cell tables the two images are each other's bytes
//!   reversed per cell, checked here without any codec.
//! * Eight threads, half on each image, start together at a barrier and meet
//!   again at a barrier every round, so decodes and encodes of the two byte
//!   orders interleave. Each round a thread first parses the *other* byte
//!   order's definition (opening the other ROM, the legacy trigger), then
//!   decodes its own image cell by cell and table by table, and encodes the
//!   values back into its own buffer, cell by cell and table by table. Every
//!   result must equal its own image's values and bytes.

use std::panic::{self, AssertUnwindSafe};
use std::sync::Barrier;

use rr_core::diag::{Provenance, Span};
use rr_core::encoding::legacy::{LegacyAttributes, parse_encoding};
use rr_core::encoding::{Encoding, Endian, Layout, Raw, Storage};
use rr_testkit::encoding_oracle::{reference_decode_table, reference_encode_table, same_bits};

// MF-04, at compile time: encodings and values cross threads freely and
// borrow nothing.
const _: () = {
    const fn shareable<T: Send + Sync + Copy + 'static>() {}
    shareable::<Encoding>();
    shareable::<Endian>();
    shareable::<Raw>();
};

/// Cells per table.
const CELLS: usize = 16;
/// Threads; even ones use the big-endian image, odd ones the little-endian.
const THREADS: usize = 8;
/// Rounds per thread, each ending at a barrier.
const ROUNDS: usize = 200;
/// What an encode buffer starts as: bits an encoding does not own (outside a
/// bitfield's mask) keep this value in both images.
const FILL: u8 = 0xA5;

/// One table of the definition: legacy `storagetype`, `mask` and
/// `dataLayout`.
const TABLES: &[(&str, Option<&str>, Option<&str>)] = &[
    ("uint8", None, None),
    ("uint16", None, None),
    ("uint24", None, None),
    ("uint32", None, None),
    ("uint64", None, None),
    ("int8", None, None),
    ("int16", None, None),
    ("int24", None, None),
    ("int32", None, None),
    ("int64", None, None),
    ("float", None, None),
    ("uint16", Some("0FF0"), None),
    ("uint32", Some("00FFF000"), None),
    ("uint64", Some("FFFF000000000000"), None),
    ("uint8", None, Some("bosch_subtract")),
    ("uint16", None, Some("bosch_subtract")),
    ("uint32", Some("0FFFFFF0"), Some("bosch_subtract")),
];

fn here() -> Provenance {
    Provenance::new("defs/mf04.xml", Span::line_col(1, 1))
}

/// The definition's encodings in `endian` (`"big"` / `"little"`), parsed
/// from the legacy strings, plus an `F64` table.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn definition(endian: &str) -> Vec<Encoding> {
    let mut encodings: Vec<Encoding> = TABLES
        .iter()
        .map(|&(storage_type, mask, data_layout)| {
            let loaded = parse_encoding(
                &LegacyAttributes {
                    storage_type,
                    endian,
                    mask,
                    data_layout,
                },
                &here(),
            );
            assert!(
                loaded.diagnostics().is_empty(),
                "{storage_type} {endian}: {:?}",
                loaded.diagnostics()
            );
            loaded.value().copied().flatten().unwrap()
        })
        .collect();
    let byte_order = encodings[0].endian();
    encodings.push(Encoding::new(Storage::F64, byte_order, None, Layout::Default).unwrap());
    encodings
}

/// One image: its tables' encodings, the bytes, where each table starts, and
/// the values each table holds.
struct Image {
    encodings: Vec<Encoding>,
    bytes: Vec<u8>,
    offsets: Vec<usize>,
    values: Vec<Vec<Raw>>,
}

impl Image {
    fn table(&self, t: usize) -> &[u8] {
        let len = CELLS * self.encodings[t].storage().cell_bytes();
        &self.bytes[self.offsets[t]..self.offsets[t] + len]
    }
}

/// A deterministic byte stream (64-bit LCG, high byte).
struct Bytes(u64);

impl Bytes {
    fn next(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0.to_be_bytes()[0]
    }
}

/// The big-endian image (random bytes, values by the reference codec) and the
/// little-endian image of the same values (bytes by the reference codec).
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn images() -> (Image, Image) {
    let big = definition("big");
    let little = definition("little");
    assert!(big.iter().all(|e| e.endian() == Endian::Big));
    assert!(little.iter().all(|e| e.endian() == Endian::Little));

    let mut rng = Bytes(0x004D_462D_3034);
    let mut big_bytes = Vec::new();
    let mut little_bytes = Vec::new();
    let mut offsets = Vec::new();
    let mut values = Vec::new();
    for (&b, &l) in big.iter().zip(&little) {
        assert_eq!(
            (b.storage(), b.bitfield(), b.layout()),
            (l.storage(), l.bitfield(), l.layout())
        );
        let len = CELLS * b.storage().cell_bytes();
        let table: Vec<u8> = (0..len).map(|_| rng.next()).collect();
        let held = reference_decode_table(b, &table).unwrap();
        let mut reversed = vec![FILL; len];
        reference_encode_table(l, &held, &mut reversed)
            .expect("unsigned fields only")
            .unwrap();
        offsets.push(big_bytes.len());
        // Bits outside a bitfield are FILL in both images, so a fresh FILL
        // buffer encodes back to exactly the image.
        let mut big_table = vec![FILL; len];
        reference_encode_table(b, &held, &mut big_table)
            .expect("unsigned fields only")
            .unwrap();
        big_bytes.extend_from_slice(&big_table);
        little_bytes.extend_from_slice(&reversed);
        values.push(held);
    }
    let image = |encodings, bytes| Image {
        encodings,
        bytes,
        offsets: offsets.clone(),
        values: values.clone(),
    };
    (image(big, big_bytes), image(little, little_bytes))
}

/// Default-layout tables are the cell's bytes reversed.
fn is_whole_cell(encoding: Encoding) -> bool {
    encoding.layout() == Layout::Default && encoding.bitfield().is_none()
}

#[test]
fn the_two_images_hold_the_same_values_in_opposite_byte_orders() {
    let (big, little) = images();
    let mut whole_cell_tables = 0;
    for t in 0..big.encodings.len() {
        if big.encodings[t].storage().cell_bytes() > 1 {
            // A one-byte cell has no byte order; every wider one differs.
            assert_ne!(big.table(t), little.table(t), "table {t}");
        }
        if !is_whole_cell(big.encodings[t]) {
            continue;
        }
        whole_cell_tables += 1;
        let width = big.encodings[t].storage().cell_bytes();
        for (b, l) in big
            .table(t)
            .chunks(width)
            .zip(little.table(t).chunks(width))
        {
            let mut reversed = b.to_vec();
            reversed.reverse();
            assert_eq!(reversed, l, "table {t}");
        }
    }
    assert_eq!(whole_cell_tables, 12);
}

/// One thread's round on `image`: decode it and encode its values back.
/// `Err` names the first result that is not `image`'s own. It never panics,
/// so a failing thread still reaches every barrier and the test fails
/// instead of hanging.
fn round(image: &Image, buffer: &mut [u8], thread: usize, round: usize) -> Result<(), String> {
    for (t, &encoding) in image.encodings.iter().enumerate() {
        let want = &image.values[t];
        let bytes = image.table(t);
        let width = encoding.storage().cell_bytes();
        let at = |what: &str| {
            format!(
                "thread {thread} round {round} table {t} ({} {} {:?}): {what}",
                encoding.storage().name(),
                encoding.endian().name(),
                encoding.layout()
            )
        };

        let got = encoding
            .decode_table(bytes, CELLS)
            .map_err(|e| at(&format!("decode_table: {e}")))?;
        if !(got.len() == want.len() && got.iter().zip(want).all(|(&g, &w)| same_bits(g, w))) {
            return Err(at(&format!("decode_table: {got:?} != {want:?}")));
        }

        let out = &mut buffer[image.offsets[t]..image.offsets[t] + bytes.len()];
        out.fill(FILL);
        encoding
            .encode_table(want, out)
            .map_err(|e| at(&format!("encode_table: {e}")))?;
        if out != bytes {
            return Err(at(&format!("encode_table: {out:02X?} != {bytes:02X?}")));
        }

        if encoding.layout() == Layout::BoschSubtract {
            continue; // no value per cell
        }
        out.fill(FILL);
        for (k, (cell, &w)) in bytes.chunks(width).zip(want).enumerate() {
            let g = encoding
                .decode(cell)
                .map_err(|e| at(&format!("decode cell {k}: {e}")))?;
            if !same_bits(g, w) {
                return Err(at(&format!("decode cell {k}: {g:?} != {w:?}")));
            }
            encoding
                .encode_into(w, &mut out[k * width..(k + 1) * width])
                .map_err(|e| at(&format!("encode_into cell {k}: {e}")))?;
        }
        if out != bytes {
            return Err(at(&format!("encode_into: {out:02X?} != {bytes:02X?}")));
        }
    }
    Ok(())
}

#[test]
fn big_and_little_endian_images_decode_and_encode_concurrently() {
    let (big, little) = images();
    let barrier = Barrier::new(THREADS);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..THREADS)
            .map(|thread| {
                let (image, other_endian) = if thread % 2 == 0 {
                    (&big, "little")
                } else {
                    (&little, "big")
                };
                let barrier = &barrier;
                scope.spawn(move || {
                    let mut buffer = vec![FILL; image.bytes.len()];
                    let mut failure = None;
                    barrier.wait();
                    for r in 0..ROUNDS {
                        if failure.is_none() {
                            // A panic here must not leave the others at the
                            // barrier: it becomes this thread's failure.
                            let work = panic::catch_unwind(AssertUnwindSafe(|| {
                                // Open the other ROM's definition: legacy's trigger.
                                let other = definition(other_endian);
                                assert_ne!(other[0].endian(), image.encodings[0].endian());
                                round(image, &mut buffer, thread, r)
                            }));
                            failure = match work {
                                Ok(result) => result.err(),
                                Err(_) => Some(format!("thread {thread} round {r}: panicked")),
                            };
                        }
                        barrier.wait();
                    }
                    failure
                })
            })
            .collect();
        workers
            .into_iter()
            .filter_map(|w| w.join().unwrap())
            .collect()
    });
    assert!(
        failures.is_empty(),
        "{} of {THREADS} threads decoded or encoded another image's way:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn opening_a_second_definition_does_not_change_the_first() {
    // The legacy sequence on one thread: load BE, load LE, read the BE ROM.
    let (big, little) = images();
    let reloaded_little = definition("little");
    assert_eq!(reloaded_little, little.encodings);
    let mut buffer = vec![FILL; big.bytes.len()];
    round(&big, &mut buffer, 0, 0).unwrap();
    let _reloaded_big = definition("big");
    let mut buffer = vec![FILL; little.bytes.len()];
    round(&little, &mut buffer, 0, 0).unwrap();
}
