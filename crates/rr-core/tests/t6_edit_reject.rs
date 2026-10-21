//! T6 safety invariant (US-RR-24-11; EC-08, MF-05): a value typed into a
//! cell is written as exactly the raw value
//! [`Conversion::to_raw_checked`] gives, or refused with a [`Reject`], and a
//! refusal leaves every byte of the image as it was. Nothing is wrapped or
//! clamped.
//!
//! rr-core never touches a ROM image; the editor (rr-rom, with its
//! capability-gated write path) composes the two pure steps this test
//! composes: `to_raw_checked` on the conversion, then, only on `Ok`,
//! [`Encoding::encode_into`] on the cell's bytes. rr-rom has no cell-write
//! API taking a real value yet; when the editor story adds one, it gets the
//! same invariant on the journalled image. Until then the boundary is held
//! here, and by a source scan that keeps wrapping and clamping arithmetic
//! out of the conversion module.

// In a `cfg(test)` module so the workspace's unwrap/expect/panic-in-tests
// allowance (clippy.toml) covers the helpers too.
#[cfg(test)]
mod t6 {
    use std::fs;
    use std::path::{Path, PathBuf};

    use proptest::prelude::*;
    use rr_core::conversion::{
        Conversion, ConversionSpec, FormSpec, NonFinite, NotInvertible, Reject,
    };
    use rr_core::encoding::{BitField, Encoding, Endian, Layout, Raw, Storage};

    fn conv(to_real: &str) -> Conversion {
        Conversion::from_spec(&ConversionSpec {
            form: FormSpec::Expr {
                to_real,
                to_raw: None,
            },
            unit: None,
            format: None,
        })
        .unwrap()
    }

    /// What an edit did: the raw value written, or the refusal.
    type Edit = Result<Raw, Reject>;

    /// A refusal case: formula, encoding, value, and the refusal expected.
    type Case = (&'static str, Encoding, f64, fn(&Reject) -> bool);

    /// Types `target` into the cell at `offset` of `image`: the editor's two
    /// steps. The cell's bytes are written only when the conversion accepts the
    /// value.
    fn edit(c: &Conversion, enc: Encoding, target: f64, image: &mut [u8], offset: usize) -> Edit {
        let raw = c.to_raw_checked(target, enc.raw_range().unwrap())?;
        enc.encode_into(raw, &mut image[offset..]).unwrap();
        Ok(raw)
    }

    fn plain(storage: Storage, endian: Endian) -> Encoding {
        Encoding::new(storage, endian, None, Layout::Default).unwrap()
    }

    /// An image with a recognisable pattern, so a stray write shows.
    fn image() -> Vec<u8> {
        (0..64_u8).map(|i| i.wrapping_mul(37) ^ 0xA5).collect()
    }

    #[test]
    fn t6_every_rejection_leaves_the_image_untouched() {
        let u32be = plain(Storage::U32, Endian::Big);
        let u8 = plain(Storage::U8, Endian::Big);
        let cases: [Case; 6] = [
            // OutOfRange on 4-byte storage: 2^32 would wrap to 0 under a
            // Java (int) cast; legacy then clamped.
            ("x", u32be, 4_294_967_296.0, |e| {
                matches!(e, Reject::OutOfRange { .. })
            }),
            ("x*0.5", u8, -1.0, |e| {
                matches!(e, Reject::OutOfRange { .. })
            }),
            // NotInvertible: turns, and a pole inside the range.
            ("(x-128)^2", u8, 4.0, |e| {
                matches!(e, Reject::NotInvertible(NotInvertible::NotMonotonic { .. }))
            }),
            ("1/(x-10)", u8, 1e6, |e| {
                matches!(e, Reject::NotInvertible(NotInvertible::Gap { .. }))
            }),
            // Singular.
            ("x*0+5", u8, 5.0, |e| matches!(e, Reject::Singular { .. })),
            // NonFinite: only reachable past the pole at the top raw value.
            ("1/(x-4294967295)", u32be, -1e12, |e| {
                matches!(e, Reject::NonFinite(NonFinite::Beyond { .. }))
            }),
        ];
        for (formula, enc, target, want) in cases {
            let mut img = image();
            let before = img.clone();
            let got = edit(&conv(formula), enc, target, &mut img, 8);
            let err = got.unwrap_err();
            assert!(want(&err), "{formula} at {target}: {err:?}");
            assert_eq!(img, before, "{formula} at {target}: bytes changed");
        }
        for t in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut img = image();
            let got = edit(&conv("x"), u32be, t, &mut img, 8);
            assert!(matches!(got, Err(Reject::NonFinite(NonFinite::Target(_)))));
            assert_eq!(img, image());
        }
    }

