//! The checksum and CRC reference vectors (`[[checksum]]` cases in
//! `fixtures/kat/checksum-*.toml`, US-RR-14-7, PROJECT.md T2 (d)) run
//! through a small reference implementation that lives only in this file.
//!
//! There is no checksum engine yet (US-RR-35 will add it to rr-rom); these
//! tests exercise the vectors now, so an arithmetic slip in a hand-worked
//! value fails today rather than when the engine lands. The reference
//! implementation is written from the fixtures' own parameter descriptions
//! and, for CRCs, from the Rocksoft model (Williams 1993, section 15); it is
//! independent of rr-core, rr-rom and legacy code, and it is not a source of
//! expected values (D-024): the values were worked by hand or transcribed
//! from the CRC RevEng catalogue before this file existed.
//!
//! Checked here:
//!
//! * every case's parameters are well formed, with no unknown keys
//!   ([`every_vector_matches_the_reference`]);
//! * its region lies inside the input and does not overlap its stored
//!   checksum, `before.hex` is the stored bytes of the input as given, and
//!   `after.hex` is the reference result stored in the case's byte order;
//! * `after` verifies, and `before` verifies exactly when the bytes as given
//!   already hold the result;
//! * every CRC check value (`"123456789"`) cites the catalogue entry its
//!   parameters come from ([`crc_check_values_cite_their_catalogue_entry`]).
//!
//! # Planned-variant coverage (US-RR-14-3)
//!
//! US-RR-14's criterion "Checksum reference vectors cover each sum/XOR/CRC
//! variant planned for the checksum engine" is checked against [`PLANNED`],
//! the one authoritative list of those variants. It is written from US-RR-35's
//! description of the engine ([`US_RR_35`]) and the legacy checksum routines,
//! never from the fixtures, and each entry names its sources: a quote of
//! US-RR-35 and/or a legacy `file:line` range with text that must be on it.
//!
//! * every planned variant has a vector, and each family has an empty-range,
//!   an all-0xFF and (for sums) a wrapping and an odd-length vector
//!   ([`every_planned_variant_has_a_vector`]);
//! * every variant a fixture holds is planned, or listed in [`EXTRA`] with a
//!   reason, so a new variant is added to the plan on purpose
//!   ([`every_vector_variant_is_planned`]);
//! * every legacy citation names an existing file, lines within it, and text
//!   on those lines ([`planned_variants_cite_their_sources`]);
//! * every planned CRC has the catalogue check vector for `"123456789"` in
//!   its own byte order ([`every_planned_crc_has_its_catalogue_check_vector`]);
//! * every legacy checksum class (each `.java` file under a `checksum`
//!   directory of `legacy/src/main/java`, and each one named `*Checksum*`) is
//!   in [`LEGACY_CLASSES`], mapped to planned variants or out of scope with a
//!   reason ([`every_legacy_checksum_class_is_mapped_or_out_of_scope`]).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rr_testkit::fixtures::repo_root;
use rr_testkit::kat::{self, ChecksumCase, Input, KatKind, Source};

/// The fixture files holding the vectors: `fixtures/kat/<prefix>*.toml`.
const PREFIX: &str = "checksum-";

/// The standard CRC check input (Williams 1993 section 15; the catalogue's
/// CHECK parameter).
const CHECK_INPUT: &[u8] = b"123456789";

// ---------------------------------------------------------------------------
// The planned variants (US-RR-14-3)
// ---------------------------------------------------------------------------

/// US-RR-35 ("Checksum engine: specs + named registry"), the lines of its
/// description that fix the algorithms, quoted from `.project/stories/US-RR-35.md`.
const US_RR_35: &str = "ChecksumSpec { algo: Sum{width,endian,init,negate} | Xor | Crc16 | Copy | Named(..), ranges, excludes, store, order } ... Small compiled-in named registry: Subaru table-based (first, for the target car), Nissan ALT/ALT2 with explicit dependency order, Motronic single/double, E38 with the zeroed-sum bug fixed.";

/// The legacy Java sources, relative to the repo root; [`Basis::Legacy`] and
/// [`LEGACY_CLASSES`] paths are relative to this.
const LEGACY_ROOT: &str = "legacy/src/main/java/com/romraider";

