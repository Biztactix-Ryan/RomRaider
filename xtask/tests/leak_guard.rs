//! `cargo xtask leak-guard` (story US-RR-13, task US-RR-13-5): the ROM-leak
//! guard, end to end, on planted positive and negative samples.
//!
//! Each test lays out a scratch tree with a `fixtures/public` directory,
//! plants samples in it, and runs the built `xtask` binary against it with
//! the workspace's committed corpus manifest and coverage report (so the
//! ROM IDs are the real corpus's), checking the exit status and the
//! findings printed. Bytes with the SHA-256 of a real corpus image cannot be
//! made here (the images are never in the repo), so the hash rule is driven
//! by a copy of the committed manifest whose first entry's hash is replaced
//! with the planted file's, and by a planted private-corpus `SHA256SUMS`.
//! The rules themselves are in `xtask/src/leak_guard.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::corpus_coverage::{self, Coverage};
use xtask::corpus_manifest::{self, Manifest, sha256_hex};
use xtask::leak_guard::{
    Finding, Known, MAX_PUBLIC_BYTES, MAX_SCAN_BYTES, PUBLIC_DIR, SYNTHETIC_MARKER, check_file,
};

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap()
}

fn committed_manifest() -> PathBuf {
    workspace_root().join(corpus_manifest::COMMITTED_PATH)
}

/// A fresh scratch root holding an empty `fixtures/public`.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("leak-guard")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(PUBLIC_DIR)).unwrap();
    dir.canonicalize().unwrap()
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn plant(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, bytes).unwrap();
    p
}

/// Runs `xtask leak-guard --root <root> <extra>` from `root`: the known
/// data defaults to the workspace's committed manifest and coverage report,
/// whatever `--root` says. Returns the exit code and all output.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn guard(root: &Path, extra: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("leak-guard")
        .arg("--root")
        .arg(root)
        .args(extra)
        .current_dir(root)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// The first ROM ID the committed manifest gives (a real corpus ID).
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn real_rom_id() -> String {
    let m = Manifest::from_toml(&fs::read_to_string(committed_manifest()).unwrap()).unwrap();
    m.roms
        .files
        .iter()
        .find_map(|f| f.filename_rom_id.clone())
        .unwrap()
}

/// A binary image of `size` bytes in the shape of a stock ROM: erased flash
/// (0xFF), a reset vector at 0, and `id` at `address`.
fn rom_like(size: usize, id: &[u8], address: usize) -> Vec<u8> {
    let mut v = vec![0xFF; size];
    v[..4].copy_from_slice(&[0x00, 0x00, 0x0C, 0x00]);
    v[address..address + id.len()].copy_from_slice(id);
    v
}

fn assert_flags(out: &str, needles: &[&str]) {
    for n in needles {
        assert!(out.contains(n), "expected {n:?} in:\n{out}");
    }
}

/// Negative samples: the committed synthetic tier, a scrubbed trace, a
/// small text fixture and a clean binary all pass.
#[test]
fn clean_public_fixtures_pass() {
    let root = scratch("clean");
    let synthetic = workspace_root().join("fixtures/public/synthetic");
    let copy = root.join("fixtures/public/synthetic");
    fs::create_dir_all(&copy).unwrap();
    for e in fs::read_dir(&synthetic).unwrap() {
        let e = e.unwrap();
        fs::copy(e.path(), copy.join(e.file_name())).unwrap();
    }
    plant(
        &root,
        "fixtures/public/traces/scrubbed.log",
        b"t=0.000 ssm vin=JF1GD29X********* ecu=XXXXXXXX\nt=0.010 vin=JF1XXXXXXXXXXXXXX\n",
    );
    plant(
        &root,
        "fixtures/public/notes.toml",
        b"schema = 1\nsha = \"6dad26e8a605d53570c3523b5d95082eadee60084328d5f9da60b0a163c2fc48\"\n",
    );
    plant(
        &root,
        "fixtures/public/small.bin",
        &rom_like(0x3000, b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF", 0x2000),
    );
    // Exactly at the limit is allowed.
    plant(
        &root,
        "fixtures/public/limit.bin",
        &vec![0u8; MAX_PUBLIC_BYTES as usize],
    );
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 0, "{out}");
    assert_flags(&out, &["leak-guard: ok (", "under fixtures/public"]);
}

