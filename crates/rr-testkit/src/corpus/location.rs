//! Where the external stock-ROM corpus lives, and the guarantee that it is
//! outside the repository working tree (US-RR-11-9, D-025: no ROM bytes are
//! ever written inside, or read from, the repo).
//!
//! One implementation, two users: rr-testkit's corpus loader (`corpus.rs`)
//! and `cargo xtask fetch-corpus`. xtask may not link rr-testkit (it is
//! dev-dependency only, `cargo xtask check-deps` rule 3) and rr-testkit
//! should not link xtask, so xtask compiles this same file as its own module
//! (`#[path]` in `xtask/src/lib.rs`). It therefore uses `std` only, links to
//! nothing outside itself, and its unit tests run in both crates.
//!
//! # The directory
//!
//! [`DIR_ENV`] when set and non-empty, else [`default_dir`]:
//! `%LOCALAPPDATA%\romraider-rs\corpus` on Windows, otherwise
//! `$XDG_CACHE_HOME/romraider-rs/corpus` (an absolute `XDG_CACHE_HOME` only),
//! falling back to `~/.cache/romraider-rs/corpus`. fetch-corpus's `--dir`
//! overrides both.
//!
//! # What is protected
//!
//! [`protected_roots`] lists, from a starting directory (the workspace root):
//!
//! * the starting directory itself (the workspace, from `CARGO_MANIFEST_DIR`);
//! * the git working tree it is in: the nearest ancestor holding a `.git`
//!   directory or file (what `git rev-parse --show-toplevel` finds, read from
//!   the file system so no `git` process is needed and `GIT_DIR` cannot
//!   redirect it);
//! * every other working tree of the same repository: the main one (the
//!   parent of the common `.git` directory) and each linked worktree recorded
//!   in `<common>/worktrees/*/gitdir` whose `.git` file still exists. ROM
//!   bytes written into a sibling worktree are one `git add` away from a
//!   commit, just as in this one.
//!
//! A *different* git repository is not protected: the corpus directory is
//! the user's choice, a home directory kept in git (dotfiles) would otherwise
//! refuse `~/.cache`, and fetch-corpus never replaces a directory it did not
//! create. Only this repository's own working trees are refused.
//!
//! # How paths are compared
//!
//! Both sides go through [`resolve`]: made absolute against the current
//! directory, then canonicalised through the nearest ancestor that exists, so
//! symlinks, `.` and `..` cannot disguise a path, and a directory that does
//! not exist yet is judged by where it would be created. On Windows and
//! macOS, whose default file systems are case-insensitive, components are
//! also compared case-insensitively (canonicalisation already returns the
//! on-disk case for the parts that exist; this covers the rest). A directory
//! is refused when it is a protected root, is inside one, or contains one.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Environment variable naming the corpus directory.
pub const DIR_ENV: &str = "RR_CORPUS_DIR";

/// The default corpus directory below the per-user cache root.
pub const CACHE_SUBDIR: &[&str] = &["romraider-rs", "corpus"];

/// The variables [`default_dir`] reads on this platform, for messages.
pub const DEFAULT_DIR_VARS: &str = if cfg!(windows) {
    "LOCALAPPDATA"
} else {
    "XDG_CACHE_HOME or HOME"
};