    #[test]
    fn t6_an_accepted_value_writes_only_its_own_cell() {
        let mut img = image();
        let enc = plain(Storage::U32, Endian::Little);
        assert_eq!(
            edit(&conv("x"), enc, 4_294_967_295.0, &mut img, 8),
            Ok(Raw::Int(4_294_967_295))
        );
        let mut want = image();
        want[8..12].copy_from_slice(&[0xFF; 4]);
        assert_eq!(img, want);
    }

    // ---------------------------------------------------------------------------
    // EC-08 over every storage: beyond either end, and through a division by zero
    // ---------------------------------------------------------------------------

    /// Every plain storage, both byte orders.
    fn every_plain_encoding() -> Vec<Encoding> {
        let storages = [
            Storage::U8,
            Storage::I8,
            Storage::U16,
            Storage::I16,
            Storage::U32,
            Storage::I32,
            Storage::F32,
            Storage::F64,
        ];
        storages
            .into_iter()
            .flat_map(|s| [Endian::Big, Endian::Little].map(|e| plain(s, e)))
            .collect()
    }

    /// The lowest and highest raw values `enc` can store, and the raw values
    /// one step inside each.
    fn ends(enc: Encoding) -> [Raw; 4] {
        match enc.storage() {
            Storage::F32 => [
                Raw::F32(-f32::MAX),
                Raw::F32((-f32::MAX).next_up()),
                Raw::F32(f32::MAX.next_down()),
                Raw::F32(f32::MAX),
            ],
            Storage::F64 => [
                Raw::F64(-f64::MAX),
                Raw::F64((-f64::MAX).next_up()),
                Raw::F64(f64::MAX.next_down()),
                Raw::F64(f64::MAX),
            ],
            s => {
                let (lo, hi) = s.int_range().unwrap();
                [
                    Raw::Int(lo),
                    Raw::Int(lo + 1),
                    Raw::Int(hi - 1),
                    Raw::Int(hi),
                ]
            }
        }
    }

    /// A raw value well inside the range of `enc`, held by the edited cell
    /// before the edit, so writing either end (a clamp) would show.
    fn middle(enc: Encoding) -> Raw {
        match enc.storage() {
            Storage::F32 => Raw::F32(1.5),
            Storage::F64 => Raw::F64(1.5),
            s => {
                let (lo, hi) = s.int_range().unwrap();
                Raw::Int(lo + (hi - lo) / 2)
            }
        }
    }

    /// A ROM image around a table of `enc` cells: the pattern of [`image`]
    /// as header and trailer, then the lowest raw value, the edited cell
    /// (holding [`middle`]) and the highest raw value side by side. The
    /// offset of the edited cell.
    fn table(enc: Encoding) -> (Vec<u8>, usize) {
        let mut img = image();
        let w = enc.storage().cell_bytes();
        let [lo, _, _, hi] = ends(enc);
        let at = 16;
        enc.encode_into(lo, &mut img[at - w..]).unwrap();
        enc.encode_into(middle(enc), &mut img[at..]).unwrap();
        enc.encode_into(hi, &mut img[at + w..]).unwrap();
        (img, at)
    }

    /// Types `target` into the middle cell of a [`table`] and checks the
    /// edit kept to the invariant: a refusal leaves every byte as it was
    /// and says why with its code; an acceptance writes the raw value
    /// chosen into that cell and nothing else.
    fn edit_table(c: &Conversion, enc: Encoding, target: f64) -> Edit {
        let (mut img, at) = table(enc);
        let before = img.clone();
        let w = enc.storage().cell_bytes();
        let got = edit(c, enc, target, &mut img, at);
        match &got {
            Err(e) => {
                assert_eq!(img, before, "{enc:?} at {target}: bytes changed by {e:?}");
                let d = e.to_diagnostic(rr_core::diag::Provenance::new(
                    "defs.xml",
                    rr_core::diag::Span::line_col(1, 1),
                ));
                assert_eq!(d.code(), e.code());
                assert_eq!(d.message(), e.to_string());
            }
            Ok(raw) => {
                assert_eq!(enc.decode(&img[at..]).unwrap(), *raw);
                assert_eq!(img[..at], before[..at], "{enc:?} at {target}");
                assert_eq!(img[at + w..], before[at + w..], "{enc:?} at {target}");
            }
        }
        got
    }