/// The repository's own fixtures/public passes with the committed data.
#[test]
fn the_committed_public_tree_passes() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("leak-guard")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_flags(&text, &["known ROM ID(s)", "leak-guard: ok ("]);
}

/// A full-size stock-ROM-shaped image with a real corpus ROM ID, as a
/// private-corpus ROM would be: too large, holds a known ID, and an ID at a
/// ROM ID address; with the private corpus's SHA256SUMS given, its hash too.
#[test]
fn a_stock_rom_under_public_is_rejected() {
    let root = scratch("stock-rom");
    let id = real_rom_id();
    let rom = rom_like(512 << 10, id.as_bytes(), 0x2000);
    plant(&root, "fixtures/public/calibration.dat", &rom);
    let sums = plant(
        &root,
        "private/SHA256SUMS",
        format!("{}  ./roms/my-car.bin\n", sha256_hex(&rom)).as_bytes(),
    );
    let (code, out) = guard(&root, &["--sums", sums.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[
            "fixtures/public/calibration.dat: 524288 bytes, over the 65536-byte limit",
            &format!("holds known ROM ID {id:?} at offset 0x2000"),
            &format!("ROM-ID-shaped {id:?} at ROM ID address 0x2000"),
            "bytes are identical to roms/my-car.bin in ",
            "4 finding(s)",
        ],
    );
}

/// A file whose bytes are a manifest entry's (here: a copy of the committed
/// manifest with the first image's hash replaced by the planted file's) is
/// rejected under fixtures/public and anywhere else, whatever its size.
#[test]
fn bytes_matching_a_manifest_hash_are_rejected_everywhere() {
    let root = scratch("manifest-hash");
    let planted = b"a stand-in for a stock ROM image".to_vec();
    let mut m = Manifest::from_toml(&fs::read_to_string(committed_manifest()).unwrap()).unwrap();
    m.roms.files[0].sha256 = sha256_hex(&planted);
    let image = m.roms.files[0].path.clone();
    let manifest = plant(
        &root,
        "planted-manifest.toml",
        m.render().unwrap().as_bytes(),
    );
    plant(&root, "fixtures/public/a.bin", &planted);
    plant(&root, "crates/x/tests/data/b.bin", &planted);
    let manifest = manifest.to_str().unwrap();
    let (code, out) = guard(
        &root,
        &["--manifest", manifest, "fixtures/public/a.bin", "crates"],
    );
    assert_eq!(code, 1, "{out}");
    let label = format!("bytes are identical to stock ROM {image} in ");
    assert_flags(
        &out,
        &[
            &format!("fixtures/public/a.bin: {label}"),
            &format!("crates/x/tests/data/b.bin: {label}"),
            "2 finding(s) in 2 file(s)",
        ],
    );
}

/// A small binary (under the size cap) cut from a ROM: the known ID at a
/// non-ID offset, an unknown ID-shaped string at each ROM ID address.
#[test]
fn a_rom_fragment_under_public_is_rejected() {
    let root = scratch("fragment");
    let id = real_rom_id();
    plant(
        &root,
        "fixtures/public/known.bin",
        &rom_like(0x1000, id.as_bytes(), 0x123),
    );
    for a in [0x200usize, 0x2000, 0x2004, 0x400C] {
        plant(
            &root,
            &format!("fixtures/public/unknown-{a:x}.bin"),
            &rom_like(0x5000, b"ZZ9X123Q", a),
        );
    }
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 1, "{out}");
    let mut needles = vec![format!(
        "fixtures/public/known.bin: holds known ROM ID {id:?} at offset 0x123"
    )];
    for a in [0x200usize, 0x2000, 0x2004, 0x400C] {
        needles.push(format!(
            "fixtures/public/unknown-{a:x}.bin: binary with ROM-ID-shaped \"ZZ9X123Q\" at ROM ID address 0x{a:X}"
        ));
    }
    assert_flags(
        &out,
        &needles.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_flags(&out, &["5 finding(s)"]);
}

/// The synthetic marker exempts a binary from the ROM-ID-address rule,
/// never from the known-ID rule.
#[test]
fn the_synthetic_marker_does_not_hide_a_known_id() {
    let root = scratch("marker");
    let id = real_rom_id();
    let mut fake = rom_like(0x3000, b"ZZ9X123Q", 0x2000);
    fake[..SYNTHETIC_MARKER.len()].copy_from_slice(SYNTHETIC_MARKER);
    plant(&root, "fixtures/public/synthetic-shaped.bin", &fake);
    assert_eq!(guard(&root, &[]).0, 0);
    fake[0x2000..0x2008].copy_from_slice(id.as_bytes());
    plant(&root, "fixtures/public/synthetic-shaped.bin", &fake);
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[&format!("holds known ROM ID {id:?}"), "1 finding(s)"],
    );
}

