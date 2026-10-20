//! `cargo xtask check-release-artifacts` (story US-RR-38, task US-RR-38-8):
//! release artifacts never contain ROM images or fixture data.
//!
//! Each artifact kind release.yml publishes (the cargo-dist tarball, the
//! .deb, the AppImage) is built here in the shape the committed recipes give
//! it (`common/artifacts.rs`), and:
//!
//! * the clean set passes, in every compression the reader accepts;
//! * a ROM-sized file, a fixture path, a ROM-extension file, a ROM in place
//!   of (or appended to) the binary, a ROM in place of an allow-listed text
//!   file, a HEX-record image, a symlink into fixtures and a nested archive
//!   are each planted in each artifact kind, and each one fails the check
//!   with the expected finding;
//! * anything in the release directory that the check cannot open fails it;
//! * the allow-lists agree with the packaging recipes, so a recipe change
//!   cannot drift from the check unnoticed.
//!
//! US-RR-38-4 (the acceptance criterion "Release artifacts never contain ROM
//! images or fixture data") adds plants from the repo's real fixture layout
//! (`fixtures/corpus-manifest.toml`, `RR_CORPUS_DIR`'s default cache, the
//! stock-ROM mirror, the synthetic tier's SHA256SUMS) and the corpus's real
//! sizes in every position (2 MiB anywhere; 1,048,784 bytes as the binary;
//! 1,048,731 bytes appended to it), and closes the one hole the artifact
//! check leaves by design, a ROM inside a well-formed ELF section, from the
//! source side: no workspace crate romraider-cli is built from compiles a
//! ROM or fixture in (`shipped_crates_compile_in_no_rom_or_fixture`).
//!
//! release.yml runs the check before publishing (`release_ci_config.rs`,
//! `content_check_problems`). The reader is also exercised on real
//! appimagetool / cargo-deb / cargo-dist output in the US-RR-38-8 log.

#[path = "common/artifacts.rs"]
mod artifacts;

use std::fs;
use std::path::{Path, PathBuf};

use artifacts::*;
use xtask::release_artifacts::{Layout, check_asset, check_dir};

fn problems(name: &str, data: &[u8]) -> Vec<String> {
    check_asset(name, data).problems
}

fn assert_clean(name: &str, data: &[u8]) {
    let r = check_asset(name, data);
    assert!(
        r.problems.is_empty(),
        "{name} should pass:\n{}",
        r.problems.join("\n")
    );
    assert!(r.entries > 0, "{name}: nothing inspected");
}

fn assert_flags(problems: &[String], needles: &[&str]) {
    for needle in needles {
        assert!(
            problems.iter().any(|p| p.contains(needle)),
            "expected a problem mentioning {needle:?}, got {problems:#?}"
        );
    }
}

/// `items` with the entry at `path` replaced by `with` (which must exist).
fn replaced(items: &[Item], path: &str, with: Item) -> Vec<Item> {
    let mut hit = false;
    let out = items
        .iter()
        .map(|i| {
            let p = match i {
                Item::Dir(p) | Item::File(p, _) | Item::Symlink(p, _) => p,
            };
            if p == path {
                hit = true;
                with.clone()
            } else {
                i.clone()
            }
        })
        .collect();
    assert!(hit, "{path} is not in the clean items");
    out
}

