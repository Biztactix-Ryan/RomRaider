//! `fixtures/corpus-coverage.toml` and `cargo xtask gen-corpus-coverage`
//! (US-RR-16-6; D-025, D-028). Reading, legacy matching, `base`
//! resolution, the probe and the stale-report comparison are unit-tested in
//! `xtask/src/corpus_coverage.rs` on synthetic definitions and images.
//!
//! Here, without the corpus: the committed report parses, is its own
//! canonical rendering, names the manifest's pins, lists every manifest
//! image in order with a status its definitions bear out, has totals that
//! add up, still agrees with the coverage figures D-028 recorded (matched on
//! `filename_rom_id`), flags the 2004 ADM Liberty images, holds metadata
//! only, and git keeps its `\n` endings. That it matches a fresh generation
//! needs the fetched corpus: the CI `corpus` job runs
//! `cargo xtask gen-corpus-coverage --check` (`corpus_ci_config.rs`).
//!
//! The command itself runs end to end against a synthetic corpus and
//! manifest in the system temp directory (never real ROMs or definitions,
//! never inside the repository): it writes the report, `--check` passes on
//! it, and fails on a stale or missing report, a corpus file that differs
//! from the manifest, and a manifest without `[defs]`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::corpus_coverage::{self, Coverage, EngineClass, Status, is_liberty_2004_adm, is_xml};
use xtask::corpus_manifest::{
    self, DefFile, DefsCorpus, LoggerPackage, Manifest, RomCorpus, RomFile, Suspect, SuspectKind,
    sha256_hex,
};
use xtask::fetch_corpus::defs_dir;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn committed() -> anyhow::Result<(String, Coverage, Manifest)> {
    let text = fs::read_to_string(workspace_root().join(corpus_coverage::COMMITTED_PATH))?;
    let c = Coverage::from_toml(&text)?;
    let m = Manifest::from_toml(&fs::read_to_string(
        workspace_root().join(corpus_manifest::COMMITTED_PATH),
    )?)?;
    Ok((text, c, m))
}

#[test]
fn committed_report_is_canonical_and_lists_every_manifest_image() -> anyhow::Result<()> {
    let (text, c, m) = committed()?;
    assert_eq!(
        c.render()?,
        text,
        "not the canonical rendering: regenerate it"
    );
    let defs = m.defs.as_ref().expect("the committed manifest has [defs]");
    assert_eq!(c.roms_commit, m.roms.commit);
    assert_eq!(c.defs_commit, defs.commit);

    assert_eq!(c.roms.len(), m.roms.files.len());
    let def_files: BTreeSet<&str> = defs.files.iter().map(|f| f.path.as_str()).collect();
    for (r, f) in c.roms.iter().zip(&m.roms.files) {
        assert_eq!(r.path, f.path, "manifest order");
        assert_eq!(r.filename_rom_id, f.filename_rom_id, "{}", r.path);
        assert_eq!(r.suspect, f.suspect.as_ref().map(|s| s.kind), "{}", r.path);
        assert_eq!(
            r.image_offset,
            f.suspect.as_ref().and_then(|s| s.image_offset),
            "{}",
            r.path
        );
        let usable = r.definitions.iter().any(|d| d.base_error.is_none());
        let want = match (usable, r.definitions.is_empty()) {
            (true, _) => Status::Matched,
            (false, true) => Status::NoDefinition,
            (false, false) => Status::UnresolvedBase,
        };
        assert_eq!(r.status, want, "{}", r.path);
        for d in &r.definitions {
            assert!(
                def_files.contains(d.file.as_str()),
                "{}: {} is not a pinned definition file",
                r.path,
                d.file
            );
        }
        let files: Vec<&str> = r.definitions.iter().map(|d| d.file.as_str()).collect();
        assert!(
            files.windows(2).all(|w| w[0] < w[1]),
            "{}: definitions sorted, one per file",
            r.path
        );
        if r.status == Status::Matched {
            assert!(
                r.image_rom_id.is_some() && r.image_rom_id_address.is_some(),
                "{}",
                r.path
            );
        }
        assert_eq!(
            r.liberty_2004_adm.is_some(),
            is_liberty_2004_adm(f),
            "{}",
            r.path
        );
    }
    let xml: Vec<&str> = defs
        .files
        .iter()
        .map(|f| f.path.as_str())
        .filter(|p| is_xml(p))
        .collect();
    let listed: Vec<&str> = c.definition_files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(listed, xml, "every .xml definition file, in manifest order");
    Ok(())
}

