//! The read-only corpus loader (US-RR-11-9, D-025): it refuses a corpus
//! directory inside the repository working tree, reports a corpus that is
//! not fetched distinctly, reads the committed schema-1 manifest, and makes
//! callers handle suspect entries.
//!
//! Every corpus here is synthetic (a few pseudo-random bytes) in the system
//! temp directory; no real ROM image is read or written, and nothing is
//! created inside the repository.

use std::fs;
use std::path::{Path, PathBuf};

use rr_testkit::corpus::{
    Corpus, CorpusError, DIR_ENV, Entry, Image, MANIFEST_PATH, MANIFEST_SCHEMA, Manifest,
    RepoGuardError, STANDARD_SIZES, Suspect, SuspectKind,
};
use rr_testkit::fixtures::repo_root;

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rr-testkit-corpus-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

const A: &str = "ADM/Forester/A-2005-ADM-Subaru-Forester.hex";
const B: &str = "JDM/Legacy/B-2005-JDM-Subaru-Legacy.hex";

/// A two-entry manifest: `A` clean (4 bytes), `B` suspect (6 bytes).
fn manifest_text(schema: &str, path_a: &str) -> String {
    format!(
        r#"schema = {schema}

[roms]
upstream = "https://example.invalid/corpus"
commit = "0123456789abcdef0123456789abcdef01234567"
path_prefix = ""

[[roms.files]]
path = "{path_a}"
size = 4
sha256 = "{zero}"
region = "ADM"

[[roms.files]]
path = "{B}"
size = 6
sha256 = "{zero}"

[roms.files.suspect]
kind = "container"
reason = "synthetic"
image_offset = 2
image_size = 4
"#,
        zero = "0".repeat(64)
    )
}

/// A synthetic repo, a manifest outside it, and a populated corpus dir.
struct Setup {
    base: PathBuf,
    repo: PathBuf,
    manifest: PathBuf,
    corpus: PathBuf,
}

#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn setup(name: &str) -> Setup {
    let base = scratch(name);
    let repo = base.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir_all(repo.join("target")).unwrap();
    let manifest = base.join("manifest.toml");
    fs::write(&manifest, manifest_text("1", A)).unwrap();
    let corpus = base.join("corpus");
    for (path, bytes) in [(A, &b"\x01\x02\x03\x04"[..]), (B, b"HD\x05\x06\x07\x08")] {
        let p = corpus.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    Setup {
        base,
        repo,
        manifest,
        corpus,
    }
}

fn is_inside(r: &Result<Corpus, CorpusError>) -> bool {
    matches!(
        r,
        Err(CorpusError::InsideRepo(RepoGuardError::Inside { .. }))
    )
}

#[test]
fn a_directory_inside_or_containing_the_repo_is_refused_before_anything_is_read() {
    let s = setup("inside");
    // A manifest that does not exist: the repository check comes first.
    let no_manifest = s.base.join("no-such-manifest.toml");
    for dir in [
        s.repo.clone(),
        s.repo.join("target"),
        s.repo.join("target/corpus/not-yet"),
        s.base.join("corpus/../repo/x"),
        s.repo.join("target/../../repo/y"),
    ] {
        let r = Corpus::open_in(&dir, &no_manifest, &s.repo);
        assert!(is_inside(&r), "{}: {r:?}", dir.display());
    }
    let r = Corpus::open_in(&s.base, &no_manifest, &s.repo);
    assert!(
        matches!(
            r,
            Err(CorpusError::InsideRepo(RepoGuardError::Contains { .. }))
        ),
        "{r:?}"
    );
    let msg = Corpus::open_in(s.repo.join("c"), &s.manifest, &s.repo)
        .unwrap_err()
        .to_string();
    assert!(msg.contains("inside the repository working tree"), "{msg}");
    assert!(msg.contains(DIR_ENV), "{msg}");
    assert!(!s.repo.join("c").exists());
    // Outside: fine.
    assert!(Corpus::open_in(&s.corpus, &s.manifest, &s.repo).is_ok());
    fs::remove_dir_all(&s.base).unwrap();
}