    /// The real value of `raw` under `c`.
    fn real(c: &Conversion, raw: Raw) -> f64 {
        c.to_real_raw(raw).unwrap()
    }

    #[test]
    fn t6_a_value_beyond_either_end_is_refused_with_the_range_never_wrapped() {
        // Rising and falling linear scalings (one a linear formula, one the
        // coefficients), and a rising formula solved by bisection. Neither
        // end of any storage overflows under them, so past an end is out of
        // range, not past a division by zero.
        let forms: [(&str, Conversion); 4] = [
            ("x*0.5+3", conv("x*0.5+3")),
            ("10-x*0.25", conv("10-x*0.25")),
            (
                "linear -0.5, 7",
                Conversion::new(
                    rr_core::conversion::Form::Linear(
                        rr_core::conversion::Linear::new(-0.5, 7.0).unwrap(),
                    ),
                    None,
                    None,
                ),
            ),
            ("x*x*x", conv("x*x*x")),
        ];
        let mut checked = 0;
        for (name, c) in &forms {
            for enc in every_plain_encoding() {
                if *name == "x*x*x" && enc.storage() == Storage::F64 {
                    continue; // overflows at both ends: see the next test
                }
                let [lo, lo_in, hi_in, hi] = ends(enc);
                let (r_lo, r_hi) = (real(c, lo), real(c, hi));
                let (min, max) = (r_lo.min(r_hi), r_lo.max(r_hi));
                // Each end of the real range: the raw value giving it, the
                // step back inside, and which way is beyond.
                let at_max = if r_hi > r_lo {
                    (hi, real(c, hi) - real(c, hi_in), 1.0)
                } else {
                    (lo, real(c, lo) - real(c, lo_in), 1.0)
                };
                let at_min = if r_hi > r_lo {
                    (lo, real(c, lo_in) - real(c, lo), -1.0)
                } else {
                    (hi, real(c, hi_in) - real(c, hi), -1.0)
                };
                for (end, step, dir) in [at_max, at_min] {
                    let edge = real(c, end);
                    assert!(step > 0.0, "{name} {enc:?}");
                    // A quarter of a step past the end: the end itself
                    // (the half-step rule), never a wrapped raw value.
                    let near = edge + dir * step * 0.25;
                    assert_eq!(edit_table(c, enc, near), Ok(end), "{name} {enc:?} {near}");
                    // Three quarters of a step, a thousand steps, and as
                    // far as an f64 goes: refused, naming the range.
                    let fars = [
                        edge + dir * step * 0.75,
                        edge + dir * step * 1000.0,
                        dir * f64::MAX,
                    ];
                    for target in fars {
                        assert!(target.is_finite(), "{name} {enc:?}");
                        let want = Reject::OutOfRange {
                            target,
                            min,
                            max,
                            nearest: end,
                        };
                        let got = edit_table(c, enc, target);
                        assert_eq!(got, Err(want), "{name} {enc:?} {target}");
                        let text = want.to_string();
                        assert!(text.contains(&format!("{min} to {max}")), "{text}");
                        assert_eq!(want.code().as_str(), "CNV0007");
                        checked += 1;
                    }
                }
            }
        }
        // 4 forms x 16 encodings, less the two skipped, x 2 ends x 3 values.
        assert_eq!(checked, (4 * 16 - 2) * 2 * 3);
    }

    /// A refusal case through a division by zero: formula, encoding, the
    /// values typed, the refusal expected, and words its reason must have.
    type PoleCase = (
        &'static str,
        Encoding,
        &'static [f64],
        fn(&Reject) -> bool,
        &'static str,
    );