fn plus(items: &[Item], extra: Item) -> Vec<Item> {
    let mut out = items.to_vec();
    out.push(extra);
    out
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// An Intel HEX image as text: 64 data records and an EOF record.
fn intel_hex() -> String {
    let mut s: String = (0..64)
        .map(|i| format!(":10{:04X}00000102030405060708090A0B0C0D0E0F00\n", i * 16))
        .collect();
    s.push_str(":00000001FF\n");
    s
}

/// The ways a ROM or fixture data can be planted in a tree laid out like
/// `clean`, with the binary at `bin`, an allow-listed text file at `text`
/// and an unlisted directory `share`; and the findings each must produce.
fn plants(
    clean: &[Item],
    bin: &str,
    text: &str,
    share: &str,
) -> Vec<(&'static str, Vec<Item>, Vec<&'static str>)> {
    vec![
        (
            "512 KiB file with an innocuous name",
            plus(
                clean,
                file(&format!("{share}/calibration.dat"), rom_bytes(512 << 10)),
            ),
            vec!["ROM-sized: 524288 bytes", "unexpected file"],
        ),
        (
            "192 KiB file with no extension",
            plus(
                clean,
                file(&format!("{share}/calibration"), rom_bytes(192 << 10)),
            ),
            vec!["ROM-sized: 196608 bytes"],
        ),
        (
            "the corpus's 1,048,731-byte suspect image",
            plus(
                clean,
                file(&format!("{share}/AE5I410A.hex"), rom_bytes(1_048_731)),
            ),
            vec!["plus 155 trailing bytes", "ROM image extension `.hex`"],
        ),
        (
            "a small fixture under fixtures/",
            plus(
                clean,
                file(
                    &format!("{share}/fixtures/public/synthetic/standard.bin"),
                    rom_bytes(2048),
                ),
            ),
            vec![
                "has a `fixtures` component",
                "`synthetic` component",
                "ROM image extension `.bin`",
            ],
        ),
        (
            "a KAT fixture",
            plus(
                clean,
                file(&format!("{share}/kat/u8-le.toml"), "[[case]]\n"),
            ),
            vec!["has a `kat` component"],
        ),
        (
            "an EcuFlash image",
            plus(clean, file(&format!("{share}/stock.SRF"), rom_bytes(3000))),
            vec!["ROM image extension `.srf`"],
        ),
        (
            "a fixture manifest",
            plus(
                clean,
                file(&format!("{share}/standard.manifest.toml"), "x = 1\n"),
            ),
            vec!["fixture manifest name"],
        ),
        // US-RR-38-4: the rest of the repo's real fixture layout and the
        // corpus images' real sizes, wherever they hide.
        (
            "the stock-ROM corpus manifest (fixtures/corpus-manifest.toml, D-025)",
            plus(
                clean,
                file(&format!("{share}/corpus-manifest.toml"), "x = 1\n"),
            ),
            vec!["fixture manifest name `corpus-manifest.toml`"],
        ),
        (
            "an image from RR_CORPUS_DIR's default cache",
            plus(
                clean,
                file(
                    &format!("{share}/.cache/romraider-rs/corpus/JDM/Legacy/AE5I410A"),
                    rom_bytes(4000),
                ),
            ),
            vec!["has a `corpus` component"],
        ),
        (
            "a file from the stock-ROM mirror checkout",
            plus(
                clean,
                file(&format!("{share}/roms/USDM/stock"), rom_bytes(4000)),
            ),
            vec!["has a `roms` component"],
        ),
        (
            "the committed synthetic fixture's SHA256SUMS",
            plus(
                clean,
                file(
                    &format!("{share}/fixtures/public/synthetic/SHA256SUMS"),
                    "0  standard.bin\n",
                ),
            ),
            vec!["has a `fixtures` component"],
        ),
        (
            "a 2 MiB image disguised as a document",
            plus(
                clean,
                file(&format!("{share}/manual.pdf"), rom_bytes(2 << 20)),
            ),
            vec!["ROM-sized: 2097152 bytes"],
        ),
        (
            "the corpus's 1,048,784-byte image under the binary's name",
            replaced(clean, bin, file(bin, rom_bytes(1_048_784))),
            vec!["should be an ELF executable", "plus 208 trailing bytes"],
        ),
        (
            "the corpus's 1,048,731-byte image appended to the binary",
            replaced(
                clean,
                bin,
                file(bin, [binary(), rom_bytes(1_048_731)].concat()),
            ),
            vec!["1048731 bytes appended after the end of the ELF image"],
        ),
        (
            "a ROM under the binary's name",
            replaced(clean, bin, file(bin, rom_bytes(1 << 20))),
            vec!["should be an ELF executable", "ROM-sized: 1048576 bytes"],
        ),
        (
            "a ROM appended to the binary",
            replaced(
                clean,
                bin,
                file(bin, [binary(), rom_bytes(160 << 10)].concat()),
            ),
            vec![
                "163840 bytes appended after the end of the ELF image",
                "ROM-sized",
            ],
        ),
        (
            "a ROM in place of an allow-listed text file",
            replaced(clean, text, file(text, rom_bytes(160 << 10))),
            vec!["contains NUL bytes", "ROM-sized: 163840 bytes"],
        ),
        (
            "the corpus's 1,048,784-byte image in place of an allow-listed text file",
            replaced(clean, text, file(text, rom_bytes(1_048_784))),
            vec![
                "too large for an allow-listed text file",
                "plus 208 trailing bytes",
            ],
        ),
        (
            "an Intel HEX image in place of an allow-listed text file",
            replaced(clean, text, file(text, intel_hex())),
            vec!["looks like an Intel HEX / Motorola S-record image"],
        ),
        (
            "a symlink into fixtures",
            plus(
                clean,
                symlink(&format!("{share}/stock"), "../../fixtures/public/x"),
            ),
            vec![
                "unexpected symlink",
                "symlink target \"../../fixtures/public/x\"",
            ],
        ),
        (
            "a nested archive",
            plus(clean, file(&format!("{share}/roms.tar.gz"), gzip(b"x"))),
            vec!["nested archive `.gz`"],
        ),
    ]
}

/// US-RR-38-8: a release directory holding the three artifacts and
/// SHA256SUMS, built as the recipes build them, passes.
#[test]
fn clean_release_set_passes() {
    let dir = scratch("release-artifacts-clean");
    fs::write(dir.join(TARBALL), clean_tarball()).unwrap();
    fs::write(dir.join(DEB), clean_deb()).unwrap();
    fs::write(dir.join(APPIMAGE), clean_appimage()).unwrap();
    fs::write(
        dir.join("SHA256SUMS"),
        format!("{}  {TARBALL}\n", "0".repeat(64)),
    )
    .unwrap();
    let report = check_dir(&dir).unwrap();
    assert!(
        report.problems.is_empty(),
        "clean release set fails:\n{}",
        report.problems.join("\n")
    );
    let names: Vec<&str> = report.assets.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["SHA256SUMS", TARBALL, APPIMAGE, DEB]);
    // Every entry was looked at: 4 tarball entries; 8 AppImage entries and
    // their 10 parent directories; debian-binary + 3 control + 15 data.
    let entries: Vec<usize> = report.assets.iter().map(|(_, n)| *n).collect();
    assert_eq!(entries, [1, 4, 18, 19]);
}