#[cfg(unix)]
#[test]
fn a_symlink_into_the_repo_is_refused() {
    let s = setup("symlink");
    let link = s.base.join("link");
    std::os::unix::fs::symlink(s.repo.join("target"), &link).unwrap();
    assert!(is_inside(&Corpus::open_in(&link, &s.manifest, &s.repo)));
    assert!(is_inside(&Corpus::open_in(
        link.join("corpus"),
        &s.manifest,
        &s.repo
    )));
    fs::remove_dir_all(&s.base).unwrap();
}

/// The real repository is protected by `open_at` / `open`: its `target/`
/// (where a careless `RR_CORPUS_DIR=target/corpus` would point) is refused,
/// and nothing is created there.
#[test]
fn this_repository_is_refused() {
    let inside = repo_root()
        .join("target")
        .join(format!("rr-corpus-refused-{}", std::process::id()));
    let r = Corpus::open_at(&inside, repo_root().join(MANIFEST_PATH));
    assert!(is_inside(&r), "{r:?}");
    let r = Corpus::open_at(repo_root(), repo_root().join(MANIFEST_PATH));
    assert!(is_inside(&r), "{r:?}");
    let r = Corpus::open_at(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("relative/../x"),
        repo_root().join(MANIFEST_PATH),
    );
    assert!(is_inside(&r), "{r:?}");
    assert!(!inside.exists());
}

#[test]
fn a_missing_or_empty_directory_is_not_fetched() {
    let s = setup("not-fetched");
    let missing = s.base.join("missing");
    let e = Corpus::open_in(&missing, &s.manifest, &s.repo).unwrap_err();
    assert!(e.is_not_fetched(), "{e:?}");
    let msg = e.to_string();
    assert!(msg.contains("not fetched"), "{msg}");
    assert!(msg.contains("does not exist"), "{msg}");
    assert!(msg.contains("cargo xtask fetch-corpus"), "{msg}");
    assert!(!missing.exists(), "the loader never creates the directory");

    let empty = s.base.join("empty");
    fs::create_dir(&empty).unwrap();
    let e = Corpus::open_in(&empty, &s.manifest, &s.repo).unwrap_err();
    assert!(e.is_not_fetched(), "{e:?}");
    assert!(e.to_string().contains("is empty"), "{e}");

    let file = s.base.join("a-file");
    fs::write(&file, b"x").unwrap();
    let e = Corpus::open_in(&file, &s.manifest, &s.repo).unwrap_err();
    assert!(
        matches!(e, CorpusError::NotADirectory { .. }) && !e.is_not_fetched(),
        "{e:?}"
    );
    // Other errors are not "not fetched".
    let e = Corpus::open_in(&s.repo, &s.manifest, &s.repo).unwrap_err();
    assert!(!e.is_not_fetched());
    fs::remove_dir_all(&s.base).unwrap();
}

#[test]
fn entries_force_clean_and_suspect_apart() {
    let s = setup("entries");
    let corpus = Corpus::open_in(&s.corpus, &s.manifest, &s.repo).unwrap();
    let m = corpus.manifest();
    assert_eq!(m.len(), 2);
    assert_eq!(m.commit(), "0123456789abcdef0123456789abcdef01234567");
    let mut clean = Vec::new();
    let mut suspect = Vec::new();
    for entry in m.entries() {
        match entry {
            Entry::Clean(file) => clean.push(file.path.clone()),
            Entry::Suspect { file, suspect: why } => {
                assert_eq!(why.kind, SuspectKind::Container);
                assert_eq!((why.image_offset, why.image_size), (Some(2), Some(4)));
                suspect.push(file.path.clone());
            }
        }
    }
    assert_eq!(clean, [A]);
    assert_eq!(suspect, [B]);
    assert_eq!(m.clean().map(|f| f.path.as_str()).collect::<Vec<_>>(), [A]);
    assert_eq!(
        m.suspect()
            .map(|(f, _)| f.path.as_str())
            .collect::<Vec<_>>(),
        [B]
    );
    assert_eq!(m.entries().map(|e| e.file().size).sum::<u64>(), 10);
    fs::remove_dir_all(&s.base).unwrap();
}