#[test]
fn committed_totals_add_up() -> anyhow::Result<()> {
    let (_, c, _) = committed()?;
    let s = &c.summary;
    let count = |st: Status| c.roms.iter().filter(|r| r.status == st).count();
    assert_eq!(s.images, c.roms.len());
    assert_eq!(s.matched, count(Status::Matched));
    assert_eq!(s.unresolved_base, count(Status::UnresolvedBase));
    assert_eq!(s.no_definition, count(Status::NoDefinition));
    assert_eq!(s.matched + s.unresolved_base + s.no_definition, s.images);
    assert_eq!(
        s.suspect,
        c.roms.iter().filter(|r| r.suspect.is_some()).count()
    );
    assert_eq!(s.regions.iter().map(|r| r.images).sum::<usize>(), s.images);
    assert_eq!(
        s.regions.iter().map(|r| r.matched).sum::<usize>(),
        s.matched
    );
    assert_eq!(
        s.regions
            .iter()
            .map(|r| r.filename_id_has_definition)
            .sum::<usize>(),
        s.filename_id_has_definition
    );
    let per_file: usize = c.definition_files.iter().map(|f| f.matched_images).sum();
    let per_rom: usize = c.roms.iter().map(|r| r.definitions.len()).sum();
    assert_eq!(per_file, per_rom);
    Ok(())
}

/// D-028 recorded, from a throwaway clone matched on `filename_rom_id`:
/// 455 of 501 images, ADM 69/72, EDM 69/77, JDM 99/113, SADM 14/15, USDM
/// 203/222. The report's file-name-ID counts must reproduce that. Matching
/// the image bytes as legacy does gives 456: +1 EDM (A8DH2Z1Z's image says
/// `A8DH20  `), +1 with no region in its name (the Baja `A2WC50-A2WC50ZU`
/// image says `A2WC50  `), -1 USDM (AE5L500V: its only definition,
/// `ecu/AE5L500V_RR.xml`, has a `<rom>` root, which legacy never matches).
#[test]
fn committed_report_reconciles_with_d028() -> anyhow::Result<()> {
    let (_, c, _) = committed()?;
    let s = &c.summary;
    assert_eq!((s.images, s.filename_id_has_definition), (501, 455));
    let region = |name: Option<&str>| {
        s.regions
            .iter()
            .find(|r| r.region.as_deref() == name)
            .map(|r| (r.filename_id_has_definition, r.images, r.matched))
    };
    assert_eq!(region(Some("ADM")), Some((69, 72, 69)));
    assert_eq!(region(Some("EDM")), Some((69, 77, 70)));
    assert_eq!(region(Some("JDM")), Some((99, 113, 99)));
    assert_eq!(region(Some("SADM")), Some((14, 15, 14)));
    assert_eq!(region(Some("USDM")), Some((203, 222, 202)));
    assert_eq!(region(None), Some((1, 2, 2)));
    assert_eq!(s.matched, 456);
    let status = |p: &str| c.roms.iter().find(|r| r.path.contains(p)).map(|r| r.status);
    assert_eq!(status("A8DH2Z1Z"), Some(Status::Matched));
    assert_eq!(status("A2WC50-A2WC50ZU"), Some(Status::Matched));
    assert_eq!(status("AE5L500V"), Some(Status::NoDefinition));
    Ok(())
}

/// Story US-RR-16: are the 2004 AU Liberty NA ROM IDs covered? D-028: all
/// eight 2004 ADM Liberty images are (seven turbo GTs and the 3.0R
/// D2WD610H), and neither corpus has a 2004 ADM Liberty 2.0i/2.5i.
#[test]
fn committed_report_flags_the_2004_adm_liberty() -> anyhow::Result<()> {
    let (_, c, _) = committed()?;
    let l = &c.liberty_2004_adm;
    assert_eq!((l.images, l.matched), (8, 8));
    assert_eq!(
        l.turbo,
        [
            "A2WD001G", "A2WD010G", "A2WD012G", "A2ZJ800G", "A2ZJC00G", "A2ZJD00G", "A2ZJD02G"
        ]
    );
    assert_eq!(l.na_h6, ["D2WD610H"]);
    assert!(l.na_4cyl.is_empty() && l.unknown.is_empty() && l.no_definition.is_empty());
    assert!(l.definitions_na_4cyl.is_empty());
    for id in l.turbo.iter().chain(&l.na_h6) {
        assert!(
            l.definitions.contains(id),
            "{id}: no 2004 ADM Liberty <romid>"
        );
    }
    let flagged: Vec<_> = c
        .roms
        .iter()
        .filter_map(|r| r.liberty_2004_adm.map(|e| (r, e)))
        .collect();
    assert_eq!(flagged.len(), 8);
    assert!(flagged.iter().all(|(r, _)| r.status == Status::Matched));
    assert!(
        flagged
            .iter()
            .any(|(r, e)| *e == EngineClass::NaH6 && r.path.contains("D2WD610H"))
    );
    Ok(())
}