/// The clean artifacts pass in every compression the reader decodes.
#[test]
fn clean_artifacts_pass_in_every_compression() {
    assert_clean(TARBALL, &clean_tarball());
    for kind in ["", "gz", "xz", "zst"] {
        assert_clean(DEB, &deb(&deb_control_items(), &deb_data_items(), kind));
    }
    for comp in [Comp::None, Comp::Gzip, Comp::Xz, Comp::Zstd] {
        assert_clean(APPIMAGE, &appimage(&appimage_items(), comp));
    }
}

/// US-RR-38-8: each planted ROM / fixture fails the tarball.
#[test]
fn tarball_with_planted_rom_or_fixture_fails() {
    let r = TARBALL_ROOT;
    let clean = tarball_items();
    for (what, items, needles) in plants(
        &clean,
        &format!("{r}/romraider-cli"),
        &format!("{r}/license.txt"),
        &format!("{r}/share"),
    ) {
        let p = problems(TARBALL, &gzip(&tar(&items)));
        assert!(!p.is_empty(), "tarball with {what} passed");
        assert_flags(&p, &needles);
    }
    // Outside the rr-cli-<target>/ root, and the same path twice.
    assert_flags(
        &problems(
            TARBALL,
            &gzip(&tar(&plus(
                &clean,
                file("AE5I410A.bin", rom_bytes(1 << 20)),
            ))),
        ),
        &[
            "AE5I410A.bin: ROM image extension",
            "ROM-sized",
            "unexpected file",
        ],
    );
    assert_flags(
        &problems(
            TARBALL,
            &gzip(&tar(&plus(
                &clean,
                file(&format!("{r}/romraider-cli"), rom_bytes(1 << 20)),
            ))),
        ),
        &["appears more than once", "should be an ELF executable"],
    );
    assert_flags(
        &problems(
            TARBALL,
            &gzip(&tar_with_raw_path(&clean, "../escape", b"x")),
        ),
        &["has a `..` component"],
    );
    let truncated = clean_tarball();
    assert_flags(
        &problems(TARBALL, &truncated[..truncated.len() / 2]),
        &["cannot be read completely"],
    );
}