#[test]
fn read_checks_type_and_size_and_never_writes() {
    let s = setup("read");
    let corpus = Corpus::open_in(&s.corpus, &s.manifest, &s.repo).unwrap();
    let files: Vec<_> = corpus
        .manifest()
        .entries()
        .map(|e| e.file().clone())
        .collect();
    assert_eq!(
        corpus.read(&files[0]).unwrap(),
        Image::Raw(b"\x01\x02\x03\x04".to_vec())
    );
    // US-RR-11-2: a suspect entry's bytes come back as `Image::Suspect`,
    // the whole file, never as a raw image; the embedded image is the
    // manifest's `image_offset` / `image_size` slice.
    match corpus.read(&files[1]).unwrap() {
        Image::Suspect { bytes, suspect } => {
            assert_eq!(bytes, b"HD\x05\x06\x07\x08");
            assert_eq!(suspect.kind, SuspectKind::Container);
            assert_eq!(suspect.embedded(&bytes), Some(&b"\x05\x06\x07\x08"[..]));
        }
        raw @ Image::Raw(_) => panic!("suspect entry read as raw: {raw:?}"),
    }

    fs::write(corpus.path(&files[0]), b"\x01\x02\x03").unwrap();
    let e = corpus.read(&files[0]).unwrap_err();
    assert!(
        matches!(
            e,
            CorpusError::ImageSize {
                want: 4,
                got: 3,
                ..
            }
        ),
        "{e:?}"
    );
    fs::remove_file(corpus.path(&files[0])).unwrap();
    let e = corpus.read(&files[0]).unwrap_err();
    assert!(matches!(e, CorpusError::ImageNotAFile { .. }), "{e:?}");
    #[cfg(unix)]
    {
        // A symlink in the corpus (which could point into the repo) is not
        // followed.
        let target = s.base.join("elsewhere.bin");
        fs::write(&target, b"\x01\x02\x03\x04").unwrap();
        std::os::unix::fs::symlink(&target, corpus.path(&files[0])).unwrap();
        let e = corpus.read(&files[0]).unwrap_err();
        assert!(matches!(e, CorpusError::ImageNotAFile { .. }), "{e:?}");
    }
    fs::remove_dir_all(&s.base).unwrap();
}

/// A suspect entry without an embedded image (blank, CRLF expansion,
/// truncated, unknown) has nothing to decode; one whose slice runs past the
/// bytes given yields nothing rather than panicking.
#[test]
fn embedded_is_the_manifest_slice_or_nothing() {
    let text = manifest_text("1", A).replace("image_offset = 2\nimage_size = 4\n", "");
    let m = Manifest::parse("m.toml", &text).unwrap();
    let (_, none) = m.suspect().next().unwrap();
    assert_eq!(none.embedded(b"HD\x05\x06\x07\x08"), None);
    let m = Manifest::parse("m.toml", &manifest_text("1", A)).unwrap();
    let (_, some): (_, &Suspect) = m.suspect().next().unwrap();
    assert_eq!(
        some.embedded(b"HD\x05\x06\x07\x08"),
        Some(&b"\x05\x06\x07\x08"[..])
    );
    assert_eq!(some.embedded(b"HD\x05\x06\x07"), None);
    assert_eq!(some.embedded(b""), None);
}