/// The default corpus directory (module docs), from the environment `env`;
/// `None` when none of the variables it needs is set.
pub fn default_dir(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut dir = if cfg!(windows) {
        set("LOCALAPPDATA")
    } else {
        set("XDG_CACHE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| set("HOME").map(|h| h.join(".cache")))
    }?;
    dir.extend(CACHE_SUBDIR);
    Some(dir)
}

/// The corpus directory from the environment `env`: [`DIR_ENV`] when set and
/// non-empty, else [`default_dir`].
pub fn dir_from_env(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    match env(DIR_ENV) {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => default_dir(env),
    }
}

/// Whether path components compare case-insensitively on this platform.
pub const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// Why a corpus directory was refused.
#[derive(Debug)]
pub enum RepoGuardError {
    /// The directory is a protected working tree, or inside one.
    Inside {
        /// The corpus directory, resolved.
        dir: PathBuf,
        /// The working tree it is in, resolved.
        root: PathBuf,
    },
    /// The directory contains a protected working tree.
    Contains {
        /// The corpus directory, resolved.
        dir: PathBuf,
        /// The working tree inside it, resolved.
        root: PathBuf,
    },
    /// A relative path could not be made absolute.
    CurrentDir(io::Error),
}

impl fmt::Display for RepoGuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inside { dir, root } => write!(
                f,
                "corpus directory {} is inside the repository working tree {}: ROM images must never be written into or read from the working tree (D-025); set RR_CORPUS_DIR (or fetch-corpus --dir) to a directory outside it, such as the default ~/.cache/romraider-rs/corpus",
                dir.display(),
                root.display()
            ),
            Self::Contains { dir, root } => write!(
                f,
                "corpus directory {} contains the repository working tree {}; set RR_CORPUS_DIR (or fetch-corpus --dir) to a dedicated directory outside it",
                dir.display(),
                root.display()
            ),
            Self::CurrentDir(e) => write!(f, "cannot read the current directory: {e}"),
        }
    }
}

impl std::error::Error for RepoGuardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDir(e) => Some(e),
            Self::Inside { .. } | Self::Contains { .. } => None,
        }
    }
}

/// `path` made absolute and resolved through every ancestor that exists, so
/// that two spellings of one place (symlinks, `.`, `..`) compare equal. The
/// part below the deepest existing ancestor is applied lexically (it cannot
/// contain a symlink, since it does not exist).
///
/// # Errors
///
/// `path` is relative and the current directory cannot be read.
pub fn resolve(path: &Path) -> Result<PathBuf, RepoGuardError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(RepoGuardError::CurrentDir)?
            .join(path)
    };
    let mut rest = Vec::new();
    let mut base = abs.as_path();
    loop {
        if let Ok(real) = base.canonicalize() {
            let mut out = real;
            for c in rest.iter().rev() {
                match c {
                    Component::ParentDir => {
                        out.pop();
                    }
                    Component::CurDir => {}
                    other => out.push(other),
                }
            }
            return Ok(out);
        }
        let Some(parent) = base.parent() else {
            return Ok(abs);
        };
        if let Some(c) = base.components().next_back() {
            rest.push(c);
        }
        base = parent;
    }
}

/// The working trees a corpus directory must stay out of, starting from
/// `start` (module docs), each [`resolve`]d; `start` first.
///
/// # Errors
///
/// `start` is relative and the current directory cannot be read.
pub fn protected_roots(start: &Path) -> Result<Vec<PathBuf>, RepoGuardError> {
    let start = resolve(start)?;
    let mut roots = vec![start.clone()];
    let Some(top) = start
        .ancestors()
        .find(|a| fs::symlink_metadata(a.join(".git")).is_ok())
    else {
        return Ok(roots);
    };
    push_unique(&mut roots, top.to_path_buf());
    let Some(common) = common_git_dir(top) else {
        return Ok(roots);
    };
    if common.file_name().is_some_and(|n| n == ".git")
        && let Some(main) = common.parent()
    {
        push_unique(&mut roots, main.to_path_buf());
    }
    if let Ok(entries) = fs::read_dir(common.join("worktrees")) {
        let mut linked = Vec::new();
        for entry in entries.flatten() {
            let Ok(text) = fs::read_to_string(entry.path().join("gitdir")) else {
                continue;
            };
            let dotgit = entry.path().join(text.trim_end_matches(['\r', '\n']));
            if fs::symlink_metadata(&dotgit).is_ok()
                && let Some(tree) = dotgit.parent()
            {
                linked.push(resolve(tree)?);
            }
        }
        linked.sort();
        for tree in linked {
            push_unique(&mut roots, tree);
        }
    }
    Ok(roots)
}

/// The repository's common git directory for the working tree at `top`: its
/// `.git` directory, or for a `.git` file (`gitdir: <path>`, a linked
/// worktree or submodule) that directory's `commondir`, if it has one.
fn common_git_dir(top: &Path) -> Option<PathBuf> {
    let dotgit = top.join(".git");
    let git_dir = if fs::metadata(&dotgit).ok()?.is_dir() {
        dotgit
    } else {
        let text = fs::read_to_string(&dotgit).ok()?;
        let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
        top.join(target)
    };
    let common = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(rel) => git_dir.join(rel.trim_end_matches(['\r', '\n'])),
        Err(_) => git_dir,
    };
    resolve(&common).ok()
}