/// D-028: the definitions have no licence, so the report is metadata only.
#[test]
fn committed_report_holds_no_definition_content() -> anyhow::Result<()> {
    let (text, _, _) = committed()?;
    for word in [
        "storageaddress",
        "<table",
        "<scaling",
        "toexpr",
        "frexpr",
        "<checksum",
    ] {
        assert!(
            !text.to_ascii_lowercase().contains(word),
            "the report contains `{word}`"
        );
    }
    Ok(())
}

/// US-RR-16-3: the report lists every corpus image exactly once, each with
/// a ROM ID and either its matching definitions (file and `xmlid`) or none.
#[test]
fn committed_report_gives_every_image_a_rom_id_and_its_definitions() -> anyhow::Result<()> {
    let (_, c, m) = committed()?;
    let listed: Vec<&str> = c.roms.iter().map(|r| r.path.as_str()).collect();
    let manifest: Vec<&str> = m.roms.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(listed, manifest, "one entry per manifest image, in order");
    let unique: BTreeSet<&str> = listed.iter().copied().collect();
    assert_eq!(unique.len(), listed.len(), "no image listed twice");

    for r in &c.roms {
        let id = r.image_rom_id.as_deref().or(r.filename_rom_id.as_deref());
        assert!(
            id.is_some_and(|id| !id.trim().is_empty()),
            "{}: no ROM ID",
            r.path
        );
        match r.status {
            Status::Matched => {
                assert!(
                    r.definitions.iter().any(|d| {
                        d.base_error.is_none()
                            && !d.file.is_empty()
                            && d.xmlid.as_deref().is_some_and(|x| !x.is_empty())
                    }),
                    "{}: matched, but no loadable definition with file and xmlid",
                    r.path
                );
            }
            Status::NoDefinition => {
                assert!(r.definitions.is_empty(), "{}: no-definition", r.path);
            }
            Status::UnresolvedBase => {
                assert!(
                    !r.definitions.is_empty()
                        && r.definitions.iter().all(|d| d.base_error.is_some()),
                    "{}: unresolved-base",
                    r.path
                );
            }
        }
    }
    // The pinned corpus has no unresolved-base image: every one is listed as
    // matched or 'no-definition'.
    assert!(c.roms.iter().all(|r| r.status != Status::UnresolvedBase));
    let n = |st: Status| c.roms.iter().filter(|r| r.status == st).count();
    assert_eq!(
        (c.summary.images, c.summary.matched, c.summary.no_definition),
        (c.roms.len(), n(Status::Matched), n(Status::NoDefinition))
    );
    Ok(())
}

/// D-028, mechanically: every key in the report is one of the metadata keys
/// its format defines, and no value is a fragment of definition XML.
#[test]
fn committed_report_has_metadata_keys_only() -> anyhow::Result<()> {
    const KEYS: &[&str] = &[
        "schema",
        "roms_commit",
        "defs_commit",
        "summary",
        "images",
        "matched",
        "unresolved_base",
        "no_definition",
        "suspect",
        "filename_id_has_definition",
        "image_id_differs_from_filename",
        "regions",
        "region",
        "liberty_2004_adm",
        "turbo",
        "na_h6",
        "na_4cyl",
        "unknown",
        "definitions",
        "definitions_na_4cyl",
        "definition_files",
        "path",
        "root",
        "roms",
        "with_id",
        "matched_images",
        "problems",
        "filename_rom_id",
        "image_rom_id",
        "image_rom_id_address",
        "image_rom_id_from",
        "status",
        "image_offset",
        "file",
        "xmlid",
        "base_error",
    ];
    fn walk(v: &toml::Value, at: &str, keys: &mut BTreeSet<String>, strings: &mut Vec<String>) {
        match v {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    keys.insert(k.clone());
                    walk(v, &format!("{at}.{k}"), keys, strings);
                }
            }
            toml::Value::Array(a) => a.iter().for_each(|v| walk(v, at, keys, strings)),
            toml::Value::String(s) => strings.push(format!("{at}: {s}")),
            _ => {}
        }
    }
    let (text, _, _) = committed()?;
    let (mut keys, mut strings) = (BTreeSet::new(), Vec::new());
    walk(
        &toml::Value::Table(toml::from_str(&text)?),
        "",
        &mut keys,
        &mut strings,
    );
    let extra: Vec<&String> = keys
        .iter()
        .filter(|k| !KEYS.contains(&k.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "keys outside the metadata format: {extra:?}"
    );
    for s in &strings {
        assert!(s.len() <= 400, "a long value: {s}");
        assert!(
            !s.contains("=\"") && !s.contains("</") && !s.contains("/>"),
            "an XML fragment: {s}"
        );
    }
    Ok(())
}