#[test]
fn manifest_schema_shape_and_paths_are_checked() {
    let ok = Manifest::parse("m.toml", &manifest_text("1", A)).unwrap();
    assert_eq!(ok.path(), Path::new("m.toml"));
    for schema in ["2", "\"1\""] {
        let e = Manifest::parse("m.toml", &manifest_text(schema, A)).unwrap_err();
        assert!(matches!(e, CorpusError::ManifestSchema { .. }), "{e:?}");
        assert!(
            e.to_string().contains(&format!("schema {MANIFEST_SCHEMA}")),
            "{e}"
        );
    }
    let no_schema = manifest_text("1", A).replacen("schema = 1\n", "", 1);
    let e = Manifest::parse("m.toml", &no_schema).unwrap_err();
    assert!(e.to_string().contains("schema missing"), "{e}");
    let unknown = manifest_text("1", A).replace("region = \"ADM\"", "regoin = \"ADM\"");
    let e = Manifest::parse("m.toml", &unknown).unwrap_err();
    assert!(matches!(e, CorpusError::ManifestToml { .. }), "{e:?}");
    for bad in ["../escape.hex", "/abs.hex", "a/./b.hex", "a\\\\b.hex"] {
        let e = Manifest::parse("m.toml", &manifest_text("1", bad)).unwrap_err();
        assert!(
            matches!(e, CorpusError::ManifestInvalid { .. }),
            "{bad}: {e:?}"
        );
    }
    let e = Manifest::parse("m.toml", "schema = [").unwrap_err();
    assert!(matches!(e, CorpusError::ManifestToml { .. }), "{e:?}");
}

/// The committed manifest (metadata only) loads with this crate's own model:
/// 501 images, the two D-025 suspects among them, every suspect explained.
#[test]
fn the_committed_manifest_loads() {
    let m = Manifest::load(repo_root().join(MANIFEST_PATH)).unwrap();
    assert_eq!(m.len(), 501);
    assert_eq!(m.commit(), "639f3c73c1bd8efee48347bd71bb064211b95242");
    let suspect: Vec<_> = m.suspect().collect();
    assert_eq!(suspect.len(), 2, "{suspect:?}");
    assert!(
        suspect
            .iter()
            .any(|(f, _)| f.path.starts_with("JDM/Legacy/AE5I410A"))
    );
    assert!(suspect.iter().all(|(_, s)| !s.reason.is_empty()));
    assert_eq!(m.clean().count() + suspect.len(), m.len());
}

/// US-RR-11-2 on the committed manifest, read with this loader's own model
/// (xtask's `committed_manifest_marks_every_nonstandard_size_suspect` is the
/// same check through xtask's): every clean entry has a size in
/// [`STANDARD_SIZES`]; every entry that does not is suspect with a reason;
/// the two D-025 suspects are containers whose embedded standard image ends
/// the file.
#[test]
fn every_nonstandard_size_in_the_committed_manifest_is_suspect_with_a_reason() {
    let m = Manifest::load(repo_root().join(MANIFEST_PATH)).unwrap();
    let mut suspects = 0;
    for entry in m.entries() {
        match entry {
            Entry::Clean(file) => assert!(
                STANDARD_SIZES.contains(&file.size),
                "{}: {} bytes is not a standard size but is not marked suspect",
                file.path,
                file.size
            ),
            Entry::Suspect { file, suspect } => {
                suspects += 1;
                assert!(!suspect.reason.trim().is_empty(), "{}", file.path);
                assert!(
                    suspect.reason.contains(&file.size.to_string()),
                    "{}: the reason does not give the size: {}",
                    file.path,
                    suspect.reason
                );
                assert!(!STANDARD_SIZES.contains(&file.size), "{}", file.path);
                assert_eq!(suspect.kind, SuspectKind::Container, "{}", file.path);
                let (offset, size) = (suspect.image_offset.unwrap(), suspect.image_size.unwrap());
                assert!(STANDARD_SIZES.contains(&size), "{}", file.path);
                assert_eq!(offset + size, file.size, "{}", file.path);
            }
        }
    }
    assert_eq!(suspects, 2);
}