fn push_unique(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.iter().any(|r| same_path(r, &root)) {
        roots.push(root);
    }
}

/// Fails unless `dir` is outside every root in `roots` and contains none of
/// them. `roots` must already be [`resolve`]d (as [`protected_roots`] returns
/// them).
///
/// # Errors
///
/// [`RepoGuardError::Inside`], [`RepoGuardError::Contains`], or
/// [`RepoGuardError::CurrentDir`] for a relative `dir`.
pub fn check_outside(dir: &Path, roots: &[PathBuf]) -> Result<(), RepoGuardError> {
    let dir = resolve(dir)?;
    for root in roots {
        if path_starts_with(&dir, root, CASE_INSENSITIVE) {
            return Err(RepoGuardError::Inside {
                dir,
                root: root.clone(),
            });
        }
    }
    for root in roots {
        if path_starts_with(root, &dir, CASE_INSENSITIVE) {
            return Err(RepoGuardError::Contains {
                dir,
                root: root.clone(),
            });
        }
    }
    Ok(())
}

/// [`check_outside`] against [`protected_roots`]`(start)`.
///
/// # Errors
///
/// As [`check_outside`].
pub fn check_outside_repo(dir: &Path, start: &Path) -> Result<(), RepoGuardError> {
    check_outside(dir, &protected_roots(start)?)
}

fn same_path(a: &Path, b: &Path) -> bool {
    path_starts_with(a, b, CASE_INSENSITIVE) && path_starts_with(b, a, CASE_INSENSITIVE)
}

