//! `hooks/pre-commit` (story US-RR-13, task US-RR-13-6): the ROM-leak guard
//! as a git pre-commit hook, end to end, in throwaway repositories.
//!
//! Each test makes a fresh repository under `CARGO_TARGET_TMPDIR`, installs
//! the hook as CONTRIBUTING.md says (a copy at `hooks/pre-commit` and
//! `core.hooksPath hooks`, set in that repository's own config only), and
//! runs real `git commit`s with the global and system git config shut out,
//! so the developer's own hooks, signing and identity never come into it.
//! The hook runs the built `xtask` binary through `RR_XTASK`, with this
//! workspace's committed corpus manifest and coverage report, so the ROM IDs
//! are the real corpus's. The checks: a planted stock-ROM-shaped image and
//! an unscrubbed VIN trace under `fixtures/public` are blocked, a clean
//! commit (paths with spaces included) goes through, the index is checked
//! rather than the working tree, a deletion-only commit does not run the
//! guard, and a guard that cannot run blocks the commit.
//!
//! The CI side (story US-RR-13, task US-RR-13-1): a stock-ROM-shaped image
//! that the hook stops, then committed anyway with `git commit --no-verify`,
//! fails the `run:` scripts of ci.yml's `leak-guard` job. The scripts are
//! read from the committed ci.yml and run unchanged with `bash -eo pipefail`
//! (the shell GitHub runs a `run:` step in) in the throwaway checkout. Only
//! `cargo` is a stand-in: `cargo xtask leak-guard ...` runs the built xtask
//! with `--root <checkout>` added, because in CI xtask is built from the
//! checkout and so takes it as its workspace. The same checkout passes both
//! scripts before the ROM goes in, so the failure comes from the ROM and not
//! from the set-up. Task US-RR-13-2 does the same with traces holding a VIN
//! (a CSV log and a binary `.rrtrace`-style file), next to masked-VIN
//! traces that pass.
//!
//! The hook is POSIX sh; these tests run where a `sh` is on `PATH` and are
//! skipped elsewhere (on Windows the `sh` of Git for Windows may be absent
//! from `PATH`).

/// Off unix there is no `sh` to rely on: say so instead of passing silently.
#[cfg(not(unix))]
#[test]
fn hook_tests_skipped_without_posix_sh() {
    eprintln!("SKIP: hooks/pre-commit tests need a POSIX sh (unix only)");
}

#[cfg(unix)]
#[path = "common/workflow.rs"]
mod workflow;

#[cfg(unix)]
mod hook {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use xtask::corpus_manifest::{self, Manifest};

    use super::workflow;

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

    fn hook_source() -> PathBuf {
        workspace_root().join("hooks/pre-commit")
    }