/// Every working tree git reports for this repository (`git worktree list
/// --porcelain` run in the workspace: the main tree and every linked one),
/// canonicalised. An oracle independent of `corpus::location`.
#[expect(
    clippy::unwrap_used,
    reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
)]
fn git_working_trees() -> Vec<PathBuf> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["worktree", "list", "--porcelain"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(out.status.success(), "git worktree list: {out:?}");
    let trees: Vec<PathBuf> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .filter_map(|p| Path::new(p).canonicalize().ok())
        .collect();
    let here = repo_root().canonicalize().unwrap();
    assert!(
        trees.iter().any(|t| here.starts_with(t)),
        "git lists no working tree around the workspace {}: {trees:?}",
        here.display()
    );
    trees
}

rr_testkit::corpus_test! {
    /// US-RR-11-3 (D-025): the corpus directory these tests actually read,
    /// resolved exactly as the loader and `fixture_root(Tier::Corpus)` do
    /// (`RR_CORPUS_DIR` in this environment: the CI `corpus` job's, or a
    /// developer's), is outside every working tree of this repository and
    /// contains none of them. Checked against `git worktree list` rather than
    /// `corpus::location`, so a CI misconfiguration fails here loudly even if
    /// the guard itself regressed.
    fn the_corpus_dir_is_outside_the_repository() -> Result<(), CorpusError> {
        let from_env = rr_testkit::corpus::location::dir_from_env(|k| std::env::var_os(k))
            .ok_or(CorpusError::NoDir)?;
        let tier_root = rr_testkit::fixtures::fixture_root(rr_testkit::fixtures::Tier::Corpus)
            .unwrap();
        let corpus = Corpus::open()?;
        let dir = corpus.dir().canonicalize().unwrap();
        for other in [&from_env, &tier_root] {
            assert_eq!(other.canonicalize().unwrap(), dir, "{}", other.display());
        }
        for tree in git_working_trees() {
            assert!(
                !dir.starts_with(&tree),
                "{DIR_ENV}={} is inside the working tree {}: ROM images must live outside the repository (D-025)",
                dir.display(),
                tree.display()
            );
            assert!(
                !tree.starts_with(&dir),
                "{DIR_ENV}={} contains the working tree {}",
                dir.display(),
                tree.display()
            );
        }
        Ok(())
    }
}

rr_testkit::corpus_test! {
    /// Smoke test of the loader on the fetched corpus: it opens from
    /// `RR_CORPUS_DIR`, and every entry, clean or suspect, is there at its
    /// manifest size. (Hash verification is `cargo xtask fetch-corpus`.)
    fn the_fetched_corpus_opens() -> Result<(), CorpusError> {
        let corpus = Corpus::open()?;
        assert_eq!(corpus.manifest().len(), 501);
        for entry in corpus.manifest().entries() {
            let file = match entry {
                Entry::Clean(file) => file,
                Entry::Suspect { file, suspect } => {
                    assert!(!suspect.reason.is_empty());
                    file
                }
            };
            let meta = fs::symlink_metadata(corpus.path(file)).unwrap();
            assert!(meta.is_file(), "{}", file.path);
            assert_eq!(meta.len(), file.size, "{}", file.path);
        }
        Ok(())
    }
}

/// The SH-2 power-on reset vectors at the start of a 32-bit image look like
/// firmware: the initial PC is even and inside the image, the initial SP in
/// the on-chip RAM at the top of the address space.
fn sh_reset_vectors_plausible(image: &[u8]) -> bool {
    let word = |at: usize| {
        image
            .get(at..at + 4)
            .and_then(|w| <[u8; 4]>::try_from(w).ok())
            .map(u32::from_be_bytes)
    };
    match (word(0), word(4)) {
        (Some(pc), Some(sp)) => {
            u64::from(pc) < image.len() as u64 && pc % 2 == 0 && sp >= 0xFFF8_0000
        }
        _ => false,
    }
}