    #[test]
    fn t6_a_value_only_reachable_through_a_division_by_zero_is_refused() {
        let u8 = plain(Storage::U8, Endian::Big);
        let i8 = plain(Storage::I8, Endian::Big);
        let u32 = plain(Storage::U32, Endian::Little);
        let f64le = plain(Storage::F64, Endian::Little);
        let cases: Vec<PoleCase> = vec![
            // Pole at the top raw value: raw 0..=254 give -1/255 down to -1;
            // below -1 by more than half the step back (-1/2 at raw 253)
            // lies only between raw 254 and the pole.
            (
                "1/(x-255)",
                u8,
                &[-1.3, -1000.0, -f64::MAX],
                |e| {
                    matches!(e, Reject::NonFinite(NonFinite::Beyond { raw: Raw::Int(254), limit, .. })
                    if *limit == -1.0)
                },
                "raw value 255 has none",
            ),
            // Pole at the lowest raw value of signed storage.
            (
                "1/(x+128)",
                i8,
                &[5.0, 1e300, f64::MAX],
                |e| {
                    matches!(
                        e,
                        Reject::NonFinite(NonFinite::Beyond {
                            raw: Raw::Int(-127),
                            ..
                        })
                    )
                },
                "raw value -128 has none",
            ),
            // K/x on 4-byte storage: 100000 at raw 1, nothing at raw 0.
            (
                "100000/x",
                u32,
                &[130_000.0, 1e9, f64::MAX],
                |e| {
                    matches!(
                        e,
                        Reject::NonFinite(NonFinite::Beyond {
                            raw: Raw::Int(1),
                            ..
                        })
                    )
                },
                "raw value 0 has none",
            ),
            (
                "1/(x-4294967295)",
                u32,
                &[-1.6, -1e12],
                |e| {
                    matches!(
                        e,
                        Reject::NonFinite(NonFinite::Beyond {
                            raw: Raw::Int(4_294_967_294),
                            ..
                        })
                    )
                },
                "raw value 4294967295 has none",
            ),
            // Not finite over part of the range: no value below raw 100.
            (
                "sqrt(x-100)",
                u8,
                &[-0.6, -5.0, -f64::MAX],
                |e| {
                    matches!(
                        e,
                        Reject::NonFinite(NonFinite::Beyond {
                            raw: Raw::Int(100),
                            ..
                        })
                    )
                },
                "raw value 99 has none",
            ),
            // An overflow part way: x*1e300 is infinite once |x| passes
            // about 1.8e8, so the real values stop there, far below f64::MAX.
            (
                "x*1e300/1e300",
                f64le,
                &[1e9, f64::MAX, -1e9, -f64::MAX],
                |e| matches!(e, Reject::NonFinite(NonFinite::Beyond { .. })),
                "has none",
            ),
            (
                "x*x*x",
                f64le,
                &[f64::MAX, -f64::MAX],
                |e| matches!(e, Reject::NonFinite(NonFinite::Beyond { .. })),
                "has none",
            ),
        ];
        // Within half the step back of the last value before a pole: that
        // value's raw value, never one past the pole.
        for (formula, enc, target, raw) in [
            ("1/(x-255)", u8, -1.2, 254),
            ("100000/x", u32, 120_000.0, 1),
            ("sqrt(x-100)", u8, -0.4, 100),
        ] {
            assert_eq!(edit_table(&conv(formula), enc, target), Ok(Raw::Int(raw)));
        }
        for (formula, enc, targets, want, reason) in cases {
            let c = conv(formula);
            for &target in targets {
                let err = edit_table(&c, enc, target).unwrap_err();
                assert!(want(&err), "{formula} at {target}: {err:?}");
                assert!(err.to_string().contains(reason), "{formula}: {err}");
                assert_eq!(err.code().as_str(), "CNV0008", "{formula}");
                assert!(!err.is_conversion_wide());
            }
            // Not a number, or infinite: refused for the value, whatever
            // the conversion.
            for t in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let err = edit_table(&c, enc, t).unwrap_err();
                assert!(
                    matches!(err, Reject::NonFinite(NonFinite::Target(v)) if v.to_bits() == t.to_bits()),
                    "{formula} at {t}: {err:?}"
                );
                assert!(err.to_string().contains("is not a finite number"));
            }
        }