/// US-RR-16-3: the report is committed, so git must not ignore it.
#[test]
fn git_does_not_ignore_the_report() -> anyhow::Result<()> {
    let out = Command::new("git")
        .args([
            "check-ignore",
            "--no-index",
            "-v",
            "--",
            corpus_coverage::COMMITTED_PATH,
        ])
        .current_dir(workspace_root())
        .output()?;
    assert_eq!(
        out.status.code(),
        Some(1),
        "ignored: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

#[test]
fn git_keeps_the_report_bytes_unconverted() -> anyhow::Result<()> {
    let out = Command::new("git")
        .args([
            "check-attr",
            "text",
            "eol",
            "--",
            corpus_coverage::COMMITTED_PATH,
        ])
        .current_dir(workspace_root())
        .output()?;
    assert!(out.status.success(), "git check-attr failed");
    let attrs = String::from_utf8(out.stdout)?;
    let p = corpus_coverage::COMMITTED_PATH;
    assert!(attrs.contains(&format!("{p}: text: set")), "{attrs}");
    assert!(attrs.contains(&format!("{p}: eol: lf")), "{attrs}");
    Ok(())
}

// ---------------------------------------------------------------------------
// The command, end to end, on a synthetic corpus
// ---------------------------------------------------------------------------

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rr-corpus-coverage-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 160 KiB (standard size) synthetic image with `id` at 0x2000.
fn image(id: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF; 0x2_8000];
    v[0x2000..0x2000 + id.len()].copy_from_slice(id);
    v
}

const DEF: &str = "<?xml version=\"1.0\"?>\n<!-- synthetic -->\n<roms>\n\
<rom><romid><xmlid>BASE</xmlid></romid></rom>\n\
<rom base=\"BASE\"><romid><xmlid>SYN0001A</xmlid><internalidaddress>2000</internalidaddress><internalidstring>SYN0001A</internalidstring></romid></rom>\n\
</roms>\n";

struct Corpus {
    dir: PathBuf,
    manifest: PathBuf,
    report: PathBuf,
}

/// Writes a corpus (a matched image, an unmatched one, a container whose
/// embedded image matches) and its manifest; `defs: false` leaves out
/// `[defs]`.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn synthetic(name: &str, defs: bool) -> Corpus {
    let root = scratch(name);
    let dir = root.join("corpus");
    let mut files = Vec::new();
    let mut put = |path: &str, bytes: &[u8], id: Option<&str>, suspect: Option<Suspect>| {
        let p = dir.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        files.push(RomFile {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            filename_rom_id: id.map(str::to_owned),
            year: None,
            year_end: None,
            region: Some("JDM".to_owned()),
            model: None,
            trim: None,
            suspect,
        });
    };
    put(
        "JDM/A/SYN0001A.hex",
        &image(b"SYN0001A"),
        Some("SYN0001A"),
        None,
    );
    put(
        "JDM/A/SYN0002B.hex",
        &image(b"SYN0002B"),
        Some("SYN0002B"),
        None,
    );
    let container = [vec![0u8; 155], image(b"SYN0001A")].concat();
    put(
        "JDM/B/SYN0001A-container.hex",
        &container,
        Some("SYN0001A"),
        Some(Suspect {
            kind: SuspectKind::Container,
            reason: "synthetic container".to_owned(),
            image_offset: Some(155),
            image_size: Some(0x2_8000),
        }),
    );
    let defs = defs.then(|| {
        let def_dir = defs_dir(&dir);
        fs::create_dir_all(def_dir.join("ecu")).unwrap();
        fs::write(def_dir.join("ecu/syn.xml"), DEF).unwrap();
        fs::write(def_dir.join("ecu/ecu_defs.dtd"), "<!-- not xml -->").unwrap();
        DefsCorpus {
            upstream: "https://example.invalid/defs".to_owned(),
            branch: "main".to_owned(),
            commit: "1".repeat(40),
            path_prefix: "RomRaider/".to_owned(),
            licence: "synthetic".to_owned(),
            logger_v370: LoggerPackage {
                file: "logger.zip".to_owned(),
                size: 1,
                sha256: "0".repeat(64),
                url: "https://example.invalid/logger.zip".to_owned(),
                note: "synthetic".to_owned(),
            },
            files: vec![
                DefFile {
                    path: "ecu/ecu_defs.dtd".to_owned(),
                    size: 16,
                    sha256: sha256_hex(b"<!-- not xml -->"),
                },
                DefFile {
                    path: "ecu/syn.xml".to_owned(),
                    size: DEF.len() as u64,
                    sha256: sha256_hex(DEF.as_bytes()),
                },
            ],
        }
    });
    let m = Manifest {
        schema: corpus_manifest::SCHEMA,
        roms: RomCorpus {
            upstream: "https://example.invalid/roms".to_owned(),
            commit: "2".repeat(40),
            path_prefix: String::new(),
            files,
        },
        defs,
    };
    m.validate().unwrap();
    let manifest = root.join("corpus-manifest.toml");
    fs::write(&manifest, m.render().unwrap()).unwrap();
    Corpus {
        dir,
        manifest,
        report: root.join("corpus-coverage.toml"),
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn run(c: &Corpus, check: bool) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xtask"));
    cmd.arg("gen-corpus-coverage");
    if check {
        cmd.arg("--check");
    }
    let out = cmd
        .arg("--dir")
        .arg(&c.dir)
        .arg("--manifest")
        .arg(&c.manifest)
        .arg("--coverage")
        .arg(&c.report)
        .env_remove("RR_CORPUS_DIR")
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

#[test]
fn generates_then_checks_a_synthetic_corpus() {
    let c = synthetic("ok", true);
    let (code, out) = run(&c, true);
    assert_eq!(code, 1, "no report yet: {out}");
    assert!(out.contains("cannot read"), "{out}");

    let (code, out) = run(&c, false);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("3 image(s)") && out.contains("2 matched") && out.contains("1 no definition"),
        "{out}"
    );
    let report = Coverage::from_toml(&fs::read_to_string(&c.report).unwrap()).unwrap();
    let by = |p: &str| report.roms.iter().find(|r| r.path == p).unwrap();
    let m = by("JDM/A/SYN0001A.hex");
    assert_eq!(m.status, Status::Matched);
    assert_eq!(m.definitions[0].file, "ecu/syn.xml");
    assert_eq!(m.definitions[0].xmlid.as_deref(), Some("SYN0001A"));
    let n = by("JDM/A/SYN0002B.hex");
    assert_eq!(
        (n.status, n.image_rom_id.as_deref()),
        (Status::NoDefinition, Some("SYN0002B"))
    );
    let s = by("JDM/B/SYN0001A-container.hex");
    assert_eq!(
        (s.status, s.suspect, s.image_offset),
        (Status::Matched, Some(SuspectKind::Container), Some(155))
    );
    assert_eq!(
        report.definition_files.len(),
        1,
        "the .dtd is not a definition"
    );

    let (code, out) = run(&c, true);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("byte-identical"), "{out}");
    let (code, out) = run(&c, false);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("unchanged"), "{out}");

    // Stale: the committed report says the unmatched image has a definition.
    let text = fs::read_to_string(&c.report).unwrap();
    let stale = text.replacen("status = \"no-definition\"", "status = \"matched\"", 1);
    assert_ne!(stale, text);
    fs::write(&c.report, &stale).unwrap();
    let (code, out) = run(&c, true);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("JDM/A/SYN0002B.hex: committed Matched"),
        "{out}"
    );
    assert_eq!(
        fs::read_to_string(&c.report).unwrap(),
        stale,
        "--check never writes"
    );
}

#[test]
fn a_corpus_file_that_differs_from_the_manifest_fails() {
    let c = synthetic("corrupt", true);
    let p = c.dir.join("JDM/A/SYN0002B.hex");
    let mut b = fs::read(&p).unwrap();
    b[0] ^= 1;
    fs::write(&p, b).unwrap();
    let (code, out) = run(&c, false);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("fetch-corpus"), "{out}");
    assert!(!c.report.exists());

    let c = synthetic("corrupt-def", true);
    fs::write(
        defs_dir(&c.dir).join("ecu/syn.xml"),
        DEF.replace("SYN0001A", "SYN0002B"),
    )
    .unwrap();
    let (code, out) = run(&c, false);
    assert_ne!(code, 0, "{out}");
    assert!(
        out.contains("syn.xml") && out.contains("fetch-corpus"),
        "{out}"
    );
}

#[test]
fn a_manifest_without_defs_fails() {
    let c = synthetic("no-defs", false);
    let (code, out) = run(&c, false);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no [defs]"), "{out}");
}