/// US-RR-38-8: each planted ROM / fixture fails the .deb, in its data.tar
/// (in every compression), its control.tar, or as an extra `ar` member.
#[test]
fn deb_with_planted_rom_or_fixture_fails() {
    let clean = deb_data_items();
    for kind in ["", "gz", "xz", "zst"] {
        for (what, items, needles) in plants(
            &clean,
            "usr/bin/romraider-cli",
            "usr/share/doc/romraider-cli/copyright",
            "usr/share/romraider-cli",
        ) {
            let p = problems(DEB, &deb(&deb_control_items(), &items, kind));
            assert!(!p.is_empty(), ".deb (data.tar {kind:?}) with {what} passed");
            assert_flags(&p, &needles);
        }
    }
    // control.tar.
    assert_flags(
        &problems(
            DEB,
            &deb(
                &plus(&deb_control_items(), file("stock", rom_bytes(160 << 10))),
                &clean,
                "xz",
            ),
        ),
        &[
            "control.tar.gz: stock: unexpected file",
            "ROM-sized: 163840 bytes",
        ],
    );
    let control = gzip(&tar(&dot_slash(&deb_control_items())));
    let data = xz(&tar(&dot_slash(&clean)));
    // An extra member, an unreadable compression, a corrupt data.tar, and
    // a package without a data.tar.
    assert_flags(
        &problems(
            DEB,
            &ar(&[
                ("debian-binary", b"2.0\n".to_vec()),
                ("control.tar.gz", control.clone()),
                ("data.tar.xz", data.clone()),
                ("stock.bin", rom_bytes(512 << 10)),
            ]),
        ),
        &[
            "stock.bin: unexpected .deb member",
            "ROM image extension `.bin`",
            "ROM-sized",
        ],
    );
    assert_flags(
        &problems(
            DEB,
            &ar(&[
                ("debian-binary", b"2.0\n".to_vec()),
                ("control.tar.gz", control.clone()),
                ("data.tar.bz2", data.clone()),
            ]),
        ),
        &["data.tar.bz2: unexpected .deb member"],
    );
    assert_flags(
        &problems(
            DEB,
            &ar(&[
                ("debian-binary", b"2.0\n".to_vec()),
                ("control.tar.gz", control.clone()),
                ("data.tar.xz", data[..data.len() / 2].to_vec()),
            ]),
        ),
        &["data.tar.xz: cannot be read completely"],
    );
    assert_flags(
        &problems(
            DEB,
            &ar(&[("debian-binary", b"2.0\n".to_vec()), ("x.bin", vec![1])]),
        ),
        &["has no control.tar or data.tar member"],
    );
    assert_flags(
        &problems(DEB, b"not an ar archive"),
        &["cannot be read completely"],
    );
}

/// US-RR-38-8: each planted ROM / fixture fails the AppImage, in every
/// SquashFS compression; so does data hidden after the payload.
#[test]
fn appimage_with_planted_rom_or_fixture_fails() {
    let clean = appimage_items();
    for comp in [Comp::None, Comp::Gzip, Comp::Xz, Comp::Zstd] {
        for (what, items, needles) in plants(
            &clean,
            "usr/bin/romraider-cli",
            "usr/share/licenses/romraider-cli/license.txt",
            "usr/share/romraider-cli",
        ) {
            let p = problems(APPIMAGE, &appimage(&items, comp));
            assert!(!p.is_empty(), "AppImage ({comp:?}) with {what} passed");
            assert_flags(&p, &needles);
        }
    }
    // .DirIcon must be the icon symlink.
    assert_flags(
        &problems(
            APPIMAGE,
            &appimage(
                &replaced(
                    &clean,
                    ".DirIcon",
                    symlink(".DirIcon", "../../fixtures/rom"),
                ),
                Comp::Zstd,
            ),
        ),
        &[
            "symlink to \"../../fixtures/rom\", expected \"romraider-cli.svg\"",
            "`fixtures` component",
        ],
    );
    assert_flags(
        &problems(
            APPIMAGE,
            &appimage(
                &replaced(&clean, ".DirIcon", file(".DirIcon", rom_bytes(512 << 10))),
                Comp::Zstd,
            ),
        ),
        &["is a file, expected a symlink", "ROM-sized"],
    );
    // A ROM appended after the SquashFS payload (outside the filesystem).
    assert_flags(
        &problems(APPIMAGE, &[clean_appimage(), rom_bytes(1 << 20)].concat()),
        &[
            "1048576 bytes appended after the SquashFS payload",
            "the appended data is ROM-sized",
        ],
    );
    // A truncated payload, and a runtime with no payload at all.
    let a = clean_appimage();
    assert_flags(
        &problems(APPIMAGE, &a[..a.len() - 8192]),
        &["romraider-cli-x86_64.AppImage!squashfs"],
    );
    assert_flags(
        &problems(APPIMAGE, &fake_elf(b"runtime")),
        &["no readable SquashFS payload"],
    );
    assert_flags(
        &problems(APPIMAGE, &rom_bytes(1 << 20)),
        &["the AppImage runtime is not an ELF image"],
    );
}