/// Where the file-name ROM ID first appears in `bytes`.
fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|w| w == needle)
}

rr_testkit::corpus_test! {
    /// US-RR-11-2 on the fetched bytes: each suspect entry reads back as
    /// `Image::Suspect`, and its manifest classification is what the bytes
    /// are. The file starts with an `INFO` chunk; an `MEMD` chunk header
    /// giving `image_size` sits just before `image_offset`; the embedded
    /// image ends the file, starts with SH-2 reset vectors and carries the
    /// file-name ROM ID where raw SH7058 images do (0x2000 / 0x2004, legacy's
    /// `internalidaddress`, `src/test/definitions/scalingbase_test.xml:61`).
    /// Legacy reads a file raw (`ECUEditor.java:663-676`) and matches the ID
    /// at that file offset (`RomID.java:71-86`), so it would not recognise
    /// these files at all: the ID is not there in the file as a whole.
    fn suspect_entries_match_the_fetched_bytes() -> Result<(), CorpusError> {
        let corpus = Corpus::open()?;
        let mut seen = 0;
        for (file, why) in corpus.manifest().suspect() {
            seen += 1;
            let Image::Suspect { bytes, suspect } = corpus.read(file)? else {
                panic!("{}: suspect entry read as raw", file.path);
            };
            assert_eq!(suspect, why);
            assert_eq!(suspect.kind, SuspectKind::Container, "{}", file.path);
            let offset = usize::try_from(suspect.image_offset.unwrap()).unwrap();
            let size = suspect.image_size.unwrap();
            assert_eq!(offset as u64 + size, bytes.len() as u64, "{}", file.path);
            assert_eq!(bytes.get(..4), Some(&b"INFO"[..]), "{}", file.path);
            let mut header = b"MEMD".to_vec();
            header.extend_from_slice(&u32::try_from(size).unwrap().to_be_bytes());
            assert_eq!(bytes.get(offset - 8..offset), Some(&header[..]), "{}", file.path);
            let image = suspect.embedded(&bytes).unwrap();
            assert_eq!(image.len() as u64, size);
            assert!(STANDARD_SIZES.contains(&size), "{}", file.path);
            assert!(sh_reset_vectors_plausible(image), "{}: reset vectors", file.path);
            assert!(!sh_reset_vectors_plausible(&bytes), "{}: the file itself is not an image", file.path);
            let id = file.filename_rom_id.as_deref().unwrap().as_bytes();
            let at = find(image, id);
            assert!(matches!(at, Some(0x2000 | 0x2004)), "{}: ROM ID at {at:?}", file.path);
            assert_ne!(find(&bytes, id), at, "{}", file.path);
        }
        assert_eq!(seen, 2);
        Ok(())
    }
}

rr_testkit::corpus_test! {
    /// US-RR-11-2, the other half: every clean entry reads back as
    /// `Image::Raw`, a standard size and not blank (one value throughout).
    /// (Every entry's classification is recomputed from the bytes by
    /// `cargo xtask gen-corpus-manifest --check`; fetch-corpus proves these
    /// are the same bytes by SHA-256.)
    fn clean_entries_are_raw_images_of_a_standard_size() -> Result<(), CorpusError> {
        let corpus = Corpus::open()?;
        let mut seen = 0;
        for file in corpus.manifest().clean() {
            seen += 1;
            let Image::Raw(bytes) = corpus.read(file)? else {
                panic!("{}: clean entry read as suspect", file.path);
            };
            assert!(STANDARD_SIZES.contains(&(bytes.len() as u64)), "{}", file.path);
            assert!(bytes.iter().any(|&b| b != bytes[0]), "{}: blank", file.path);
        }
        assert_eq!(seen, 499);
        Ok(())
    }
}