/// A source a planned variant is drawn from.
#[derive(Debug)]
enum Basis {
    /// A quote of [`US_RR_35`].
    Story(&'static str),
    /// A legacy routine: `file` under [`LEGACY_ROOT`], 1-based inclusive
    /// `lines`, and `text` that must appear on those lines.
    Legacy {
        file: &'static str,
        lines: (usize, usize),
        text: &'static str,
    },
}

/// One variant the checksum engine plans, as [`Spec::variant`] names it.
#[derive(Debug)]
struct Planned {
    variant: &'static str,
    why: &'static str,
    basis: &'static [Basis],
}

const SUM8: &str = "sum word=1 width=8 none";
const SUM_BYTES16_BE: &str = "sum word=1 width=16 none store=big";
const SUM16_BE: &str = "sum word=2 width=16 big none store=big";
const SUM16_LE: &str = "sum word=2 width=16 little none store=little";
const SUM16_BE_TWOS: &str = "sum word=2 width=16 big twos store=big";
const SUM16_BE_ONES: &str = "sum word=2 width=16 big ones store=big";
const SUM32_BE: &str = "sum word=4 width=32 big none store=big";
const SUM32_LE: &str = "sum word=4 width=32 little none store=little";
const SUM32_SUBARU: &str = "sum word=4 width=32 big target=0x5AA5A55A store=big";
const XOR8: &str = "xor word=1";
const XOR32_BE: &str = "xor word=4 big store=big";
const XOR32_LE: &str = "xor word=4 little store=big";
const CRC_ARC_BE: &str = "crc CRC-16/ARC store=big";
const CRC_ARC_LE: &str = "crc CRC-16/ARC store=little";
const CRC_IBM_3740: &str = "crc CRC-16/IBM-3740 store=big";
const CRC_XMODEM: &str = "crc CRC-16/XMODEM store=big";
const CRC_KERMIT: &str = "crc CRC-16/KERMIT store=big";
const CRC_MODBUS: &str = "crc CRC-16/MODBUS store=big";
const CRC_IBM_SDLC_BE: &str = "crc CRC-16/IBM-SDLC store=big";
const CRC_IBM_SDLC_LE: &str = "crc CRC-16/IBM-SDLC store=little";

/// Every sum, XOR and CRC variant the checksum engine plans: the authority
/// for US-RR-14's coverage criterion (US-RR-14-3). Each needs at least one
/// vector; a CRC needs its catalogue check vector.
const PLANNED: &[Planned] = &[
    Planned {
        variant: SUM8,
        why: "SSM and NCS frame checksums, Ecotrons ALM checksum: bytes summed, low byte kept",
        basis: &[
            Basis::Story("Sum{width,endian,init,negate}"),
            Basis::Legacy {
                file: "io/protocol/ssm/iso9141/SSMChecksumCalculator.java",
                lines: (31, 38),
                text: "total += asInt(b)",
            },
            Basis::Legacy {
                file: "io/protocol/ncs/iso14230/NCSChecksumCalculator.java",
                lines: (31, 38),
                text: "total += asInt(b)",
            },
            Basis::Legacy {
                file: "logger/external/ecotrons/io/AlmChecksumCalculator.java",
                lines: (31, 37),
                text: "cs += bytes.get(i)",
            },
        ],
    },
    Planned {
        variant: SUM_BYTES16_BE,
        why: "Motronic single/double: bytes summed into 16 bits from an initial value, stored big-endian; double chains a second range",
        basis: &[
            Basis::Story("Motronic single/double"),
            Basis::Legacy {
                file: "maps/checksum/ChecksumMOTRONICSINGLE.java",
                lines: (86, 93),
                text: "value += parseByteValue(binData, Settings.Endian.BIG, i, 1, false)",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumMOTRONICSINGLE.java",
                lines: (79, 80),
                text: "binData[loc] = (byte)((checksum >> 8) & 0xFF)",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumMOTRONICDOUBLE.java",
                lines: (69, 70),
                text: "calculate(checksum, binData, start2, end2)",
            },
        ],
    },
    Planned {
        variant: SUM16_BE,
        why: "Nissan ALT2 calibration and code sums: big-endian 16-bit words, stored big-endian",
        basis: &[
            Basis::Story("Nissan ALT/ALT2"),
            Basis::Legacy {
                file: "maps/checksum/CalculateALT2.java",
                lines: (61, 85),
                text: "sum += (short)parseByteValue(binData, Settings.Endian.BIG, i, 2, false)",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumALT2.java",
                lines: (85, 86),
                text: "parseIntegerValue(results.get(START), Settings.Endian.BIG, 2)",
            },
        ],
    },
    Planned {
        variant: SUM16_LE,
        why: "the `endian` parameter of a 16-bit word sum: little-endian words, stored little-endian",
        basis: &[Basis::Story("Sum{width,endian,init,negate}")],
    },
    Planned {
        variant: SUM16_BE_TWOS,
        why: "GM E38 segment sum: big-endian 16-bit words, two's complement stored big-endian",
        basis: &[
            Basis::Story("E38 with the zeroed-sum bug fixed"),
            Basis::Legacy {
                file: "maps/checksum/ChecksumE38PCM.java",
                lines: (213, 221),
                text: "sum = (((sum & 0xFFFF) ^ 0xFFFF) + 1) & 0xFFFF",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumE38PCM.java",
                lines: (91, 92),
                text: "binData[seg[i].start] = (byte) ((seg[i].csum >>> 8) & 0xFF)",
            },
        ],
    },
    Planned {
        variant: SUM16_BE_ONES,
        why: "the `negate` parameter's one's complement (E38 is the two's complement)",
        basis: &[Basis::Story("Sum{width,endian,init,negate}")],
    },
    Planned {
        variant: SUM32_BE,
        why: "Nissan STD/ALT/ALT2 `sumt`: big-endian 32-bit words, stored big-endian",
        basis: &[
            Basis::Story("Nissan ALT/ALT2"),
            Basis::Legacy {
                file: "maps/checksum/CalculateSTD.java",
                lines: (47, 54),
                text: "sumt += dw",
            },
            Basis::Legacy {
                file: "maps/checksum/CalculateALT2.java",
                lines: (49, 57),
                text: "sumt += dw",
            },
            Basis::Legacy {
                file: "maps/checksum/NissanChecksum.java",
                lines: (78, 78),
                text: "parseIntegerValue(results.get(SUMT), Settings.Endian.BIG, 4)",
            },
        ],
    },
    Planned {
        variant: SUM32_LE,
        why: "the `endian` parameter of a 32-bit word sum: little-endian words, stored little-endian",
        basis: &[Basis::Story("Sum{width,endian,init,negate}")],
    },
    Planned {
        variant: SUM32_SUBARU,
        why: "Subaru table checksum: big-endian 32-bit words plus the stored value total 0x5AA5A55A",
        basis: &[
            Basis::Story("Subaru table-based"),
            Basis::Legacy {
                file: "maps/RomChecksum.java",
                lines: (110, 116),
                text: "Settings.CHECK_TOTAL - byteSum",
            },
            Basis::Legacy {
                file: "Settings.java",
                lines: (131, 131),
                text: "CHECK_TOTAL = 0x5AA5A55A",
            },
        ],
    },
    Planned {
        variant: XOR8,
        why: "BMW byte XOR and the DS2 frame checksum",
        basis: &[
            Basis::Story("Xor"),
            Basis::Legacy {
                file: "maps/checksum/ChecksumBYTEXOR.java",
                lines: (77, 85),
                text: "xort ^= dw",
            },
            Basis::Legacy {
                file: "io/protocol/ds2/iso9141/DS2ChecksumCalculator.java",
                lines: (29, 35),
                text: "cs ^= bytes[i]",
            },
        ],
    },
    Planned {
        variant: XOR32_BE,
        why: "Nissan STD/ALT/ALT2 `xort`: big-endian 32-bit words, stored big-endian",
        basis: &[
            Basis::Story("Xor"),
            Basis::Legacy {
                file: "maps/checksum/CalculateSTD.java",
                lines: (51, 51),
                text: "xort ^= dw",
            },
            Basis::Legacy {
                file: "maps/checksum/NissanChecksum.java",
                lines: (79, 79),
                text: "parseIntegerValue(results.get(XORT), Settings.Endian.BIG, 4)",
            },
        ],
    },
    Planned {
        variant: XOR32_LE,
        why: "an XOR of little-endian 32-bit words (a spec's `endian`) stored big-endian: XOR has no carries, so little-endian stored little-endian is the same bytes as big/big, and only mixed orders differ",
        basis: &[Basis::Story("Xor")],
    },
    Planned {
        variant: CRC_ARC_LE,
        why: "GM E38 CVN: the reflected table of poly 0x8005 (CRC-16/ARC) chained over two ranges, byte-swapped, then stored big-endian, i.e. stored little-endian",
        basis: &[
            Basis::Story("Crc16"),
            Basis::Legacy {
                file: "maps/checksum/ChecksumE38PCM.java",
                lines: (162, 162),
                text: "0x0000, 0xC0C1",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumE38PCM.java",
                lines: (186, 192),
                text: "num5 = crc16t[temp1]",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumE38PCM.java",
                lines: (223, 228),
                text: "sum = swapab(sum)",
            },
            Basis::Legacy {
                file: "maps/checksum/ChecksumE38PCM.java",
                lines: (84, 85),
                text: "binData[seg[i].start + 0x1E] = (byte) ((seg[i].ccvn >>> 8) & 0xFF)",
            },
        ],
    },
    Planned {
        variant: CRC_ARC_BE,
        why: "CRC-16/ARC in the catalogue's own byte order, beside the E38 little-endian store",
        basis: &[Basis::Story("Crc16")],
    },
    Planned {
        variant: CRC_IBM_3740,
        why: "a `Crc16` parameter set with a non-zero INIT, MSB first (CCITT-FALSE)",
        basis: &[Basis::Story("Crc16")],
    },
    Planned {
        variant: CRC_XMODEM,
        why: "a `Crc16` parameter set with poly 0x1021, zero INIT, MSB first",
        basis: &[Basis::Story("Crc16")],
    },
    Planned {
        variant: CRC_KERMIT,
        why: "a `Crc16` parameter set with poly 0x1021, reflected",
        basis: &[Basis::Story("Crc16")],
    },
    Planned {
        variant: CRC_MODBUS,
        why: "a `Crc16` parameter set with poly 0x8005, non-zero INIT, reflected",
        basis: &[Basis::Story("Crc16")],
    },
    Planned {
        variant: CRC_IBM_SDLC_BE,
        why: "a `Crc16` parameter set with a final XOR (X.25)",
        basis: &[Basis::Story("Crc16")],
    },
    Planned {
        variant: CRC_IBM_SDLC_LE,
        why: "the NCS block CRC: CRC-16/MCRF4XX (register 0xFFFF, LSB first, poly 0x8408), complemented (= CRC-16/IBM-SDLC) and stored low byte first",
        basis: &[
            Basis::Story("Crc16"),
            Basis::Legacy {
                file: "maps/checksum/NcsCoDec.java",
                lines: (118, 134),
                text: "crc = crc ^ 0x8408",
            },
            Basis::Legacy {
                file: "io/j2534/api/TestJ2534NCS.java",
                lines: (573, 581),
                text: "final short invert = (short) ~crc",
            },
        ],
    },
];

/// Variants a fixture holds deliberately beyond the plan, and why. Anything
/// in a fixture that is neither here nor in [`PLANNED`] fails
/// [`every_vector_variant_is_planned`].
const EXTRA: &[(&str, &str)] = &[(
    "crc CRC-32/ISO-HDLC store=big",
    "US-RR-35 plans only `Crc16`; the CRC-32 check value pins the width-generic reference model above 16 bits",
)];

/// What a legacy checksum class is to the plan.
#[derive(Debug)]
enum Disposition {
    /// Its arithmetic is these planned variants.
    Mapped(&'static [&'static str]),
    /// It is not a sum, XOR or CRC the engine plans: why.
    OutOfScope(&'static str),
}

/// Every legacy checksum class (see
/// [`every_legacy_checksum_class_is_mapped_or_out_of_scope`]), by path under
/// [`LEGACY_ROOT`].
const LEGACY_CLASSES: &[(&str, Disposition)] = &[
    (
        "io/protocol/ds2/iso9141/DS2ChecksumCalculator.java",
        Disposition::Mapped(&[XOR8]),
    ),
    (
        "io/protocol/ncs/iso14230/NCSChecksumCalculator.java",
        Disposition::Mapped(&[SUM8]),
    ),
    (
        "io/protocol/ssm/iso9141/SSMChecksumCalculator.java",
        Disposition::Mapped(&[SUM8]),
    ),
    (
        "logger/external/ecotrons/io/AlmChecksumCalculator.java",
        Disposition::Mapped(&[SUM8]),
    ),
    (
        "maps/RomChecksum.java",
        Disposition::Mapped(&[SUM32_SUBARU]),
    ),
    (
        "maps/checksum/CalculateALT2.java",
        Disposition::Mapped(&[SUM16_BE, SUM32_BE, XOR32_BE]),
    ),
    (
        "maps/checksum/CalculateSTD.java",
        Disposition::Mapped(&[SUM32_BE, XOR32_BE]),
    ),
    (
        "maps/checksum/Calculator.java",
        Disposition::OutOfScope(
            "the interface the Nissan calculators implement; no arithmetic of its own",
        ),
    ),
    (
        "maps/checksum/ChecksumALT.java",
        Disposition::Mapped(&[SUM32_BE, XOR32_BE]),
    ),
    (
        "maps/checksum/ChecksumALT2.java",
        Disposition::Mapped(&[SUM16_BE, SUM32_BE, XOR32_BE]),
    ),
    (
        "maps/checksum/ChecksumBYTEXOR.java",
        Disposition::Mapped(&[XOR8]),
    ),
    (
        "maps/checksum/ChecksumCOPY.java",
        Disposition::OutOfScope(
            "copies a block to a second location and compares bytes (US-RR-35's `Copy`); no sum, XOR or CRC, so no reference value",
        ),
    ),
    (
        "maps/checksum/ChecksumE38PCM.java",
        Disposition::Mapped(&[SUM16_BE_TWOS, CRC_ARC_LE]),
    ),
    (
        "maps/checksum/ChecksumFactory.java",
        Disposition::OutOfScope(
            "loads a checksum class by name at run time, which US-RR-35 forbids (MF-11); no arithmetic",
        ),
    ),
    (
        "maps/checksum/ChecksumManager.java",
        Disposition::OutOfScope(
            "the interface every legacy checksum implements; no arithmetic of its own",
        ),
    ),
    (
        "maps/checksum/ChecksumMOTRONICDOUBLE.java",
        Disposition::Mapped(&[SUM_BYTES16_BE]),
    ),
    (
        "maps/checksum/ChecksumMOTRONICSINGLE.java",
        Disposition::Mapped(&[SUM_BYTES16_BE]),
    ),
    (
        "maps/checksum/ChecksumSTD.java",
        Disposition::Mapped(&[SUM32_BE, XOR32_BE]),
    ),
    (
        // nisEncode/nisDecode are a cipher, not a checksum; their vectors are
        // fixtures/kat/ncs-codec-legacy.toml (US-RR-14-8). calcCrc is mapped.
        "maps/checksum/NcsCoDec.java",
        Disposition::Mapped(&[CRC_IBM_SDLC_LE]),
    ),
    (
        "maps/checksum/NissanChecksum.java",
        Disposition::Mapped(&[SUM32_BE, XOR32_BE]),
    ),
    (
        "util/MD5Checksum.java",
        Disposition::OutOfScope(
            "an MD5 digest of a whole file (java.security.MessageDigest), not a checksum stored in and fixed on a ROM",
        ),
    ),
];

/// One `[[checksum]]` case of a vector fixture, with its fixture's input.
struct Vector {
    file: String,
    number: usize,
    input: Vec<u8>,
    case: ChecksumCase,
}

/// Every `[[checksum]]` case of every `fixtures/kat/checksum-*.toml`.
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn vectors() -> Vec<Vector> {
    let dir = repo_root().join("fixtures").join("kat");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|ext| ext == "toml")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(PREFIX))
        })
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let fixture = kat::load(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
        let KatKind::Checksum { cases, .. } = fixture.data.kind else {
            panic!("{file}: a {PREFIX}*.toml fixture must be a [[checksum]] fixture");
        };
        let Some(Input::Hex(hex)) = fixture.data.input else {
            panic!("{file}: a checksum vector needs an inline `hex` input");
        };
        for (index, case) in cases.into_iter().enumerate() {
            out.push(Vector {
                file: file.clone(),
                number: index + 1,
                input: hex.as_bytes().to_vec(),
                case,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The reference implementation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Big,
    Little,
}

impl Endian {
    fn name(self) -> &'static str {
        match self {
            Self::Big => "big",
            Self::Little => "little",
        }
    }

    /// `bytes` (at most 8) read as an unsigned integer in this byte order.
    fn read(self, bytes: &[u8]) -> u64 {
        let fold = |acc: u64, &b: &u8| (acc << 8) | u64::from(b);
        match self {
            Self::Big => bytes.iter().fold(0, fold),
            Self::Little => bytes.iter().rev().fold(0, fold),
        }
    }

    /// The low `len` bytes of `value` in this byte order.
    fn write(self, value: u64, len: usize) -> Vec<u8> {
        match self {
            Self::Big => value.to_be_bytes()[8 - len..].to_vec(),
            Self::Little => value.to_le_bytes()[..len].to_vec(),
        }
    }
}

/// What a sum stores: the sum itself, a complement of it, or what brings the
/// total to a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Negate {
    None,
    Ones,
    Twos,
    Target(u64),
}

#[derive(Debug)]
enum Algo {
    /// `algorithm = "sum"`: words added mod 2^width, from `init`.
    Sum {
        word: usize,
        endian: Endian,
        width: u32,
        init: u64,
        negate: Negate,
    },
    /// `algorithm = "xor"`: words XORed, from 0.
    Xor { word: usize, endian: Endian },
    /// `algorithm = "crc"`: the Rocksoft model.
    Crc {
        catalogue: String,
        width: u32,
        poly: u64,
        init: u64,
        refin: bool,
        refout: bool,
        xorout: u64,
    },
}

#[derive(Debug)]
struct Spec {
    algo: Algo,
    start: usize,
    end: usize,
    pad: bool,
    store: Endian,
}

/// `2^width - 1`.
fn mask(width: u32) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1 << width) - 1
    }
}