    /// `sh` and `git` are both runnable.
    fn tools_present() -> bool {
        let runs = |cmd: &str, args: &[&str]| {
            Command::new(cmd)
                .args(args)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        let ok = runs("sh", &["-c", "true"]) && runs("git", &["--version"]);
        if !ok {
            eprintln!("SKIP: hooks/pre-commit tests need `sh` and `git` on PATH");
        }
        ok
    }

    /// The first ROM ID the committed manifest gives (a real corpus ID).
    #[expect(
        clippy::unwrap_used,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn real_rom_id() -> String {
        let path = workspace_root().join(corpus_manifest::COMMITTED_PATH);
        let m = Manifest::from_toml(&fs::read_to_string(path).unwrap()).unwrap();
        m.roms
            .files
            .iter()
            .find_map(|f| f.filename_rom_id.clone())
            .unwrap()
    }

    /// A binary image of `size` bytes in the shape of a stock ROM: erased
    /// flash (0xFF), a reset vector at 0, and `id` at `address`.
    fn rom_like(size: usize, id: &[u8], address: usize) -> Vec<u8> {
        let mut v = vec![0xFF; size];
        v[..4].copy_from_slice(&[0x00, 0x00, 0x0C, 0x00]);
        v[address..address + id.len()].copy_from_slice(id);
        v
    }

    const VIN_TRACE: &[u8] = b"t=0.000 ssm init\nt=0.120 vin=JF1GD29662G512345\n";
    const SCRUBBED_TRACE: &[u8] = b"t=0.000 ssm init\nt=0.120 vin=JF1GD29X*********\n";

    /// A throwaway repository with the hook installed.
    struct Repo {
        dir: PathBuf,
        /// Holds the empty global config and the hook's scratch trees.
        home: PathBuf,
    }

    /// The result of one `git commit`.
    struct Commit {
        ok: bool,
        out: String,
    }

    impl Repo {
        #[expect(
            clippy::unwrap_used,
            reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
        )]
        fn new(name: &str) -> Self {
            let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .join("leak-guard-hook")
                .join(name);
            let _ = fs::remove_dir_all(&base);
            let dir = base.join("repo");
            let home = base.join("home");
            fs::create_dir_all(dir.join("hooks")).unwrap();
            fs::create_dir_all(&home).unwrap();
            fs::write(home.join("gitconfig"), "").unwrap();
            // A copy keeps the source file's mode (executable).
            fs::copy(hook_source(), dir.join("hooks/pre-commit")).unwrap();
            let repo = Self {
                dir: dir.canonicalize().unwrap(),
                home: home.canonicalize().unwrap(),
            };
            repo.git(&["-c", "init.defaultBranch=main", "init", "-q"]);
            // This throwaway repository's own config only.
            repo.git(&["config", "user.name", "Leak Guard Test"]);
            repo.git(&["config", "user.email", "leak-guard@example.invalid"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo.git(&["config", "core.autocrlf", "false"]);
            repo.git(&["config", "core.hooksPath", "hooks"]);
            repo
        }

        /// `git <args>` in the repository, with the developer's global and
        /// system config shut out and `RR_XTASK` set to `xtask`.
        fn command(&self, args: &[&str], xtask: &str) -> Command {
            let mut cmd = Command::new("git");
            cmd.args(args)
                .current_dir(&self.dir)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", self.home.join("gitconfig"))
                .env("HOME", &self.home)
                .env("TMPDIR", &self.home)
                .env("RR_XTASK", xtask)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE");
            cmd
        }

        #[expect(
            clippy::unwrap_used,
            reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
        )]
        fn git(&self, args: &[&str]) -> String {
            let out = self
                .command(args, env!("CARGO_BIN_EXE_xtask"))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        }

        #[expect(
            clippy::unwrap_used,
            reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
        )]
        fn write(&self, rel: &str, bytes: &[u8]) {
            let p = self.dir.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, bytes).unwrap();
        }

        /// Writes `rel` and stages it.
        fn stage(&self, rel: &str, bytes: &[u8]) {
            self.write(rel, bytes);
            self.git(&["add", "--", rel]);
        }

        #[expect(
            clippy::unwrap_used,
            reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
        )]
        fn commit_with(&self, msg: &str, xtask: &str) -> Commit {
            let out = self
                .command(&["commit", "-q", "-m", msg], xtask)
                .output()
                .unwrap();
            Commit {
                ok: out.status.success(),
                out: format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                ),
            }
        }

        fn commit(&self, msg: &str) -> Commit {
            self.commit_with(msg, env!("CARGO_BIN_EXE_xtask"))
        }

        /// Commits on the current branch (0 on an unborn branch).
        fn commits(&self) -> usize {
            let out = self
                .command(&["rev-list", "--count", "HEAD"], "")
                .output()
                .ok();
            out.filter(|o| o.status.success())
                .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
                .unwrap_or(0)
        }

        /// Leftover hook scratch trees under the hook's TMPDIR.
        #[expect(
            clippy::unwrap_used,
            reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
        )]
        fn scratch_left(&self) -> Vec<String> {
            fs::read_dir(&self.home)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("rr-leak-guard."))
                .collect()
        }
    }

    /// Stands in for `cargo` in the CI checkout: `cargo xtask leak-guard
    /// ARGS` runs the built xtask (`RR_XTASK`) with the checkout (the
    /// current directory, as in the CI job) as `--root`, which is what xtask
    /// built from that checkout defaults to. Anything else is refused.
    const CARGO_SHIM: &str = r#"#!/bin/sh