/// A trace or log holding a 17-character VIN fails, text or binary; the
/// output names only the manufacturer code, never the VIN.
#[test]
fn a_trace_with_a_vin_is_rejected_without_printing_it() {
    let root = scratch("vin");
    plant(
        &root,
        "fixtures/public/traces/drive.log",
        b"t=0.000 ssm init\nt=0.120 vin=JF1GD29662G512345\n",
    );
    let mut bin = vec![0x80, 0x10, 0xF0, 0x11];
    bin.extend_from_slice(b"JF2SH63619H700001");
    bin.extend_from_slice(&[0x00, 0x8C]);
    plant(&root, "fixtures/public/traces/drive.rrtrace", &bin);
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[
            "fixtures/public/traces/drive.log: unscrubbed VIN (JF1...) at offset 0x1D",
            "fixtures/public/traces/drive.rrtrace: unscrubbed VIN (JF2...) at offset 0x4",
            "2 finding(s)",
        ],
    );
    assert!(!out.contains("512345") && !out.contains("700001"), "{out}");
}

/// Outside fixtures/public only the hash rule applies: a large binary, a
/// known ROM ID and a VIN pass there.
#[test]
fn other_paths_get_the_hash_rule_only() {
    let root = scratch("outside");
    let id = real_rom_id();
    plant(
        &root,
        "docs/big.bin",
        &rom_like(256 << 10, id.as_bytes(), 0x2000),
    );
    plant(&root, "docs/vin.txt", b"JF1GD29662G512345\n");
    let (code, out) = guard(&root, &["docs"]);
    assert_eq!(code, 0, "{out}");
    assert_flags(&out, &["ok (2 file(s) checked, 0 under fixtures/public)"]);
}

/// A symlink under fixtures/public fails, found by the walk or named on the
/// command line (as the pre-commit hook names staged paths): a named
/// symlink is checked as itself, never resolved to its target.
#[cfg(unix)]
#[test]
fn a_symlink_under_public_is_rejected() {
    let root = scratch("symlink");
    let target = plant(&root, "outside.bin", b"x");
    std::os::unix::fs::symlink(&target, root.join("fixtures/public/link.bin")).unwrap();
    for args in [&[][..], &["fixtures/public/link.bin"]] {
        let (code, out) = guard(&root, args);
        assert_eq!(code, 1, "{args:?}: {out}");
        assert_flags(
            &out,
            &[
                "fixtures/public/link.bin: symlink",
                ": 1 finding(s) in 1 file(s)",
            ],
        );
    }
}

/// A path that does not exist, or a corrupt manifest, is a read error
/// (exit 2), never a pass.
#[test]
fn unreadable_inputs_exit_2() {
    let root = scratch("errors");
    let (code, out) = guard(&root, &["fixtures/public/missing.bin"]);
    assert_eq!(code, 2, "{out}");
    assert_flags(&out, &["leak-guard: error:", "missing.bin"]);
    let bad = plant(&root, "bad.toml", b"schema = 99\n");
    let (code, out) = guard(&root, &["--manifest", bad.to_str().unwrap()]);
    assert_eq!(code, 2, "{out}");
    // No fixtures/public at the root, and no paths: an error too.
    fs::remove_dir_all(root.join(PUBLIC_DIR)).unwrap();
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 2, "{out}");
}