        // K/x on signed storage, and a pole inside unsigned storage: the
        // range has a hole, so no value at all is solved across it, not even
        // one with a raw value of its own (100/x = 50 at raw 2).
        for (formula, enc, pole) in [
            ("100/x", i8, 0),
            ("100/x", plain(Storage::I16, Endian::Little), 0),
            ("100/x", plain(Storage::I32, Endian::Big), 0),
            ("1/(x-10)", u8, 10),
        ] {
            let c = conv(formula);
            for target in [
                50.0,
                -50.0,
                0.0,
                1e6,
                -1e6,
                f64::MAX,
                f64::NAN,
                f64::INFINITY,
            ] {
                let err = edit_table(&c, enc, target).unwrap_err();
                assert!(
                    matches!(err, Reject::NotInvertible(NotInvertible::Gap { at })
                        if at.raw() == Raw::Int(pole)),
                    "{formula} {enc:?} at {target}: {err:?}"
                );
                assert!(
                    err.to_string()
                        .contains(&format!("has no real value at raw value {pole} ")),
                    "{err}"
                );
                assert_eq!(err.code().as_str(), "CNV0006");
                assert!(err.is_conversion_wide());
            }
        }
    }

    fn encodings() -> impl Strategy<Value = Encoding> {
        let storage = prop::sample::select(vec![
            Storage::U8,
            Storage::I8,
            Storage::U16,
            Storage::I16,
            Storage::U32,
            Storage::I32,
            Storage::F32,
        ]);
        let endian = prop::sample::select(vec![Endian::Big, Endian::Little]);
        (storage, endian, any::<bool>()).prop_map(|(s, e, field)| {
            let bitfield = (field && !s.is_float())
                .then(|| BitField::new(0x0FF0, None, false).unwrap())
                .filter(|_| s.cell_bytes() >= 2);
            Encoding::new(s, e, bitfield, Layout::Default).unwrap()
        })
    }

    fn formulas() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec![
            "x",
            "-x",
            "x*0.25-50",
            "100-x/2",
            "x*1e9",
            "sqrt(x)",
            "x*x*x",
            "100/x",
            "1/(x-255)",
            "1/(x-10)",
            "(x-128)^2",
            "x*0+4",
        ])
    }

    fn targets() -> impl Strategy<Value = f64> {
        prop_oneof![
            -1e12..1e12_f64,
            -300.0..300.0_f64,
            Just(f64::NAN),
            Just(f64::INFINITY),
            Just(4_294_967_296.0),
            Just(-2_147_483_649.0),
            Just(65_536.0),
            Just(256.0),
            Just(-129.0),
            Just(f64::MAX),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Whatever the value, conversion and encoding: a refusal changes no
        /// byte; an acceptance changes only the cell, which then decodes to
        /// exactly the raw value chosen (never a wrapped or clamped one).
        #[test]
        fn t6_a_refused_edit_changes_no_bytes(
            formula in formulas(),
            enc in encodings(),
            target in targets(),
            offset in 0_usize..16,
        ) {
            let c = conv(formula);
            let mut img = image();
            let before = img.clone();
            let width = enc.storage().cell_bytes();
            match edit(&c, enc, target, &mut img, offset) {
                Err(e) => {
                    prop_assert_eq!(&img, &before, "{} at {}: {:?}", formula, target, e);
                    prop_assert!(!e.to_string().is_empty());
                }
                Ok(raw) => {
                    prop_assert!(enc.raw_range().unwrap().contains(raw));
                    prop_assert_eq!(enc.decode(&img[offset..]).unwrap(), raw);
                    prop_assert_eq!(&img[..offset], &before[..offset]);
                    prop_assert_eq!(&img[offset + width..], &before[offset + width..]);
                    // Not clamped: the value written is within half a raw
                    // step of the value typed (an end, only when the value
                    // is within half the step back from it).
                    let real = c.to_real_raw(raw).unwrap();
                    let step = steps(&c, enc, raw);
                    let slack = 4.0 * f64::EPSILON * target.abs().max(real.abs());
                    prop_assert!(
                        (target - real).abs() <= step / 2.0 + slack,
                        "{} at {}: wrote {:?} ({}), step {}", formula, target, raw, real, step
                    );
                }
            }
        }
    }

    /// The largest distance from the real value of `raw` to that of a
    /// storable neighbour with one: the step a written value is rounded
    /// within.
    fn steps(c: &Conversion, enc: Encoding, raw: Raw) -> f64 {
        let range = enc.raw_range().unwrap();
        let real = c.to_real_raw(raw).unwrap();
        let neighbours = match raw {
            Raw::Int(v) => [Raw::Int(v - 1), Raw::Int(v + 1)],
            Raw::F32(v) => [Raw::F32(v.next_down()), Raw::F32(v.next_up())],
            Raw::F64(v) => [Raw::F64(v.next_down()), Raw::F64(v.next_up())],
        };
        neighbours
            .into_iter()
            .filter(|&n| range.contains(n))
            .filter_map(|n| c.to_real_raw(n).ok())
            .map(|n| (n - real).abs())
            .fold(0.0, f64::max)
    }

    // ---------------------------------------------------------------------------
    // No wrapping or clamping arithmetic in the conversion module
    // ---------------------------------------------------------------------------

    /// Arithmetic that could turn a value the cell cannot hold into one it can:
    /// clamping, wrapping or saturating, and `as` casts between numeric types
    /// (which truncate, wrap or round silently).
    const SUSPECT: &[&str] = &[
        ".clamp(",
        "wrapping_",
        "saturating_",
        "overflowing_",
        " as u8",
        " as u16",
        " as u32",
        " as u64",
        " as u128",
        " as usize",
        " as i8",
        " as i16",
        " as i32",
        " as i64",
        " as i128",
        " as isize",
        " as f32",
        " as f64",
    ];

    /// A line with a suspect operation must say why it cannot reach a written
    /// value, with this marker and a reason after it.
    const MARKER: &str = "// no-wrap: ";

    /// The conversion module's shipping sources (its unit tests excluded).
    fn conversion_sources() -> Vec<PathBuf> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![src.join("conversion.rs")];
        let mut dirs = vec![src.join("conversion")];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    if !name.ends_with("tests.rs") {
                        files.push(path);
                    }
                }
            }
        }
        files.sort();
        files
    }

    /// The suspect lines of `text` with no [`MARKER`] reason, as
    /// `line: text`. Comment lines are skipped.
    fn unjustified(text: &str) -> Vec<String> {
        text.lines()
            .enumerate()
            .filter(|(_, line)| {
                let code = line.trim_start();
                !code.starts_with("//") && SUSPECT.iter().any(|s| code.contains(s))
            })
            .filter(|(_, line)| {
                line.split_once(MARKER)
                    .is_none_or(|(_, why)| why.trim().is_empty())
            })
            .map(|(n, line)| format!("{}: {}", n + 1, line.trim()))
            .collect()
    }

    #[test]
    fn t6_the_conversion_module_never_wraps_or_clamps() {
        let files = conversion_sources();
        assert!(files.len() >= 8, "{files:?}");
        let mut problems = Vec::new();
        let mut justified = 0;
        for file in &files {
            let text = fs::read_to_string(file).unwrap();
            justified += text.matches(MARKER).count();
            for p in unjustified(&text) {
                problems.push(format!("{}:{p}", file.display()));
            }
        }
        assert!(
            problems.is_empty(),
            "wrapping, clamping or casting arithmetic in the conversion module without a \
             `{MARKER}<reason>`: {problems:#?}"
        );
        // The current exceptions (domain keys, the raw-to-f64 input, a message's
        // point number, the solve's candidate indices): a new one is reviewed.
        assert_eq!(justified, 8, "the number of `{MARKER}` exceptions changed");
    }

    #[test]
    fn t6_the_scan_finds_what_it_looks_for() {
        assert_eq!(
            unjustified("let r = v.clamp(0, 255);\n// x.clamp(1, 2)\n"),
            ["1: let r = v.clamp(0, 255);"]
        );
        assert_eq!(
            unjustified("let a = x as u32;\nlet b = y as u8; // no-wrap: \n"),
            ["1: let a = x as u32;", "2: let b = y as u8; // no-wrap:"]
        );
        assert!(unjustified("let a = x as u32; // no-wrap: fits\n").is_empty());
        assert_eq!(
            unjustified("a.wrapping_add(1)\nb.saturating_sub(1)\n").len(),
            2
        );
    }
}