/// Anything in the release directory the check cannot open fails it, as
/// does a directory with nothing to check.
#[test]
fn unknown_assets_fail_the_release() {
    let dir = scratch("release-artifacts-unknown");
    fs::write(dir.join(TARBALL), clean_tarball()).unwrap();
    fs::write(dir.join("standard.bin"), rom_bytes(2048)).unwrap();
    fs::write(dir.join("romraider-cli-x86_64.zip"), b"PK").unwrap();
    fs::create_dir_all(dir.join("fixtures")).unwrap();
    fs::write(dir.join("SHA256SUMS"), rom_bytes(4096)).unwrap();
    let p = check_dir(&dir).unwrap().problems;
    assert_flags(
        &p,
        &[
            "standard.bin: not a known release asset",
            "romraider-cli-x86_64.zip: not a known release asset",
            "fixtures: is not a regular file",
            "SHA256SUMS: contains NUL bytes",
        ],
    );

    let empty = scratch("release-artifacts-empty");
    fs::write(empty.join("SHA256SUMS"), "").unwrap();
    assert_flags(
        &check_dir(&empty).unwrap().problems,
        &["holds no release artifact to check"],
    );
}

/// `cargo xtask check-release-artifacts` exits 0 on a clean set and 1, with
/// the findings on stderr, when a ROM is planted.
#[test]
fn command_exit_status() {
    let dir = scratch("release-artifacts-cli");
    fs::write(dir.join(DEB), clean_deb()).unwrap();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-release-artifacts")
            .arg(&dir)
            .output()
            .unwrap()
    };
    let ok = run();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("check-release-artifacts: ok"));

    let items = plus(
        &deb_data_items(),
        file("usr/share/romraider-cli/stock.bin", rom_bytes(1 << 20)),
    );
    fs::write(dir.join(DEB), deb(&deb_control_items(), &items, "xz")).unwrap();
    let bad = run();
    assert_eq!(bad.status.code(), Some(1));
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(
        err.contains("usr/share/romraider-cli/stock.bin: ROM image extension `.bin`")
            && err.contains("nothing may be published"),
        "{err}"
    );
}

// --- The allow-lists agree with the packaging recipes ------------------------

#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every file packaging/appimage/build.sh puts in the AppDir is on the
/// AppImage allow-list, and every allow-listed path is one build.sh writes.
#[test]
fn appimage_allow_list_matches_build_sh() {
    let build = repo_file("packaging/appimage/build.sh");
    let mut written: Vec<String> = build
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let (_, rest) = l.split_once("\"$appdir/")?;
            Some(rest.split('"').next()?.to_owned())
        })
        .collect();
    written.sort();
    written.dedup();
    let mut allowed: Vec<String> = Layout::AppImage
        .allowed_files()
        .into_iter()
        .map(str::to_owned)
        .collect();
    allowed.sort();
    assert_eq!(
        written, allowed,
        "packaging/appimage/build.sh vs the AppImage allow-list"
    );
}

/// Every `[package.metadata.deb]` asset of rr-cli lands on the .deb data
/// allow-list; the only other allowed file is the copyright cargo-deb writes
/// from `license-file`.
#[test]
fn deb_allow_list_matches_cargo_deb_assets() {
    let cli: toml::Table = toml::from_str(&repo_file("crates/rr-cli/Cargo.toml")).unwrap();
    let assets = cli["package"]["metadata"]["deb"]["assets"]
        .as_array()
        .unwrap();
    let mut installed: Vec<String> = assets
        .iter()
        .map(|a| {
            let a = a.as_array().unwrap();
            let src = a[0].as_str().unwrap();
            let dest = a[1].as_str().unwrap();
            let name = src.rsplit('/').next().unwrap();
            format!("{}{name}", dest.trim_start_matches('/'))
        })
        .collect();
    installed.push("usr/share/doc/romraider-cli/copyright".to_owned());
    installed.sort();
    let mut allowed: Vec<String> = Layout::DebData
        .allowed_files()
        .into_iter()
        .map(str::to_owned)
        .collect();
    allowed.sort();
    assert_eq!(
        installed, allowed,
        "rr-cli [package.metadata.deb] assets vs the .deb allow-list"
    );
}