/// `value`'s low `width` bits in reverse order.
fn reflect(value: u64, width: u32) -> u64 {
    value.reverse_bits() >> (64 - width)
}

/// The Rocksoft model, bit by bit, MSB first (Williams 1993 section 15):
/// each input byte (reflected when `refin`) is XORed into the top of the
/// register, which is shifted left 8 times, XORing `poly` in whenever a 1
/// leaves the top; the final register is reflected when `refout`, then
/// XORed with `xorout`.
fn crc(
    width: u32,
    poly: u64,
    init: u64,
    refin: bool,
    refout: bool,
    xorout: u64,
    data: &[u8],
) -> u64 {
    let mask = mask(width);
    let top = 1_u64 << (width - 1);
    let mut reg = init & mask;
    for &byte in data {
        let byte = if refin { byte.reverse_bits() } else { byte };
        reg ^= u64::from(byte) << (width - 8);
        for _ in 0..8 {
            reg = if reg & top == 0 {
                reg << 1
            } else {
                (reg << 1) ^ poly
            } & mask;
        }
    }
    if refout {
        reg = reflect(reg, width);
    }
    (reg ^ xorout) & mask
}

/// `bytes` split into `word`-byte words read in `endian` order; a last
/// partial word is completed with 0x00 bytes at the higher addresses when
/// `pad`, and is an error otherwise.
fn words(bytes: &[u8], word: usize, endian: Endian, pad: bool) -> Result<Vec<u64>, String> {
    if !bytes.len().is_multiple_of(word) && !pad {
        return Err(format!(
            "the region is {} byte(s), not a whole number of {word}-byte words, and has no `pad`",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks(word)
        .map(|chunk| {
            let mut padded = vec![0_u8; word];
            padded[..chunk.len()].copy_from_slice(chunk);
            endian.read(&padded)
        })
        .collect())
}

impl Spec {
    /// The stored checksum's width in bytes.
    fn store_len(&self) -> usize {
        match self.algo {
            Algo::Sum { width, .. } | Algo::Crc { width, .. } => (width / 8) as usize,
            Algo::Xor { word, .. } => word,
        }
    }

    /// The variant this spec is, as [`PLANNED`] names it: the algorithm and
    /// its parameters, then ` store=<order>` when the stored value is wider
    /// than a byte (its byte order then matters).
    fn variant(&self) -> String {
        let store = if self.store_len() > 1 {
            format!(" store={}", self.store.name())
        } else {
            String::new()
        };
        let algo = match &self.algo {
            Algo::Sum {
                word,
                endian,
                width,
                negate,
                ..
            } => {
                let endian = if *word == 1 {
                    String::new()
                } else {
                    format!(" {}", endian.name())
                };
                let negate = match negate {
                    Negate::None => "none".to_owned(),
                    Negate::Ones => "ones".to_owned(),
                    Negate::Twos => "twos".to_owned(),
                    Negate::Target(t) => format!("target=0x{t:X}"),
                };
                format!("sum word={word} width={width}{endian} {negate}")
            }
            Algo::Xor { word, endian } => {
                if *word == 1 {
                    "xor word=1".to_owned()
                } else {
                    format!("xor word={word} {}", endian.name())
                }
            }
            Algo::Crc { catalogue, .. } => format!("crc {catalogue}"),
        };
        algo + &store
    }

    /// The checksum of `region`, and whether a sum wrapped (its exact total
    /// did not fit in `width` bits).
    fn compute(&self, region: &[u8]) -> Result<(u64, bool), String> {
        Ok(match &self.algo {
            Algo::Sum {
                word,
                endian,
                width,
                init,
                negate,
            } => {
                let total: u128 = words(region, *word, *endian, self.pad)?
                    .into_iter()
                    .map(u128::from)
                    .sum::<u128>()
                    + u128::from(*init);
                let mask = mask(*width);
                let wrapped = total > u128::from(mask);
                let sum = (total & u128::from(mask)) as u64;
                let stored = match negate {
                    Negate::None => sum,
                    Negate::Ones => !sum & mask,
                    Negate::Twos => sum.wrapping_neg() & mask,
                    Negate::Target(target) => target.wrapping_sub(sum) & mask,
                };
                (stored, wrapped)
            }
            Algo::Xor { word, endian } => (
                words(region, *word, *endian, self.pad)?
                    .into_iter()
                    .fold(0, |acc, w| acc ^ w),
                false,
            ),
            Algo::Crc {
                width,
                poly,
                init,
                refin,
                refout,
                xorout,
                ..
            } => (
                crc(*width, *poly, *init, *refin, *refout, *xorout, region),
                false,
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// A case's `params`, read key by key; [`Params::finish`] rejects any key
/// never read, so a misspelt parameter cannot be silently ignored.
struct Params<'a> {
    table: &'a toml::Table,
    read: BTreeSet<&'static str>,
}

impl<'a> Params<'a> {
    fn new(table: &'a toml::Table) -> Self {
        Self {
            table,
            read: BTreeSet::new(),
        }
    }

    fn get(&mut self, key: &'static str) -> Option<&'a toml::Value> {
        self.read.insert(key);
        self.table.get(key)
    }

    fn int(&mut self, key: &'static str) -> Result<Option<u64>, String> {
        self.get(key)
            .map(|value| {
                value
                    .as_integer()
                    .and_then(|i| u64::try_from(i).ok())
                    .ok_or_else(|| format!("`{key}` must be a non-negative integer, not {value}"))
            })
            .transpose()
    }

    fn required_int(&mut self, key: &'static str) -> Result<u64, String> {
        self.int(key)?.ok_or_else(|| format!("missing `{key}`"))
    }

    fn usize(&mut self, key: &'static str) -> Result<usize, String> {
        usize::try_from(self.required_int(key)?).map_err(|e| format!("`{key}`: {e}"))
    }

    fn str(&mut self, key: &'static str) -> Result<Option<&'a str>, String> {
        self.get(key)
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| format!("`{key}` must be a string, not {value}"))
            })
            .transpose()
    }

    fn bool(&mut self, key: &'static str) -> Result<bool, String> {
        let value = self.get(key).ok_or_else(|| format!("missing `{key}`"))?;
        value
            .as_bool()
            .ok_or_else(|| format!("`{key}` must be true or false, not {value}"))
    }

    fn endian(&mut self, key: &'static str) -> Result<Endian, String> {
        match self.str(key)? {
            Some("big") => Ok(Endian::Big),
            Some("little") => Ok(Endian::Little),
            Some(other) => Err(format!(
                "`{key}` must be \"big\" or \"little\", not {other:?}"
            )),
            None => Err(format!("missing `{key}`")),
        }
    }

    fn finish(self) -> Result<(), String> {
        let unknown: Vec<&str> = self
            .table
            .keys()
            .map(String::as_str)
            .filter(|key| !self.read.contains(key))
            .collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!("unknown parameter(s) {unknown:?}"))
        }
    }
}

/// The word size and byte order of a sum or XOR: `endian` is required for a
/// multi-byte word and not allowed for a byte.
fn word_and_endian(params: &mut Params<'_>) -> Result<(usize, Endian), String> {
    let word = params.usize("word")?;
    if !matches!(word, 1 | 2 | 4) {
        return Err(format!("`word` must be 1, 2 or 4, not {word}"));
    }
    let endian = if word == 1 {
        if params.get("endian").is_some() {
            return Err("`endian` means nothing for a 1-byte word".to_owned());
        }
        Endian::Big
    } else {
        params.endian("endian")?
    };
    Ok((word, endian))
}

fn parse(case: &ChecksumCase) -> Result<Spec, String> {
    let mut params = Params::new(&case.params);
    let start = params.usize("start")?;
    let end = params.usize("end")?;
    let store = params.endian("store")?;
    let pad = match params.str("pad")? {
        None => false,
        Some("zero") => true,
        Some(other) => return Err(format!("`pad` must be \"zero\", not {other:?}")),
    };
    let algo = match case.algorithm.as_str() {
        "sum" => {
            let (word, endian) = word_and_endian(&mut params)?;
            let width = u32::try_from(params.required_int("width")?).map_err(|e| e.to_string())?;
            if !matches!(width, 8 | 16 | 32) || width < 8 * word as u32 {
                return Err(format!(
                    "sum `width` {width} must be 8, 16 or 32 and hold a word"
                ));
            }
            let init = params.int("init")?.unwrap_or(0);
            let target = params.int("target")?;
            let negate = match (params.str("negate")?, target) {
                (None | Some("none"), None) => Negate::None,
                (Some("ones"), None) => Negate::Ones,
                (Some("twos"), None) => Negate::Twos,
                (None, Some(target)) => Negate::Target(target),
                (Some(_), Some(_)) => return Err("`negate` and `target` are exclusive".to_owned()),
                (Some(other), None) => {
                    return Err(format!(
                        "`negate` must be none, ones or twos, not {other:?}"
                    ));
                }
            };
            for (key, value) in [("init", init), ("target", target.unwrap_or(0))] {
                if value > mask(width) {
                    return Err(format!("`{key}` 0x{value:X} does not fit in {width} bits"));
                }
            }
            Algo::Sum {
                word,
                endian,
                width,
                init,
                negate,
            }
        }
        "xor" => {
            let (word, endian) = word_and_endian(&mut params)?;
            Algo::Xor { word, endian }
        }
        "crc" => {
            let catalogue = params
                .str("catalogue")?
                .ok_or("missing `catalogue`")?
                .to_owned();
            let width = u32::try_from(params.required_int("width")?).map_err(|e| e.to_string())?;
            if !matches!(width, 8 | 16 | 24 | 32) {
                return Err(format!("crc `width` {width} must be 8, 16, 24 or 32"));
            }
            let poly = params.required_int("poly")?;
            let init = params.required_int("init")?;
            let refin = params.bool("refin")?;
            let refout = params.bool("refout")?;
            let xorout = params.required_int("xorout")?;
            for (key, value) in [("poly", poly), ("init", init), ("xorout", xorout)] {
                if value > mask(width) {
                    return Err(format!("`{key}` 0x{value:X} does not fit in {width} bits"));
                }
            }
            Algo::Crc {
                catalogue,
                width,
                poly,
                init,
                refin,
                refout,
                xorout,
            }
        }
        other => return Err(format!("unknown algorithm {other:?} (sum, xor or crc)")),
    };
    params.finish()?;
    Ok(Spec {
        algo,
        start,
        end,
        pad,
        store,
    })
}

// ---------------------------------------------------------------------------
// Checking a vector
// ---------------------------------------------------------------------------

/// What a passing vector covers.
struct Covered {
    variant: String,
    family: String,
    empty: bool,
    all_ff: bool,
    wrapped: bool,
    padded_word: Option<usize>,
}

/// Checks `vector` against the reference implementation.
fn check(vector: &Vector) -> Result<Covered, String> {
    let case = &vector.case;
    let spec = parse(case)?;
    let len = spec.store_len();
    let input = &vector.input;
    let offset = usize::try_from(case.offset).map_err(|e| e.to_string())?;
    let stored_end = offset
        .checked_add(len)
        .filter(|&e| e <= input.len())
        .ok_or_else(|| {
            format!("the {len}-byte stored checksum at {offset} is outside the input")
        })?;
    if spec.start > spec.end || spec.end > input.len() {
        return Err(format!(
            "region [{}, {}) is not inside the {}-byte input",
            spec.start,
            spec.end,
            input.len()
        ));
    }
    if spec.start < stored_end && offset < spec.end {
        return Err(format!(
            "region [{}, {}) overlaps the stored checksum [{offset}, {stored_end})",
            spec.start, spec.end
        ));
    }
    let region = &input[spec.start..spec.end];
    let (value, wrapped) = spec.compute(region)?;
    let expected = spec.store.write(value, len);
    let given = &input[offset..stored_end];
    let before = case.before.hex.as_bytes();
    let after = case.after.hex.as_bytes();
    if before != given {
        return Err(format!(
            "`before.hex` {before:02X?} is not the input's stored bytes {given:02X?}"
        ));
    }
    if after != expected.as_slice() {
        return Err(format!(
            "`after.hex` is {after:02X?}; the reference gives 0x{value:X}, stored {} as {expected:02X?}",
            spec.store.name()
        ));
    }
    if !case.after.valid {
        return Err("`after.valid` is false, but a fixed checksum verifies".to_owned());
    }
    if case.before.valid != (before == after) {
        return Err(format!(
            "`before.valid` is {}, but the input as given {} the result",
            case.before.valid,
            if before == after {
                "holds"
            } else {
                "does not hold"
            }
        ));
    }
    let padded_word = match spec.algo {
        Algo::Sum { word, .. } | Algo::Xor { word, .. } if !region.len().is_multiple_of(word) => {
            Some(word)
        }
        _ => None,
    };
    Ok(Covered {
        variant: spec.variant(),
        family: case.algorithm.clone(),
        empty: region.is_empty(),
        all_ff: !region.is_empty() && region.iter().all(|&b| b == 0xFF),
        wrapped,
        padded_word,
    })
}

#[test]
fn every_vector_matches_the_reference() {
    let vectors = vectors();
    assert!(
        vectors.len() >= PLANNED.len(),
        "only {} checksum vectors in fixtures/kat/{PREFIX}*.toml",
        vectors.len()
    );
    let failures: Vec<String> = vectors
        .iter()
        .filter_map(|v| {
            check(v)
                .err()
                .map(|why| format!("{} [[checksum]] #{}: {why}", v.file, v.number))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What `planned` lacks among the `covered` vectors: a planned variant with
/// no vector, and a family without an empty-range, all-0xFF, wrapping (sums)
/// or odd-length (word sums) vector.
fn coverage_gaps(covered: &[Covered], planned: &[Planned]) -> Vec<String> {
    let variants: BTreeSet<&str> = covered.iter().map(|c| c.variant.as_str()).collect();
    let mut missing: Vec<String> = planned
        .iter()
        .filter(|p| !variants.contains(p.variant))
        .map(|p| format!("no vector for planned variant {:?} ({})", p.variant, p.why))
        .collect();
    for family in ["sum", "xor", "crc"] {
        let of = || covered.iter().filter(move |c| c.family == family);
        if !of().any(|c| c.empty) {
            missing.push(format!("no empty-range {family} vector"));
        }
        if !of().any(|c| c.all_ff) {
            missing.push(format!("no all-0xFF {family} vector"));
        }
    }
    if !covered.iter().any(|c| c.family == "sum" && c.wrapped) {
        missing.push("no sum vector that wraps".to_owned());
    }
    for word in [2, 4] {
        if !covered
            .iter()
            .any(|c| c.family == "sum" && c.padded_word == Some(word))
        {
            missing.push(format!("no odd-length {word}-byte-word sum vector"));
        }
    }
    missing
}

/// The variants among `covered` that are neither in `planned` nor in `extra`.
fn unplanned<'c>(
    covered: &'c [Covered],
    planned: &[Planned],
    extra: &[(&str, &str)],
) -> Vec<&'c str> {
    covered
        .iter()
        .map(|c| c.variant.as_str())
        .filter(|v| !planned.iter().any(|p| p.variant == *v) && !extra.iter().any(|(e, _)| e == v))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Every vector that passes [`check`], with what it covers.
fn covered() -> Vec<Covered> {
    vectors().iter().filter_map(|v| check(v).ok()).collect()
}

#[test]
fn every_planned_variant_has_a_vector() {
    let missing = coverage_gaps(&covered(), PLANNED);
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn every_vector_variant_is_planned() {
    let covered = covered();
    let unplanned = unplanned(&covered, PLANNED, EXTRA);
    assert!(
        unplanned.is_empty(),
        "fixture variant(s) {unplanned:?} are not in PLANNED or EXTRA; add each to PLANNED with its US-RR-35 / legacy source, or to EXTRA with a reason"
    );
    for (variant, why) in EXTRA {
        assert!(!why.is_empty(), "EXTRA {variant:?} needs a reason");
        assert!(
            !PLANNED.iter().any(|p| p.variant == *variant),
            "{variant:?} is in both PLANNED and EXTRA"
        );
        assert!(
            covered.iter().any(|c| c.variant == *variant),
            "EXTRA {variant:?} has no vector; drop it"
        );
    }
}

#[test]
fn the_coverage_checks_catch_a_missing_or_unplanned_variant() {
    let covered = covered();
    let bogus = [Planned {
        variant: "sum word=4 width=32 big ones store=big",
        why: "a variant no fixture holds",
        basis: &[Basis::Story("negate")],
    }];
    let gaps = coverage_gaps(&covered, &bogus);
    assert!(
        gaps.iter()
            .any(|g| g.contains("sum word=4 width=32 big ones store=big")),
        "{gaps:?}"
    );
    // With PLANNED minus its first entry, that entry's vectors are unplanned.
    let first = PLANNED[0].variant;
    assert_eq!(unplanned(&covered, &PLANNED[1..], EXTRA), [first]);
    assert!(unplanned(&covered, PLANNED, &[]).contains(&EXTRA[0].0));
}

/// `lines` (1-based, inclusive) of `file` under [`LEGACY_ROOT`], or why not.
fn legacy_lines(file: &str, (first, last): (usize, usize)) -> Result<String, String> {
    let path = repo_root().join(LEGACY_ROOT).join(file);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    if first == 0 || first > last || last > lines.len() {
        return Err(format!(
            "{file}:{first}-{last} is not a line range of its {} lines",
            lines.len()
        ));
    }
    Ok(lines[first - 1..last].join("\n"))
}

#[test]
fn planned_variants_cite_their_sources() {
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    for planned in PLANNED {
        if !seen.insert(planned.variant) {
            failures.push(format!("{:?} is planned twice", planned.variant));
        }
        if planned.basis.is_empty() || planned.why.is_empty() {
            failures.push(format!("{:?} needs a reason and a source", planned.variant));
        }
        for basis in planned.basis {
            match basis {
                Basis::Story(quote) => {
                    if quote.is_empty() || !US_RR_35.contains(quote) {
                        failures.push(format!(
                            "{:?}: {quote:?} is not quoted from US-RR-35",
                            planned.variant
                        ));
                    }
                }
                Basis::Legacy { file, lines, text } => match legacy_lines(file, *lines) {
                    Ok(cited) if cited.contains(text) => {}
                    Ok(_) => failures.push(format!(
                        "{:?}: {file}:{}-{} does not contain {text:?}",
                        planned.variant, lines.0, lines.1
                    )),
                    Err(why) => failures.push(format!("{:?}: {why}", planned.variant)),
                },
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // The check rejects a wrong line range and wrong text.
    assert!(legacy_lines("maps/RomChecksum.java", (110, 100_000)).is_err());
    assert!(
        legacy_lines("maps/RomChecksum.java", (110, 116))
            .is_ok_and(|t| t.contains("CHECK_TOTAL") && !t.contains("xort ^= dw"))
    );
}

/// Every `.java` file under [`LEGACY_ROOT`] in a `checksum` directory or
/// named `*Checksum*`, as a `/`-separated path under [`LEGACY_ROOT`].
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn legacy_checksum_classes() -> BTreeSet<String> {
    fn walk(dir: &Path, rel: &str, out: &mut BTreeSet<String>) {
        let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &rel, out);
            } else if name.ends_with(".java")
                && (rel.split('/').any(|part| part == "checksum") || name.contains("Checksum"))
            {
                out.insert(rel);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(&repo_root().join(LEGACY_ROOT), "", &mut out);
    out
}

#[test]
fn every_legacy_checksum_class_is_mapped_or_out_of_scope() {
    let found = legacy_checksum_classes();
    let listed: BTreeSet<String> = LEGACY_CLASSES
        .iter()
        .map(|(f, _)| (*f).to_owned())
        .collect();
    assert!(
        found.len() >= 15,
        "only {} legacy checksum classes found under {LEGACY_ROOT}",
        found.len()
    );
    let unlisted: Vec<&String> = found.difference(&listed).collect();
    let gone: Vec<&String> = listed.difference(&found).collect();
    assert!(
        unlisted.is_empty(),
        "legacy checksum class(es) {unlisted:?} are not in LEGACY_CLASSES"
    );
    assert!(
        gone.is_empty(),
        "LEGACY_CLASSES lists missing file(s) {gone:?}"
    );
    assert_eq!(
        listed.len(),
        LEGACY_CLASSES.len(),
        "LEGACY_CLASSES has a duplicate"
    );
    for (file, disposition) in LEGACY_CLASSES {
        match disposition {
            Disposition::Mapped(variants) => {
                assert!(!variants.is_empty(), "{file}: mapped to nothing");
                for variant in *variants {
                    assert!(
                        PLANNED.iter().any(|p| p.variant == *variant),
                        "{file}: mapped to {variant:?}, which is not planned"
                    );
                }
            }
            Disposition::OutOfScope(why) => {
                assert!(!why.is_empty(), "{file}: out of scope with no reason");
            }
        }
    }
}

/// Why a CRC check-value vector's `after` does not cite the catalogue entry
/// of its own parameters, or `None` when it does.
fn citation_problem(vector: &Vector) -> Option<String> {
    let spec = parse(&vector.case).ok()?;
    let Algo::Crc {
        catalogue,
        width,
        poly,
        init,
        refin,
        refout,
        xorout,
    } = &spec.algo
    else {
        return None;
    };
    if vector.input.get(spec.start..spec.end) != Some(CHECK_INPUT) {
        return None; // not a check-value vector: worked by hand
    }
    let Source::Standard { doc, section } = &vector.case.after.source else {
        return Some(
            "a CRC check value must be a `standard` source citing the catalogue".to_owned(),
        );
    };
    if !doc.contains("reveng.sourceforge.io/crc-catalogue") || !doc.contains("retrieved") {
        return Some(format!(
            "`doc` {doc:?} must name the CRC RevEng catalogue page and its retrieval date"
        ));
    }
    let digits = (*width / 4) as usize;
    let value = spec.compute(CHECK_INPUT).ok()?.0;
    let wanted = [
        catalogue.clone(),
        format!("width={width}"),
        format!("poly=0x{poly:0digits$x}"),
        format!("init=0x{init:0digits$x}"),
        format!("refin={refin}"),
        format!("refout={refout}"),
        format!("xorout=0x{xorout:0digits$x}"),
        format!("check=0x{value:0digits$x}"),
    ];
    let absent: Vec<&String> = wanted.iter().filter(|w| !section.contains(*w)).collect();
    (!absent.is_empty()).then(|| format!("`section` {section:?} lacks {absent:?}"))
}

#[test]
fn crc_check_values_cite_their_catalogue_entry() {
    let vectors = vectors();
    let failures: Vec<String> = vectors
        .iter()
        .filter_map(|v| {
            citation_problem(v).map(|why| format!("{} [[checksum]] #{}: {why}", v.file, v.number))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_planned_crc_has_its_catalogue_check_vector() {
    let vectors = vectors();
    let missing: Vec<&str> = PLANNED
        .iter()
        .map(|p| p.variant)
        .filter(|variant| variant.starts_with("crc "))
        .filter(|variant| {
            !vectors.iter().any(|v| {
                parse(&v.case).is_ok_and(|spec| {
                    spec.variant() == *variant
                        && v.input.get(spec.start..spec.end) == Some(CHECK_INPUT)
                }) && check(v).is_ok()
                    && citation_problem(v).is_none()
            })
        })
        .collect();
    assert!(
        missing.is_empty(),
        "planned CRC(s) {missing:?} have no catalogue check vector for \"123456789\""
    );
}

#[test]
fn a_wrong_stored_value_or_validity_is_reported() {
    let vectors = vectors();
    let mut vector = vectors
        .into_iter()
        .find(|v| v.case.algorithm == "crc" && !v.case.before.valid)
        .unwrap();
    assert!(check(&vector).is_ok());
    vector.case.after.hex.0[0] ^= 0x01;
    assert!(check(&vector).is_err_and(|e| e.contains("after.hex")));
    vector.case.after.hex.0[0] ^= 0x01;
    vector.case.before.valid = true;
    assert!(check(&vector).is_err_and(|e| e.contains("before.valid")));
    vector.case.before.valid = false;
    vector
        .case
        .params
        .insert("polly".to_owned(), toml::Value::Integer(1));
    assert!(check(&vector).is_err_and(|e| e.contains("unknown parameter")));
}

#[test]
fn reference_crc_matches_the_model_on_a_known_table_entry() {
    // Independent of the fixtures: entry 1 of the reflected table of poly
    // 0x8005 is 0xC0C1 (the CRC-16/ARC of the single byte 0x01, worked by
    // shifting 0x0001 right eight times with the reflected poly 0xA001), and
    // entry 1 of the MSB-first table of poly 0x1021 is 0x1021 (CRC-16/XMODEM
    // of 0x01: 0x0100 shifted left eight times, the 1 leaving the top on the
    // last shift).
    assert_eq!(crc(16, 0x8005, 0, true, true, 0, &[0x01]), 0xC0C1);
    assert_eq!(crc(16, 0x1021, 0, false, false, 0, &[0x01]), 0x1021);
    assert_eq!(reflect(0x8005, 16), 0xA001);
}