if [ "${1:-}" != xtask ] || [ "${2:-}" != leak-guard ]; then
    echo "cargo stand-in: unexpected command: cargo $*" >&2
    exit 99
fi
shift 2
exec "$RR_XTASK" leak-guard --root "$(pwd)" "$@"
"#;

    /// The `run:` scripts of ci.yml's `leak-guard` job that run the guard,
    /// in order, as committed.
    #[expect(
        clippy::unwrap_used,
        reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
    )]
    fn ci_leak_guard_scripts() -> Vec<String> {
        let src = workflow::ci_yml();
        let wf = workflow::Workflow::parse(&src);
        let scripts: Vec<String> = wf
            .job("leak-guard")
            .unwrap()
            .run_scripts()
            .into_iter()
            .filter(|s| s.contains("cargo xtask leak-guard"))
            .collect();
        assert_eq!(scripts.len(), 2, "ci.yml leak-guard scripts: {scripts:?}");
        scripts
    }

    impl Repo {
        /// Runs a CI `run:` script in the repository as GitHub's bash does
        /// (`bash --noprofile --norc -eo pipefail`), with [`CARGO_SHIM`]
        /// first on `PATH`. Returns the exit code and all output.
        #[expect(
            clippy::unwrap_used,
            reason = "test-only helper outside a #[test] fn; panicking is how it fails the calling test"
        )]
        fn run_ci(&self, script: &str) -> (i32, String) {
            let bin = self.home.join("ci-bin");
            fs::create_dir_all(&bin).unwrap();
            let cargo = bin.join("cargo");
            fs::write(&cargo, CARGO_SHIM).unwrap();
            fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
            let path = std::env::var_os("PATH").unwrap_or_default();
            let path =
                std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path)))
                    .unwrap();
            let out = Command::new("bash")
                .args(["--noprofile", "--norc", "-eo", "pipefail", "-c", script])
                .current_dir(&self.dir)
                .env("PATH", path)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", self.home.join("gitconfig"))
                .env("HOME", &self.home)
                .env("RR_XTASK", env!("CARGO_BIN_EXE_xtask"))
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
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
    }

    fn assert_flags(out: &str, needles: &[&str]) {
        for n in needles {
            assert!(out.contains(n), "expected {n:?} in:\n{out}");
        }
    }

    /// The committed hook is a POSIX sh script with the executable bit set
    /// (git runs it directly).
    #[test]
    fn the_hook_is_an_executable_sh_script() {
        let src = fs::read_to_string(hook_source()).unwrap();
        assert!(src.starts_with("#!/bin/sh\n"), "{src}");
        assert!(!src.contains('\r'), "CRLF in hooks/pre-commit breaks sh");
        let mode = fs::metadata(hook_source()).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "hooks/pre-commit is not executable");
        if tools_present() {
            let out = Command::new("sh")
                .arg("-n")
                .arg(hook_source())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    /// Negative sample: scrubbed and small fixtures, paths with spaces, and
    /// a large binary outside fixtures/public (hash rule only) commit fine,
    /// as the first commit of an unborn branch and as a later one.
    #[test]
    fn a_clean_commit_passes() {
        if !tools_present() {
            return;
        }
        let repo = Repo::new("clean");
        repo.stage("fixtures/public/notes.toml", b"schema = 1\n");
        repo.stage("fixtures/public/traces/scrubbed trace.log", SCRUBBED_TRACE);
        repo.stage(
            "docs/big blob.bin",
            &rom_like(256 << 10, real_rom_id().as_bytes(), 0x2000),
        );
        let c = repo.commit("clean fixtures");
        assert!(c.ok, "{}", c.out);
        assert_flags(
            &c.out,
            &["leak-guard: ok (3 file(s) checked, 2 under fixtures/public)"],
        );
        assert_eq!(repo.commits(), 1);
        repo.stage("fixtures/public/notes.toml", b"schema = 1\n# more\n");
        let c = repo.commit("edit");
        assert!(c.ok, "{}", c.out);
        assert_flags(
            &c.out,
            &["leak-guard: ok (1 file(s) checked, 1 under fixtures/public)"],
        );
        assert_eq!(repo.commits(), 2);
        assert!(repo.scratch_left().is_empty(), "{:?}", repo.scratch_left());
    }

    /// Positive sample: a full-size stock-ROM-shaped image with a real corpus
    /// ROM ID, staged under fixtures/public next to a clean file, stops the
    /// commit; nothing is committed.
    #[test]
    fn a_staged_stock_rom_is_blocked() {
        if !tools_present() {
            return;
        }
        let repo = Repo::new("stock-rom");
        repo.stage("README", b"x\n");
        assert!(repo.commit("base").ok);
        let id = real_rom_id();
        repo.stage("fixtures/public/notes.toml", b"schema = 1\n");
        repo.stage(
            "fixtures/public/my stock rom.bin",
            &rom_like(512 << 10, id.as_bytes(), 0x2000),
        );
        let c = repo.commit("add a ROM");
        assert!(!c.ok, "commit went through:\n{}", c.out);
        assert_flags(
            &c.out,
            &[
                "leak-guard: fixtures/public/my stock rom.bin: 524288 bytes, over the 65536-byte limit",
                &format!("holds known ROM ID {id:?} at offset 0x2000"),
                "pre-commit: the ROM-leak guard rejected the staged files above; commit stopped.",
            ],
        );
        assert!(!c.out.contains("notes.toml:"), "{}", c.out);
        assert_eq!(repo.commits(), 1);
        assert!(repo.scratch_left().is_empty(), "{:?}", repo.scratch_left());
    }

    /// `bash` is runnable (the CI-script tests run the `run:` scripts with
    /// it, as GitHub does).
    fn bash_present() -> bool {
        let ok = Command::new("bash")
            .args(["-c", "true"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            eprintln!("SKIP: the CI-script tests need `bash` on PATH");
        }
        ok
    }

    impl Repo {
        /// Every one of ci.yml's leak-guard `scripts` passes in the checkout
        /// as it stands.
        fn ci_passes(&self, scripts: &[String]) {
            for s in scripts {
                let (code, out) = self.run_ci(s);
                assert_eq!(code, 0, "clean checkout failed `{s}`:\n{out}");
                assert_flags(&out, &["leak-guard: ok ("]);
            }
        }

        /// Commits what is staged the way the hook can be got past: the
        /// hook stops the commit, then `git commit --no-verify` makes it.
        fn commit_past_the_hook(&self, msg: &str) {
            let before = self.commits();
            let c = self.commit(msg);
            assert!(!c.ok, "the hook let `{msg}` through:\n{}", c.out);
            assert_eq!(self.commits(), before);
            self.git(&["commit", "-q", "--no-verify", "-m", msg]);
            assert_eq!(self.commits(), before + 1);
        }

        /// Every one of ci.yml's leak-guard `scripts` fails in the checkout
        /// as it stands, with the guard's findings (not an error or the
        /// cargo stand-in's refusal) and every one of `needles` in its
        /// output. Returns each script's output.
        fn ci_fails(&self, scripts: &[String], needles: &[&str]) -> Vec<String> {
            scripts
                .iter()
                .map(|s| {
                    let (code, out) = self.run_ci(s);
                    assert_ne!(code, 0, "`{s}` passed:\n{out}");
                    assert!(!out.contains("leak-guard: error:"), "{out}");
                    assert!(!out.contains("cargo stand-in"), "{out}");
                    assert_flags(&out, needles);
                    out
                })
                .collect()
        }
    }

    /// Positive sample, CI side: a stock-ROM-shaped image under
    /// fixtures/public that the hook stops, committed anyway with
    /// `--no-verify`, fails both of ci.yml's leak-guard scripts, run as
    /// committed in the checkout. Before it goes in, the same checkout
    /// passes both, so the failure is the ROM's.
    #[test]
    fn a_stock_rom_committed_past_the_hook_fails_ci() {
        if !tools_present() || !bash_present() {
            return;
        }
        let scripts = ci_leak_guard_scripts();
        let repo = Repo::new("ci-stock-rom");
        repo.stage("README", b"x\n");
        repo.stage("fixtures/public/notes.toml", b"schema = 1\n");
        assert!(repo.commit("base").ok);
        repo.ci_passes(&scripts);

        let id = real_rom_id();
        let rel = "fixtures/public/roms/stock rom.bin";
        repo.stage(rel, &rom_like(512 << 10, id.as_bytes(), 0x2000));
        repo.commit_past_the_hook("add a ROM, hook skipped");
        assert_eq!(repo.git(&["ls-files", "--", rel]), rel);

        repo.ci_fails(
            &scripts,
            &[
                &format!("leak-guard: {rel}: 524288 bytes, over the 65536-byte limit"),
                &format!("leak-guard: {rel}: holds known ROM ID {id:?} at offset 0x2000"),
            ],
        );
    }

    /// A binary `.rrtrace`-style trace: a header, then `vin` among
    /// non-alphanumeric frame bytes at offset 0xC.
    fn binary_trace(vin: &[u8]) -> Vec<u8> {
        let mut v = b"RRTRACE\0".to_vec();
        v.extend_from_slice(&[0x01, 0x00, 0x80, 0x10]);
        v.extend_from_slice(vin);
        v.extend_from_slice(&[0x00, 0x8C, 0xF0, 0xFF]);
        v
    }

    /// Positive sample, CI side (task US-RR-13-2): traces holding a
    /// 17-character VIN under fixtures/public, a text CSV log and a binary
    /// `.rrtrace`-style file, committed past the hook with `--no-verify`,
    /// fail both of ci.yml's leak-guard scripts, run as committed in the
    /// checkout; neither VIN reaches the CI log. Negative sample: the same
    /// traces with the VIN masked pass CI before, and stay unflagged after,
    /// so the failure is the VINs'.
    #[test]
    fn a_vin_trace_committed_past_the_hook_fails_ci() {
        if !tools_present() || !bash_present() {
            return;
        }
        let scripts = ci_leak_guard_scripts();
        let repo = Repo::new("ci-vin-trace");
        let scrubbed_csv = "fixtures/public/traces/scrubbed log.csv";
        let scrubbed_bin = "fixtures/public/traces/scrubbed.rrtrace";
        repo.stage("README", b"x\n");
        repo.stage(scrubbed_csv, b"time,rpm,vin\n0.000,800,JF1GD29X*********\n");
        repo.stage(scrubbed_bin, &binary_trace(b"JF2SH636*********"));
        assert!(repo.commit("base, scrubbed traces").ok);
        repo.ci_passes(&scripts);

        let csv = "fixtures/public/traces/drive log.csv";
        let bin = "fixtures/public/traces/drive.rrtrace";
        repo.stage(csv, b"time,rpm,vin\n0.000,800,JF1GD29662G512345\n");
        repo.stage(bin, &binary_trace(b"JF2SH63619H700001"));
        repo.commit_past_the_hook("add traces, hook skipped");
        assert_eq!(repo.git(&["ls-files", "--", csv]), csv);
        assert_eq!(repo.git(&["ls-files", "--", bin]), bin);

        for out in repo.ci_fails(
            &scripts,
            &[
                &format!("leak-guard: {csv}: unscrubbed VIN (JF1...) at offset 0x17"),
                &format!("leak-guard: {bin}: unscrubbed VIN (JF2...) at offset 0xC"),
                "2 finding(s)",
            ],
        ) {
            assert!(!out.contains("512345") && !out.contains("700001"), "{out}");
            assert!(!out.contains("scrubbed log.csv:"), "{out}");
            assert!(!out.contains("scrubbed.rrtrace:"), "{out}");
        }
    }

    /// Positive sample: a trace with an unscrubbed VIN under fixtures/public
    /// stops the first commit of a repository; the VIN is not printed.
    #[test]
    fn a_staged_vin_trace_is_blocked() {
        if !tools_present() {
            return;
        }
        let repo = Repo::new("vin");
        repo.stage("fixtures/public/traces/drive log.txt", VIN_TRACE);
        let c = repo.commit("add a trace");
        assert!(!c.ok, "commit went through:\n{}", c.out);
        assert_flags(
            &c.out,
            &[
                "leak-guard: fixtures/public/traces/drive log.txt: unscrubbed VIN (JF1...) at offset 0x1D",
            ],
        );
        assert!(!c.out.contains("512345"), "{}", c.out);
        assert_eq!(repo.commits(), 0);
    }

    /// The staged bytes are checked, not the working tree's: a VIN staged
    /// and then scrubbed only in the working tree is still blocked, and a
    /// scrubbed trace staged while the working tree holds the VIN commits
    /// (the committed blob being the scrubbed one).
    #[test]
    fn the_index_is_checked_not_the_working_tree() {
        if !tools_present() {
            return;
        }
        let repo = Repo::new("index");
        let rel = "fixtures/public/traces/drive.log";
        repo.stage(rel, VIN_TRACE);
        repo.write(rel, SCRUBBED_TRACE);
        let c = repo.commit("staged VIN, scrubbed working tree");
        assert!(!c.ok, "commit went through:\n{}", c.out);
        assert_flags(&c.out, &["drive.log: unscrubbed VIN (JF1...)"]);
        assert_eq!(repo.commits(), 0);

        repo.stage(rel, SCRUBBED_TRACE);
        repo.write(rel, VIN_TRACE);
        let c = repo.commit("scrubbed index, VIN in working tree");
        assert!(c.ok, "{}", c.out);
        assert_eq!(repo.commits(), 1);
        let committed = repo.git(&["show", &format!("HEAD:{rel}")]);
        assert!(!committed.contains("512345"), "{committed}");
    }

    /// A commit that only deletes runs no guard (here the guard could not
    /// even start, and the commit still goes through); one with a staged file
    /// and a guard that cannot run is stopped: the hook fails closed.
    #[test]
    fn deletions_skip_the_guard_and_a_broken_guard_blocks() {
        if !tools_present() {
            return;
        }
        let repo = Repo::new("closed");
        repo.stage("fixtures/public/notes.toml", b"schema = 1\n");
        assert!(repo.commit("base").ok);
        let missing = repo.home.join("no-such-xtask");
        let missing = missing.to_str().unwrap();

        repo.stage("fixtures/public/more.toml", b"schema = 1\n");
        let c = repo.commit_with("guard missing", missing);
        assert!(!c.ok, "commit went through:\n{}", c.out);
        assert_flags(
            &c.out,
            &["pre-commit: the ROM-leak guard could not run (exit "],
        );
        assert_eq!(repo.commits(), 1);

        repo.git(&["reset", "-q"]);
        repo.git(&["rm", "-q", "--", "fixtures/public/notes.toml"]);
        let c = repo.commit_with("delete only", missing);
        assert!(c.ok, "{}", c.out);
        assert_eq!(repo.commits(), 2);
        assert!(repo.scratch_left().is_empty(), "{:?}", repo.scratch_left());
    }
}