/// What cargo-dist puts in the tarball (the binary, dist-workspace.toml
/// `include`, and the auto-included README.md) is the tarball allow-list.
#[test]
fn tarball_allow_list_matches_dist_config() {
    let dist: toml::Table = toml::from_str(&repo_file("dist-workspace.toml")).unwrap();
    let mut shipped: Vec<String> = dist["dist"]["include"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    shipped.push("romraider-cli".to_owned());
    shipped.push("README.md".to_owned());
    shipped.sort();
    let mut allowed: Vec<String> = Layout::Tarball
        .allowed_files()
        .into_iter()
        .map(str::to_owned)
        .collect();
    allowed.sort();
    assert_eq!(
        shipped, allowed,
        "cargo-dist tarball contents vs the tarball allow-list"
    );
}

// --- Nothing is compiled into the shipped binary (US-RR-38-4) ----------------

/// The macros that compile a file's contents into the crate.
const INCLUDE_MACROS: &[&str] = &["include_bytes", "include_str", "include"];

/// `include*!` uses in non-test code that could put a ROM or fixture into
/// the binary: every `include_bytes!` (nothing on the allow-lists needs a
/// binary blob compiled in, and a ROM inside a well-formed ELF section
/// passes the artifact check), and any `include_str!` / `include!` of a
/// path with a fixture / corpus component or a ROM or archive extension.
/// `#[cfg(test)]` items do not ship and are skipped (`cfg(not(test))` is
/// not). Macro bodies are searched too.
#[expect(
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn embed_findings(path: &str, src: &str) -> Vec<String> {
    use syn::visit::{self, Visit};

    fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            let syn::Meta::List(list) = &a.meta else {
                return false;
            };
            let t = list.tokens.to_string();
            a.path().is_ident("cfg") && t.contains("test") && !t.contains("not")
        })
    }

    fn flag(found: &mut Vec<String>, path: &str, line: usize, mac: &str, arg: &str) {
        let target = arg.trim_matches('"');
        let marked = target.split('/').any(|c| {
            xtask::release_artifacts::FIXTURE_DIRS.contains(&c.to_ascii_lowercase().as_str())
        }) || target
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .is_some_and(|e| {
                xtask::release_artifacts::ROM_EXTENSIONS.contains(&e.as_str())
                    || xtask::release_artifacts::ARCHIVE_EXTENSIONS.contains(&e.as_str())
            });
        if mac == "include_bytes" || marked {
            found.push(format!(
                "{path}:{line}: `{mac}!({arg})` compiles a file into romraider-cli"
            ));
        }
    }

    /// Nested `include*!(...)` uses inside a macro's tokens.
    fn scan_tokens(found: &mut Vec<String>, path: &str, ts: proc_macro2::TokenStream) {
        use proc_macro2::TokenTree;
        let tokens: Vec<TokenTree> = ts.into_iter().collect();
        for (i, t) in tokens.iter().enumerate() {
            match t {
                TokenTree::Ident(id) if INCLUDE_MACROS.contains(&id.to_string().as_str()) => {
                    if let (Some(TokenTree::Punct(p)), Some(TokenTree::Group(g))) =
                        (tokens.get(i + 1), tokens.get(i + 2))
                        && p.as_char() == '!'
                    {
                        let arg = g.stream().into_iter().next().map(|a| a.to_string());
                        flag(
                            found,
                            path,
                            id.span().start().line,
                            &id.to_string(),
                            &arg.unwrap_or_default(),
                        );
                    }
                }
                TokenTree::Group(g) => scan_tokens(found, path, g.stream()),
                _ => {}
            }
        }
    }

    struct Scan<'p> {
        path: &'p str,
        found: Vec<String>,
    }

    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
            if !is_cfg_test(&i.attrs) {
                visit::visit_item_mod(self, i);
            }
        }
        fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
            if !is_cfg_test(&i.attrs) {
                visit::visit_item_fn(self, i);
            }
        }
        fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
            if !is_cfg_test(&i.attrs) {
                visit::visit_item_impl(self, i);
            }
        }
        fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
            if !is_cfg_test(&i.attrs) {
                visit::visit_item_const(self, i);
            }
        }
        fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
            if !is_cfg_test(&i.attrs) {
                visit::visit_item_static(self, i);
            }
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            if let Some(last) = m.path.segments.last() {
                let name = last.ident.to_string();
                if INCLUDE_MACROS.contains(&name.as_str()) {
                    let arg = m.tokens.clone().into_iter().next().map(|a| a.to_string());
                    let line = last.ident.span().start().line;
                    flag(
                        &mut self.found,
                        self.path,
                        line,
                        &name,
                        &arg.unwrap_or_default(),
                    );
                }
            }
            scan_tokens(&mut self.found, self.path, m.tokens.clone());
            visit::visit_macro(self, m);
        }
    }

    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("{path}: cannot parse: {e}"));
    if is_cfg_test(&file.attrs) {
        return Vec::new();
    }
    let mut scan = Scan {
        path,
        found: Vec::new(),
    };
    scan.visit_file(&file);
    scan.found
}