/// The committed data is what the guard knows by default.
#[test]
fn committed_manifest_and_coverage_load() {
    let k = Known::committed(&workspace_root()).unwrap();
    assert!(k.hash_count() > 500, "{}", k.hash_count());
    assert!(k.rom_id_count() >= 400, "{}", k.rom_id_count());
    let id = real_rom_id();
    let root = scratch("library");
    let f = plant(
        &root,
        "fixtures/public/x.bin",
        &rom_like(0x100, id.as_bytes(), 0x10),
    );
    assert_eq!(
        check_file(&f, true, &k).unwrap(),
        [Finding::KnownRomId { id, offset: 0x10 }]
    );
    assert!(check_file(&f, false, &k).unwrap().is_empty());
}

/// The size rule's boundary: one byte over 64 KiB fails (exactly 64 KiB
/// passes, [`clean_public_fixtures_pass`]), and only the size rule fires.
#[test]
fn one_byte_over_the_size_limit_is_rejected() {
    let root = scratch("size-limit");
    plant(
        &root,
        "fixtures/public/over.bin",
        &vec![0u8; MAX_PUBLIC_BYTES as usize + 1],
    );
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[
            "fixtures/public/over.bin: 65537 bytes, over the 65536-byte limit",
            ": 1 finding(s) in 1 file(s)",
        ],
    );
}

/// Near misses of the hash rule pass: bytes one byte off a manifest entry,
/// and a definition file's bytes outside fixtures/public (legacy/ keeps the
/// GPL DTDs the definition corpus also carries). The same definition bytes
/// under fixtures/public, and the logger package anywhere, fail.
#[test]
fn hash_near_misses_pass_and_definition_hashes_are_public_only() {
    let root = scratch("hash-near-miss");
    let rom = b"a stand-in for a stock ROM image".to_vec();
    let mut off_by_one = rom.clone();
    off_by_one[0] ^= 1;
    let def = b"<!ELEMENT roms (rom+)>\n".to_vec();
    let logger = b"a stand-in for logger_v370.zip".to_vec();
    let mut m = Manifest::from_toml(&fs::read_to_string(committed_manifest()).unwrap()).unwrap();
    m.roms.files[0].sha256 = sha256_hex(&rom);
    let defs = m.defs.as_mut().unwrap();
    defs.files[0].sha256 = sha256_hex(&def);
    defs.logger_v370.sha256 = sha256_hex(&logger);
    let def_path = defs.files[0].path.clone();
    let manifest = plant(
        &root,
        "planted-manifest.toml",
        m.render().unwrap().as_bytes(),
    );
    let manifest = manifest.to_str().unwrap();
    plant(&root, "fixtures/public/near.bin", &off_by_one);
    plant(&root, "legacy/definitions/cars_def.dtd", &def);
    let (code, out) = guard(
        &root,
        &["--manifest", manifest, "fixtures/public", "legacy"],
    );
    assert_eq!(code, 0, "{out}");
    assert_flags(&out, &["ok (2 file(s) checked, 1 under fixtures/public)"]);

    plant(&root, "fixtures/public/def.dtd", &def);
    plant(&root, "tools/logger.zip", &logger);
    let (code, out) = guard(&root, &["--manifest", manifest, "fixtures/public", "tools"]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[
            &format!("fixtures/public/def.dtd: bytes are identical to definition {def_path} in "),
            "tools/logger.zip: bytes are identical to logger package logger_v370.zip in ",
            ": 2 finding(s) in 3 file(s)",
        ],
    );
}