/// Whether `path` is `base` or below it, component by component, optionally
/// ignoring case.
#[must_use]
pub fn path_starts_with(path: &Path, base: &Path, ignore_case: bool) -> bool {
    let mut p = path.components();
    base.components().all(|b| {
        p.next().is_some_and(|c| {
            if ignore_case {
                c.as_os_str().to_string_lossy().to_lowercase()
                    == b.as_os_str().to_string_lossy().to_lowercase()
            } else {
                c == b
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh scratch directory under the system temp dir (never in the repo).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rr-outside-repo-{}-{name}-{}",
            env!("CARGO_PKG_NAME"),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn default_dir_follows_xdg_then_home() {
        let tail = Path::new("romraider-rs").join("corpus");
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", "/x/cache"), ("HOME", "/h")])),
            Some(Path::new("/x/cache").join(&tail))
        );
        // A relative XDG_CACHE_HOME is ignored, as the spec says.
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", "rel"), ("HOME", "/h")])),
            Some(Path::new("/h/.cache").join(&tail))
        );
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", ""), ("HOME", "/h")])),
            Some(Path::new("/h/.cache").join(&tail))
        );
        assert_eq!(default_dir(env(&[])), None);
    }

    #[test]
    #[cfg(windows)]
    fn default_dir_is_under_local_app_data() {
        assert_eq!(
            default_dir(env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")])),
            Some(PathBuf::from(
                r"C:\Users\u\AppData\Local\romraider-rs\corpus"
            ))
        );
        assert_eq!(default_dir(env(&[("HOME", "/h")])), None);
    }

    #[test]
    fn the_variable_wins_over_the_default_unless_empty() {
        let home = if cfg!(windows) {
            "LOCALAPPDATA"
        } else {
            "HOME"
        };
        let default = default_dir(env(&[(home, "/h")]));
        assert!(default.is_some());
        assert_eq!(
            dir_from_env(env(&[(DIR_ENV, "/c"), (home, "/h")])),
            Some(PathBuf::from("/c"))
        );
        assert_eq!(dir_from_env(env(&[(DIR_ENV, ""), (home, "/h")])), default);
        assert_eq!(dir_from_env(env(&[(home, "/h")])), default);
        assert_eq!(dir_from_env(env(&[])), None);
    }

    fn inside(r: Result<(), RepoGuardError>) -> bool {
        matches!(r, Err(RepoGuardError::Inside { .. }))
    }

    fn contains(r: Result<(), RepoGuardError>) -> bool {
        matches!(r, Err(RepoGuardError::Contains { .. }))
    }

    #[test]
    fn inside_equal_containing_and_outside() {
        let base = scratch("basic");
        let repo = base.join("repo");
        fs::create_dir_all(repo.join("target")).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();

        assert!(inside(check_outside_repo(&repo, &repo)), "the repo root");
        assert!(inside(check_outside_repo(
            &repo.join("target/corpus"),
            &repo
        )));
        assert!(
            inside(check_outside_repo(&repo.join("not/yet/there"), &repo)),
            "a path that does not exist yet"
        );
        assert!(
            inside(check_outside_repo(&base.join("elsewhere/../repo/x"), &repo)),
            "`..` back into the repo"
        );
        assert!(inside(check_outside_repo(
            &repo.join("./target/./c"),
            &repo
        )));
        assert!(
            inside(check_outside_repo(
                &repo.join("missing/../../repo/target/c"),
                &repo
            )),
            "`..` through a missing directory"
        );
        assert!(contains(check_outside_repo(&base, &repo)));
        assert!(contains(check_outside_repo(&repo.join(".."), &repo)));

        assert!(check_outside_repo(&base.join("corpus"), &repo).is_ok());
        assert!(
            check_outside_repo(&base.join("repo-corpus"), &repo).is_ok(),
            "a sibling sharing the name prefix"
        );
        assert!(
            check_outside_repo(&repo.join("../outside"), &repo).is_ok(),
            "`..` out of the repo"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn errors_name_both_paths_and_the_fix() {
        let base = scratch("message");
        let repo = base.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let msg = check_outside_repo(&repo.join("c"), &repo)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("inside the repository working tree"), "{msg}");
        assert!(msg.contains("RR_CORPUS_DIR"), "{msg}");
        assert!(msg.contains("D-025"), "{msg}");
        let msg = check_outside_repo(&base, &repo).unwrap_err().to_string();
        assert!(
            msg.contains("contains the repository working tree"),
            "{msg}"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_relative_path_is_resolved_against_the_current_directory() {
        // Tests run in the crate's own directory, inside this repository;
        // nothing is created.
        let here = std::env::current_dir().unwrap();
        assert!(inside(check_outside_repo(
            Path::new("target/rr-never-created"),
            &here
        )));
        assert!(inside(check_outside_repo(Path::new("."), &here)));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_the_repo_is_refused() {
        use std::os::unix::fs::symlink;
        let base = scratch("symlink");
        let repo = base.join("repo");
        fs::create_dir_all(repo.join("target")).unwrap();
        let outside = base.join("out");
        fs::create_dir_all(&outside).unwrap();
        // A link to the repo, and a link to a directory inside it.
        symlink(&repo, outside.join("to-repo")).unwrap();
        symlink(repo.join("target"), outside.join("to-target")).unwrap();
        assert!(inside(check_outside_repo(&outside.join("to-repo"), &repo)));
        assert!(inside(check_outside_repo(
            &outside.join("to-target/corpus"),
            &repo
        )));
        // `..` after a symlink is physical, as the OS resolves it: back out of
        // target lands in the repo.
        assert!(inside(check_outside_repo(
            &outside.join("to-target/../corpus"),
            &repo
        )));
        // A link to the repo's parent contains it.
        symlink(&base, outside.join("to-base")).unwrap();
        assert!(contains(check_outside_repo(
            &outside.join("to-base"),
            &repo
        )));
        // The repo reached through a symlinked start is still the repo.
        assert!(inside(check_outside_repo(
            &repo.join("target/c"),
            &outside.join("to-repo")
        )));
        assert!(check_outside_repo(&outside.join("corpus"), &repo).is_ok());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn case_folding_compares_components() {
        let a = Path::new("/Users/Me/Repo/RomRaider/target");
        let b = Path::new("/users/me/repo/romraider");
        assert!(path_starts_with(a, b, true));
        assert!(!path_starts_with(a, b, false));
        assert!(!path_starts_with(
            Path::new("/users/me/repo/romraider-corpus"),
            b,
            true
        ));
        assert!(!path_starts_with(Path::new("/users/me"), b, true));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_differently_cased_path_into_the_repo_is_refused() {
        let base = scratch("case");
        let repo = base.join("Repo");
        fs::create_dir_all(&repo).unwrap();
        let shouted = base.join("REPO").join("NEW").join("corpus");
        assert!(inside(check_outside_repo(&shouted, &repo)));
        fs::remove_dir_all(&base).unwrap();
    }

    /// The start's enclosing git working tree, and every worktree of the same
    /// repository, are protected; a different repository is not. The layout
    /// is git's own (gitrepository-layout(5)): `.git` directory in the main
    /// tree, `.git` file plus `worktrees/<name>/{gitdir,commondir}` for a
    /// linked one.
    #[test]
    fn worktrees_of_the_same_repository_are_protected() {
        let base = scratch("worktrees");
        let main = base.join("main");
        let linked = base.join("linked");
        let stale = base.join("pruned");
        let common = main.join(".git");
        let meta = common.join("worktrees").join("linked");
        fs::create_dir_all(main.join("crates/rr-testkit")).unwrap();
        fs::create_dir_all(&meta).unwrap();
        fs::create_dir_all(linked.join("crates/rr-testkit")).unwrap();
        fs::write(linked.join(".git"), format!("gitdir: {}\n", meta.display())).unwrap();
        fs::write(
            meta.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        fs::write(meta.join("commondir"), "../..\n").unwrap();
        // A worktree that was deleted without `git worktree prune`.
        let gone = common.join("worktrees").join("pruned");
        fs::create_dir_all(&gone).unwrap();
        fs::write(
            gone.join("gitdir"),
            format!("{}\n", stale.join(".git").display()),
        )
        .unwrap();
        let other = base.join("other-repo");
        fs::create_dir_all(other.join(".git")).unwrap();

        for start in [
            main.join("crates/rr-testkit"),
            linked.join("crates/rr-testkit"),
        ] {
            let roots = protected_roots(&start).unwrap();
            let want = [
                resolve(&start).unwrap(),
                resolve(&main).unwrap(),
                resolve(&linked).unwrap(),
            ];
            for w in &want {
                assert!(roots.contains(w), "{roots:?} lacks {w:?}");
            }
            assert_eq!(roots.len(), 3, "{roots:?}");
            assert!(inside(check_outside(&main.join("target/corpus"), &roots)));
            assert!(inside(check_outside(&linked.join("target/corpus"), &roots)));
            assert!(contains(check_outside(&base, &roots)));
            assert!(check_outside(&stale.join("corpus"), &roots).is_ok());
            assert!(check_outside(&other.join("corpus"), &roots).is_ok());
        }

        // No git at all: only the start itself.
        let plain = base.join("plain");
        fs::create_dir_all(&plain).unwrap();
        // (`base` is under the system temp dir, which is not in a git tree.)
        let roots = protected_roots(&plain).unwrap();
        assert_eq!(roots.first(), Some(&resolve(&plain).unwrap()));
        fs::remove_dir_all(&base).unwrap();
    }

    /// The same, with a real `git worktree add`, so the hand-built layout
    /// above cannot drift from what git writes.
    #[test]
    fn a_real_git_worktree_is_protected() {
        let base = scratch("real-git");
        let main = base.join("main");
        fs::create_dir_all(&main).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(dir)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "init.defaultBranch=main",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(&main, &["init", "--quiet"]);
        git(&main, &["commit", "--quiet", "--allow-empty", "-m", "x"]);
        git(&main, &["worktree", "add", "--quiet", "-b", "wt", "../wt"]);
        let wt = base.join("wt");
        let from_main = protected_roots(&main).unwrap();
        let from_wt = protected_roots(&wt.join("sub-not-there")).unwrap();
        for roots in [&from_main, &from_wt] {
            assert!(
                inside(check_outside(&wt.join("corpus"), roots)),
                "{roots:?}"
            );
            assert!(
                inside(check_outside(&main.join("corpus"), roots)),
                "{roots:?}"
            );
            assert!(check_outside(&base.join("corpus"), roots).is_ok());
        }
        fs::remove_dir_all(&base).unwrap();
    }
}