/// The directories of the workspace crates compiled into romraider-cli:
/// rr-cli and every workspace crate it reaches over normal or build
/// dependencies.
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn shipped_crate_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let meta = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .unwrap();
    let pkgs = meta.workspace_packages();
    let mut todo = vec!["rr-cli".to_owned()];
    let mut seen: Vec<String> = Vec::new();
    let mut dirs = Vec::new();
    while let Some(name) = todo.pop() {
        if seen.contains(&name) {
            continue;
        }
        let p = pkgs
            .iter()
            .find(|p| p.name.as_str() == name)
            .unwrap_or_else(|| panic!("workspace crate {name} not found"));
        dirs.push(p.manifest_path.parent().unwrap().as_std_path().to_owned());
        for d in &p.dependencies {
            if d.path.is_some() && d.kind != cargo_metadata::DependencyKind::Development {
                todo.push(d.name.clone());
            }
        }
        seen.push(name);
    }
    dirs
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// US-RR-38-4: no ROM or fixture is compiled into the binary the artifacts
/// ship. The artifact check exempts a well-formed ELF image from the size
/// rules, so a ROM inside one of its sections (`include_bytes!`) would pass
/// it; this closes that from the source side, over every `src/` file and
/// `build.rs` of every workspace crate romraider-cli is built from.
#[test]
fn shipped_crates_compile_in_no_rom_or_fixture() {
    let dirs = shipped_crate_dirs();
    assert!(
        dirs.iter().any(|d| d.ends_with("crates/rr-cli")),
        "rr-cli not among {dirs:?}"
    );
    let mut files = Vec::new();
    for d in &dirs {
        rust_files(&d.join("src"), &mut files);
        if d.join("build.rs").is_file() {
            files.push(d.join("build.rs"));
        }
    }
    assert!(!files.is_empty());
    let found: Vec<String> = files
        .iter()
        .flat_map(|f| embed_findings(&f.display().to_string(), &fs::read_to_string(f).unwrap()))
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The source scan bites on each way a ROM or fixture can be compiled in,
/// and leaves test-only and harmless includes alone.
#[test]
fn embed_scan_rejects_compiled_in_roms() {
    let bad = [
        "static ROM: &[u8] = include_bytes!(\"../../../fixtures/public/synthetic/standard.bin\");",
        "const LOGO: &[u8] = include_bytes!(\"logo.png\");",
        "const D: &str = include_str!(\"../roms/USDM/stock.hex\");",
        "const K: &str = include_str!(\"../../fixtures/kat/u8-le.toml\");",
        "fn f() -> Vec<&'static [u8]> { vec![include_bytes!(\"a\")] }",
        "#[cfg(not(test))] mod m { const X: &[u8] = std::include_bytes!(\"x\"); }",
        "#[cfg(test)] mod t {}\nconst X: &[u8] = include_bytes!(\"x\");",
    ];
    for src in bad {
        assert!(
            !embed_findings("x.rs", src).is_empty(),
            "not flagged: {src}"
        );
    }
    let ok = [
        "#[cfg(test)] mod t { const X: &[u8] = include_bytes!(\"../../fixtures/x.bin\"); }",
        "#![cfg(test)]\nconst X: &[u8] = include_bytes!(\"x.bin\");",
        "#[doc = include_str!(\"../README.md\")] pub struct S;",
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));",
        "// const X: &[u8] = include_bytes!(\"x.bin\");\nconst S: &str = \"include_bytes!(x)\";",
    ];
    for src in ok {
        assert_eq!(embed_findings("x.rs", src), Vec::<String>::new(), "{src}");
    }
}