/// A public file larger than the content-scan window is still hashed over
/// all of its bytes, so a large private-corpus image is matched by hash.
#[test]
fn a_file_past_the_scan_window_is_hashed_whole() {
    let root = scratch("scan-window");
    let mut big = vec![0u8; usize::try_from(MAX_SCAN_BYTES).unwrap() + 1];
    *big.last_mut().unwrap() = 1;
    plant(&root, "fixtures/public/huge.bin", &big);
    let sums = plant(
        &root,
        "private/SHA256SUMS",
        format!("{}  ./roms/huge.bin\n", sha256_hex(&big)).as_bytes(),
    );
    let (code, out) = guard(&root, &["--sums", sums.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[
            "fixtures/public/huge.bin: bytes are identical to roms/huge.bin in ",
            ": 2 finding(s) in 1 file(s)",
        ],
    );
}

/// Near misses of the ROM-ID-address rule pass: seven alphanumerics then a
/// non-alphanumeric byte at an ID address, an ID cut off by the end of the
/// file, and an ID-shaped string one byte past an ID address.
#[test]
fn rom_id_address_near_misses_pass() {
    let root = scratch("id-near-miss");
    plant(
        &root,
        "fixtures/public/seven.bin",
        &rom_like(0x5000, b"ZZ9X123\xFF", 0x2000),
    );
    let mut cut = rom_like(0x2004, b"", 0);
    cut[0x2000..].copy_from_slice(b"ZZ9X");
    plant(&root, "fixtures/public/cut.bin", &cut);
    plant(
        &root,
        "fixtures/public/shifted.bin",
        &rom_like(0x5000, b"ZZ9X123Q", 0x201),
    );
    let (code, out) = guard(&root, &[]);
    assert_eq!(code, 0, "{out}");
    assert_flags(&out, &["ok (3 file(s) checked, 3 under fixtures/public)"]);
}

/// The symlink rule is scoped to fixtures/public: a symlink elsewhere, given
/// on the command line, passes.
#[cfg(unix)]
#[test]
fn a_symlink_outside_public_passes() {
    let root = scratch("symlink-outside");
    let target = plant(&root, "outside.bin", b"x");
    fs::create_dir_all(root.join("docs")).unwrap();
    std::os::unix::fs::symlink(&target, root.join("docs/link.bin")).unwrap();
    let (code, out) = guard(&root, &["docs/link.bin"]);
    assert_eq!(code, 0, "{out}");
    assert_flags(&out, &["ok (1 file(s) checked, 0 under fixtures/public)"]);
}

/// The coverage report is a source of known ROM IDs (file-name, image and
/// definition `xmlid` IDs) and of ROM ID addresses, on its own: a planted
/// report (`--coverage`) with IDs and an address no manifest has makes each
/// of them fail, and the stock defaults no longer know them.
#[test]
fn coverage_ids_and_addresses_are_known() {
    let root = scratch("coverage");
    let mut c = Coverage::from_toml(
        &fs::read_to_string(workspace_root().join(corpus_coverage::COMMITTED_PATH)).unwrap(),
    )
    .unwrap();
    c.roms[0].filename_rom_id = Some("QQ6V222U".into());
    c.roms[0].image_rom_id = Some("QQ7Z000X".into());
    c.roms[0].image_rom_id_address = Some("0x1F0".into());
    let with_def = c
        .roms
        .iter_mut()
        .find(|r| !r.definitions.is_empty())
        .unwrap();
    with_def.definitions[0].xmlid = Some("QQ8Y111W".into());
    let coverage = plant(
        &root,
        "planted-coverage.toml",
        c.render().unwrap().as_bytes(),
    );
    plant(
        &root,
        "fixtures/public/ids.txt",
        b"a QQ6V222U b QQ7Z000X c QQ8Y111W\n",
    );
    plant(
        &root,
        "fixtures/public/at-1f0.bin",
        &rom_like(0x3000, b"ZZ9X123Q", 0x1F0),
    );
    assert_eq!(guard(&root, &[]).0, 0);
    let (code, out) = guard(&root, &["--coverage", coverage.to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert_flags(
        &out,
        &[
            "fixtures/public/ids.txt: holds known ROM ID \"QQ6V222U\" at offset 0x2",
            "fixtures/public/ids.txt: holds known ROM ID \"QQ7Z000X\" at offset 0xD",
            "fixtures/public/ids.txt: holds known ROM ID \"QQ8Y111W\" at offset 0x18",
            "fixtures/public/at-1f0.bin: binary with ROM-ID-shaped \"ZZ9X123Q\" at ROM ID address 0x1F0",
            ": 4 finding(s) in 2 file(s)",
        ],
    );
}
