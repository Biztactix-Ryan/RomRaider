//! `release.yml` + `dist-workspace.toml` wiring for story US-RR-38, task
//! US-RR-38-5 (INFRASTRUCTURE.md "Linux Release", D-022): a cargo-dist build
//! of the `romraider-cli` tarball, published with one SHA256SUMS as the
//! moving `nightly` GitHub pre-release every night, and as a release on a
//! `v*` tag.
//!
//! [`release_problems`] is a pure check over the text of the two files; one
//! test runs it on the committed files and `release_config_rejects_*` proves
//! each assertion bites on a mutated copy. It asserts that:
//!
//! * release.yml runs on a `schedule`, on pushed `v*` tags and on
//!   `workflow_dispatch`;
//! * it installs exactly the cargo-dist version dist-workspace.toml pins
//!   (`DIST_VERSION` = `cargo-dist-version`), checks the download against a
//!   SHA-256 (`DIST_SHA256`, 64 hex digits), and dist-workspace.toml has
//!   `allow-dirty = ["ci"]` (release.yml is hand-written; without it
//!   `dist` would want to regenerate it);
//! * dist ships rr-cli (`[package.metadata.dist] dist = true`) for
//!   `x86_64-unknown-linux-gnu` as a `.tar.gz`, and its binary is
//!   `romraider-cli`;
//! * job `build-cli` runs `dist build`, publishes the archive as
//!   `romraider-cli-<target>.tar.gz` and runs `romraider-cli --version` from
//!   it before upload;
//! * job `publish` writes and verifies `SHA256SUMS`, creates the release with
//!   `gh release create`, and the nightly channel is always a pre-release.
//!
//! [`appimage_problems`] (task US-RR-38-6) checks the AppImage the same way,
//! over release.yml and the recipe in `packaging/appimage/`: appimagetool and
//! the type2 runtime are pinned by version and SHA-256 and checked on
//! download; job `build-appimage` needs `build-cli`, builds
//! `romraider-cli-x86_64.AppImage` from the tarball's binary with
//! `packaging/appimage/build.sh` (`--runtime-file`, so nothing unpinned is
//! fetched), runs its `--version` and uploads it as `release-appimage-*`, and
//! `publish` needs it; the AppDir skeleton has an AppRun that execs
//! `usr/bin/romraider-cli`, a desktop entry and the icon it names.
//!
//! [`appimage_container_problems`] (task US-RR-38-3) checks that the AppImage
//! is run on a clean Ubuntu 22.04 before release: job `test-appimage` needs
//! `build-appimage`, downloads the `release-appimage-x86_64` artifact it
//! uploaded, and runs `romraider-cli --version` from it with `docker run
//! --rm` in the `ubuntu:22.04` image pinned by digest (`UBUNTU_22_04_IMAGE`),
//! offline, unprivileged, with only the AppImage mounted read-only and
//! nothing installed (`--appimage-extract-and-run`: no FUSE); the output must
//! be `romraider-cli <planned version>`; nothing in the job can be skipped,
//! and `publish` needs it.
//!
//! [`deb_problems`] (task US-RR-38-7) checks the .deb over release.yml,
//! `[package.metadata.deb]` in crates/rr-cli/Cargo.toml and
//! `packaging/deb/build.sh`: cargo-deb is installed as exactly
//! `CARGO_DEB_VERSION` with `--locked`; job `build-deb` needs `build-cli`,
//! packages the tarball's binary with build.sh (`--no-build --no-strip`, so
//! the bytes are the tarball's) as `romraider-cli_<version>_amd64.deb`,
//! installs it with `dpkg -i`, runs the installed `romraider-cli --version`,
//! and uploads it as `release-deb-*`; `publish` needs it; the package holds
//! exactly the binary in `usr/bin/`, the AppImage's desktop entry in
//! `usr/share/applications/` and its icon in the hicolor theme.
//!
//! [`deb_container_problems`] (task US-RR-38-2) checks that the .deb is
//! installed and run on clean Ubuntus before release: job `test-deb` needs
//! `build-deb`, downloads the `release-deb-amd64` artifact it uploaded, and
//! for each of ubuntu:22.04 and ubuntu:24.04 (matrix; both images pinned by
//! digest, `UBUNTU_22_04_IMAGE` / `UBUNTU_24_04_IMAGE`) installs exactly that
//! one package with `dpkg -i` in a fresh `docker run --rm` container,
//! offline, with only the package mounted read-only and nothing else
//! installed or forced (so its Depends must be met by the base image), checks
//! the container is that Ubuntu and the binary is /usr/bin/romraider-cli, and
//! runs `romraider-cli --version`; the output must be `romraider-cli
//! <planned version>`; nothing in the job can be skipped, and `publish`
//! needs it.
//!
//! [`content_check_problems`] (task US-RR-38-8) checks that `publish` runs
//! the release artifact content check (`cargo xtask
//! check-release-artifacts`, tested on planted ROMs in
//! `release_artifacts.rs`) on the downloaded `assets/` directory: after the
//! checkout that provides xtask, the download and SHA256SUMS; before any
//! step that publishes; unconditionally (no `if:`, no `continue-on-error`,
//! no `|| true`); and that the release uploads only `assets/`. US-RR-38-4
//! adds that what is checked is what is published: nothing after the check
//! can add to or change `assets/` (no action, no `working-directory:
//! assets`, no script touching it beyond `ls` and `gh release create ...
//! assets/*`), and every `release-*` artifact lands in it before the check
//! (one download, `pattern: release-*`, `merge-multiple: true`, after every
//! job that uploads one).
//!
//! That release.yml cannot build or publish without the ci.yml gate is
//! `t6_ci_config.rs` (`t6_release_needs_the_ci_gate`).
//!
//! [`nightly_publish::nightly_publish_problems`] (task US-RR-38-1) runs the
//! nightly's publish path: the `plan` step's script and `publish`'s
//! `SHA256SUMS` and `Publish GitHub Release` scripts, exactly as release.yml
//! has them, under bash with stub `gh` and `dist` on PATH that record every
//! call (no network, nothing reaches GitHub), on a scheduled and a manual
//! run. The contract: the release is `nightly`, created with
//! `--prerelease` at the built commit after the previous nightly and its tag
//! are gone, and it attaches exactly the tarball, the AppImage, the .deb (the
//! only `release-*` artifacts) and a SHA256SUMS whose entries are exactly
//! those three with their true digests; a scheduled run at a commit that
//! already is `nightly` publishes nothing. Whether GitHub then shows that
//! pre-release can only be seen on a real run.

#[path = "common/workflow.rs"]
mod workflow;

use std::fs;
use std::path::Path;

use workflow::{Workflow, assert_flags, key_at_any, skippable_problems, workflow_yml};

const TARGET: &str = "x86_64-unknown-linux-gnu";

fn repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
    fs::read_to_string(&path)
        .map(|s| s.replace("\r\n", "\n"))
        .unwrap_or_else(|e| format!("<unreadable {}: {e}>", path.display()))
}

/// The value of `key: value` in the workflow's top-level `env:`, unquoted.
fn top_env(wf: &Workflow<'_>, key: &str) -> Option<String> {
    wf.section("env").iter().find_map(|l| {
        let (k, v) = l.trim().split_once(':')?;
        (workflow::indent(l) == 2 && k == key)
            .then(|| v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
    })
}

/// Every way release.yml / dist-workspace.toml / rr-cli's manifest fail the
/// US-RR-38-5 wiring; empty when it holds.
fn release_problems(release: &str, dist_toml: &str, cli_toml: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);

    // Triggers.
    let on = wf.on_block();
    if !on.iter().any(|l| l.trim().starts_with("- cron:")) {
        problems.push("release.yml has no nightly `schedule` cron".to_owned());
    }
    if key_at_any(on, "tags").is_none() || !on.iter().any(|l| l.trim().starts_with("- \"v")) {
        problems.push("release.yml does not run on pushed `v*` tags".to_owned());
    }
    if key_at_any(on, "workflow_dispatch").is_none() {
        problems.push("release.yml has no `workflow_dispatch`".to_owned());
    }

    // cargo-dist pin.
    let dist: toml::Table = match toml::from_str(dist_toml) {
        Ok(t) => t,
        Err(e) => return vec![format!("dist-workspace.toml does not parse: {e}")],
    };
    let cfg = dist.get("dist").and_then(|d| d.as_table());
    let get = |k: &str| cfg.and_then(|c| c.get(k));
    let pinned = get("cargo-dist-version").and_then(|v| v.as_str());
    let installed = top_env(&wf, "DIST_VERSION");
    if pinned.is_none() || installed.as_deref() != pinned {
        problems.push(format!(
            "release.yml DIST_VERSION {installed:?} != dist-workspace.toml cargo-dist-version {pinned:?}"
        ));
    }
    let sha_ok = top_env(&wf, "DIST_SHA256")
        .is_some_and(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()));
    if !sha_ok {
        problems.push("release.yml DIST_SHA256 is not a SHA-256 hex digest".to_owned());
    }
    let verifies = wf.jobs.iter().all(|j| {
        let scripts = j.run_scripts();
        let installs = scripts
            .iter()
            .any(|s| s.contains("cargo-dist-x86_64-unknown-linux-gnu.tar.xz"));
        !installs
            || scripts
                .iter()
                .any(|s| s.contains("${DIST_SHA256}") && s.contains("sha256sum -c"))
    });
    if !verifies {
        problems.push("a release.yml job installs dist without checking DIST_SHA256".to_owned());
    }
    let dirty_ci = get("allow-dirty")
        .and_then(|v| v.as_array())
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("ci")));
    if !dirty_ci {
        problems.push(
            "dist-workspace.toml lacks allow-dirty = [\"ci\"] (release.yml is hand-written)"
                .to_owned(),
        );
    }
    let targets_ok = get("targets")
        .and_then(|v| v.as_array())
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(TARGET)));
    if !targets_ok {
        problems.push(format!("dist-workspace.toml targets lack {TARGET}"));
    }
    if get("unix-archive").and_then(|v| v.as_str()) != Some(".tar.gz") {
        problems.push("dist-workspace.toml unix-archive is not .tar.gz".to_owned());
    }

    // What ships: rr-cli, binary romraider-cli.
    match toml::from_str::<toml::Table>(cli_toml) {
        Err(e) => problems.push(format!("crates/rr-cli/Cargo.toml does not parse: {e}")),
        Ok(cli) => {
            let shipped = cli
                .get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("dist"))
                .and_then(|d| d.get("dist"))
                .and_then(|v| v.as_bool());
            if shipped != Some(true) {
                problems.push("rr-cli lacks [package.metadata.dist] dist = true".to_owned());
            }
            let bin = cli
                .get("bin")
                .and_then(|b| b.as_array())
                .and_then(|b| b.first())
                .and_then(|b| b.get("name"))
                .and_then(|n| n.as_str());
            if bin != Some("romraider-cli") {
                problems.push(format!("rr-cli's binary is {bin:?}, not romraider-cli"));
            }
        }
    }

    // build-cli: dist build, renamed asset, smoke test.
    match wf.job("build-cli") {
        None => problems.push("release.yml has no `build-cli` job".to_owned()),
        Some(job) => {
            let scripts = job.run_scripts().join("\n");
            if !scripts.contains("dist build ") || !scripts.contains("--artifacts=local") {
                problems.push("`build-cli` does not run `dist build --artifacts=local`".to_owned());
            }
            let staged = job
                .steps()
                .iter()
                .find(|st| st.id == "Stage release asset")
                .is_some_and(|st| {
                    st.run_scripts()
                        .iter()
                        .any(|s| s.contains("/release/romraider-cli-${{ matrix.target }}.tar.gz"))
                });
            if !staged {
                problems
                    .push("`build-cli` does not publish romraider-cli-<target>.tar.gz".to_owned());
            }
            if !scripts.contains("/romraider-cli\" --version") {
                problems.push(
                    "`build-cli` does not run romraider-cli --version from the tarball".to_owned(),
                );
            }
            if !job
                .lines
                .iter()
                .any(|l| l.trim() == format!("- target: {TARGET}"))
            {
                problems.push(format!("`build-cli` matrix lacks {TARGET}"));
            }
        }
    }

    // publish: SHA256SUMS over every asset, gh release, nightly pre-release.
    match wf.job("publish") {
        None => problems.push("release.yml has no `publish` job".to_owned()),
        Some(job) => {
            let scripts = job.run_scripts().join("\n");
            if !scripts.contains("> SHA256SUMS") || !scripts.contains("sha256sum -c SHA256SUMS") {
                problems.push("`publish` does not write and verify SHA256SUMS".to_owned());
            }
            if !job.lines.iter().any(|l| l.trim() == "pattern: release-*") {
                problems.push("`publish` does not collect every `release-*` artifact".to_owned());
            }
            if !scripts.contains("gh release create \"$RELEASE_TAG\"") {
                problems.push("`publish` does not `gh release create`".to_owned());
            }
            if !scripts.contains("flags+=(--prerelease)") {
                problems.push("`publish` never marks a release as pre-release".to_owned());
            }
        }
    }
    let nightly_prerelease = wf.job("plan").is_some_and(|j| {
        j.run_scripts()
            .iter()
            .any(|s| s.contains("if [ \"$channel\" = nightly ]; then prerelease=true"))
    });
    if !nightly_prerelease {
        problems.push("`plan` does not always make the nightly a pre-release".to_owned());
    }
    problems
}

fn committed() -> (String, String, String) {
    (
        workflow_yml("release.yml"),
        repo_file("dist-workspace.toml"),
        repo_file("crates/rr-cli/Cargo.toml"),
    )
}

/// US-RR-38-5: the committed release wiring holds.
#[test]
fn release_config_builds_and_publishes_cli_tarball() {
    let (release, dist, cli) = committed();
    let problems = release_problems(&release, &dist, &cli);
    assert!(
        problems.is_empty(),
        "release.yml / dist-workspace.toml violate US-RR-38-5:\n{}",
        problems.join("\n")
    );
}

/// Each assertion bites on a mutated copy of the committed files.
#[test]
fn release_config_rejects_broken_wiring() {
    let (release, dist, cli) = committed();
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        release_problems(&changed, &dist, &cli)
    };
    let dst = |from: &str, to: &str| {
        let changed = dist.replacen(from, to, 1);
        assert_ne!(changed, dist, "mutation {from:?} did not apply");
        release_problems(&release, &changed, &cli)
    };
    let cl = |from: &str, to: &str| {
        let changed = cli.replacen(from, to, 1);
        assert_ne!(changed, cli, "mutation {from:?} did not apply");
        release_problems(&release, &dist, &changed)
    };

    assert_flags(
        &rel("    - cron:", "    # - cron:"),
        "no nightly `schedule`",
    );
    assert_flags(&rel("      - \"v[0-9]", "      - \"x[0-9]"), "`v*` tags");
    assert_flags(
        &rel("  workflow_dispatch:\n", "  workflow_run:\n"),
        "no `workflow_dispatch`",
    );
    assert_flags(
        &rel("DIST_VERSION: \"0.33.0\"", "DIST_VERSION: \"0.32.0\""),
        "DIST_VERSION",
    );
    assert_flags(
        &dst(
            "cargo-dist-version = \"0.33.0\"",
            "cargo-dist-version = \"0.32.0\"",
        ),
        "DIST_VERSION",
    );
    assert_flags(
        &rel("DIST_SHA256: \"4b3f", "DIST_SHA256: \"zz3f"),
        "not a SHA-256",
    );
    assert_flags(
        &rel("| sha256sum -c -", "| cat"),
        "installs dist without checking DIST_SHA256",
    );
    assert_flags(
        &dst("allow-dirty = [\"ci\"]", "allow-dirty = []"),
        "allow-dirty",
    );
    assert_flags(
        &dst("unix-archive = \".tar.gz\"", "unix-archive = \".tar.xz\""),
        "unix-archive",
    );
    assert_flags(
        &dst(
            "targets = [\"x86_64-unknown-linux-gnu\"]",
            "targets = [\"aarch64-unknown-linux-gnu\"]",
        ),
        "targets lack",
    );
    assert_flags(&cl("dist = true", "dist = false"), "dist = true");
    assert_flags(
        &cl("name = \"romraider-cli\"", "name = \"rr\""),
        "not romraider-cli",
    );
    assert_flags(
        &rel("--artifacts=local", "--artifacts=all"),
        "dist build --artifacts=local",
    );
    assert_flags(
        &rel(
            "\"$RUNNER_TEMP/release/romraider-cli-${{ matrix.target }}.tar.gz\"\n",
            "\"$RUNNER_TEMP/release/rr-cli-${{ matrix.target }}.tar.gz\"\n",
        ),
        "romraider-cli-<target>.tar.gz",
    );
    assert_flags(
        &rel("/romraider-cli\" --version", "/romraider-cli\" --help"),
        "--version from the tarball",
    );
    assert_flags(&rel("> SHA256SUMS", "> sums.txt"), "SHA256SUMS");
    assert_flags(
        &rel("pattern: release-*", "pattern: release-cli-*"),
        "every `release-*`",
    );
    assert_flags(
        &rel(
            "gh release create \"$RELEASE_TAG\"",
            "gh release upload \"$RELEASE_TAG\"",
        ),
        "gh release create",
    );
    assert_flags(&rel("flags+=(--prerelease)", "flags+=()"), "pre-release");
    assert_flags(
        &rel(
            "            prerelease=true\n",
            "            prerelease=false\n",
        ),
        "always make the nightly a pre-release",
    );
}

// --- AppImage (US-RR-38-6) ----------------------------------------------------

/// The AppImage published by job `build-appimage`.
const APPIMAGE: &str = "romraider-cli-x86_64.AppImage";

/// The committed AppImage recipe under packaging/appimage/.
struct Recipe {
    build_sh: String,
    app_run: String,
    desktop: String,
    /// File names in the committed AppDir skeleton.
    skeleton: Vec<String>,
}

fn committed_recipe() -> Recipe {
    let skeleton_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/appimage/AppDir");
    let mut skeleton: Vec<String> = fs::read_dir(&skeleton_dir)
        .map(|d| {
            d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    skeleton.sort();
    Recipe {
        build_sh: repo_file("packaging/appimage/build.sh"),
        app_run: repo_file("packaging/appimage/AppDir/AppRun"),
        desktop: repo_file("packaging/appimage/AppDir/romraider-cli.desktop"),
        skeleton,
    }
}

/// `Key=value` entries of the `[Desktop Entry]` group.
fn desktop_entry(desktop: &str) -> Vec<(&str, &str)> {
    let mut in_group = false;
    let mut out = Vec::new();
    for line in desktop.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
        } else if in_group
            && !line.starts_with('#')
            && let Some((k, v)) = line.split_once('=')
        {
            out.push((k.trim(), v.trim()));
        }
    }
    out
}

fn is_sha256(v: Option<String>) -> bool {
    v.is_some_and(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Every way release.yml and packaging/appimage/ fail the US-RR-38-6 wiring:
/// a pinned appimagetool (and runtime) builds the romraider-cli AppImage from
/// the tarball's binary, smoke-tests it, and `publish` ships it. Empty when
/// it holds.
fn appimage_problems(release: &str, recipe: &Recipe) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);

    // Pins: version + SHA-256 for appimagetool and for the runtime.
    for (version, sha) in [
        ("APPIMAGETOOL_VERSION", "APPIMAGETOOL_SHA256"),
        ("APPIMAGE_RUNTIME_VERSION", "APPIMAGE_RUNTIME_SHA256"),
    ] {
        let v = top_env(&wf, version);
        if v.as_deref()
            .is_none_or(|v| v.is_empty() || v == "continuous")
        {
            problems.push(format!(
                "release.yml {version} is not a pinned release ({v:?})"
            ));
        }
        if !is_sha256(top_env(&wf, sha)) {
            problems.push(format!("release.yml {sha} is not a SHA-256 hex digest"));
        }
    }
    // Any job that downloads either tool checks it against its pin.
    for job in &wf.jobs {
        for s in job.run_scripts() {
            for (repo, version, sha) in [
                (
                    "AppImage/appimagetool/releases/download/",
                    "APPIMAGETOOL_VERSION",
                    "APPIMAGETOOL_SHA256",
                ),
                (
                    "AppImage/type2-runtime/releases/download/",
                    "APPIMAGE_RUNTIME_VERSION",
                    "APPIMAGE_RUNTIME_SHA256",
                ),
            ] {
                if !s.contains(repo) {
                    continue;
                }
                if !s.contains(&format!("{repo}${{{version}}}/")) {
                    problems.push(format!(
                        "release.yml job `{}` downloads {repo} without ${{{version}}}",
                        job.id
                    ));
                }
                if !(s.contains(&format!("${{{sha}}}")) && s.contains("sha256sum -c")) {
                    problems.push(format!(
                        "release.yml job `{}` downloads {repo} without checking {sha}",
                        job.id
                    ));
                }
            }
        }
    }

    match wf.job("build-appimage") {
        None => problems.push("release.yml has no `build-appimage` job".to_owned()),
        Some(job) => {
            if !job.needs().iter().any(|n| n == "build-cli") {
                problems.push("`build-appimage` does not need `build-cli`".to_owned());
            }
            let content: Vec<&str> = job.content_lines().collect();
            if !content.contains(&format!("name: release-cli-{TARGET}").as_str()) {
                problems.push(format!(
                    "`build-appimage` does not download the release-cli-{TARGET} artifact"
                ));
            }
            if !content.contains(&format!("APPIMAGE: {APPIMAGE}").as_str()) {
                problems.push(format!(
                    "`build-appimage` does not name the AppImage {APPIMAGE}"
                ));
            }
            let scripts = job.run_scripts().join("\n");
            for tool in ["appimagetool/releases", "type2-runtime/releases"] {
                if !scripts.contains(tool) {
                    problems.push(format!("`build-appimage` does not download {tool}"));
                }
            }
            if !scripts.contains("bash packaging/appimage/build.sh ")
                || !scripts.contains("APPIMAGE_RUNTIME=\"$RUNNER_TEMP/runtime-x86_64\"")
            {
                problems.push(
                    "`build-appimage` does not run packaging/appimage/build.sh with the pinned runtime"
                        .to_owned(),
                );
            }
            if !scripts
                .contains("\"$RUNNER_TEMP/release/$APPIMAGE\" --appimage-extract-and-run --version")
                || !scripts.contains("test \"$version\" = \"$expected\"")
            {
                problems.push(
                    "`build-appimage` does not smoke-test romraider-cli --version from the AppImage"
                        .to_owned(),
                );
            }
            if !content
                .iter()
                .any(|l| l.starts_with("name: release-appimage-"))
            {
                problems.push(
                    "`build-appimage` does not upload a `release-appimage-*` artifact".to_owned(),
                );
            }
        }
    }
    if !wf
        .job("publish")
        .is_some_and(|j| j.needs().iter().any(|n| n == "build-appimage"))
    {
        problems.push(
            "`publish` does not need `build-appimage`, so the AppImage is not released".to_owned(),
        );
    }

    // The recipe: build.sh, AppRun, desktop entry, icon.
    let b = &recipe.build_sh;
    if !b.contains("--runtime-file \"$APPIMAGE_RUNTIME\"") {
        problems.push(
            "build.sh does not pass --runtime-file (appimagetool would fetch an unpinned runtime)"
                .to_owned(),
        );
    }
    for (from, to) in [
        ("\"$bin\"", "\"$appdir/usr/bin/romraider-cli\""),
        ("\"$skeleton/AppRun\"", "\"$appdir/AppRun\""),
        (
            "\"$skeleton/romraider-cli.desktop\"",
            "\"$appdir/romraider-cli.desktop\"",
        ),
        (
            "\"$skeleton/romraider-cli.svg\"",
            "\"$appdir/romraider-cli.svg\"",
        ),
    ] {
        if !b.contains(&format!("{from} {to}")) {
            problems.push(format!("build.sh does not install {from} as {to}"));
        }
    }
    if !recipe.app_run.starts_with("#!/bin/sh\n")
        || !recipe
            .app_run
            .lines()
            .any(|l| l.trim() == "exec \"${APPDIR:-$HERE}/usr/bin/romraider-cli\" \"$@\"")
    {
        problems.push(
            "AppRun does not exec usr/bin/romraider-cli with the caller's arguments".to_owned(),
        );
    }
    let entry = desktop_entry(&recipe.desktop);
    let get = |k: &str| entry.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    for (key, want) in [
        ("Type", "Application"),
        ("Exec", "romraider-cli"),
        ("Terminal", "true"),
    ] {
        if get(key) != Some(want) {
            problems.push(format!(
                "romraider-cli.desktop {key} is {:?}, not {want:?}",
                get(key)
            ));
        }
    }
    if get("Name").is_none_or(str::is_empty) {
        problems.push("romraider-cli.desktop has no Name".to_owned());
    }
    if get("Categories").is_none_or(|c| c.is_empty() || !c.ends_with(';')) {
        problems.push("romraider-cli.desktop has no `;`-terminated Categories".to_owned());
    }
    match get("Icon") {
        None => problems.push("romraider-cli.desktop has no Icon".to_owned()),
        Some(icon) => {
            let found = ["svg", "png"]
                .iter()
                .any(|ext| recipe.skeleton.contains(&format!("{icon}.{ext}")));
            if !found {
                problems.push(format!("AppDir has no icon {icon}.svg or {icon}.png"));
            }
        }
    }
    for f in ["AppRun", "romraider-cli.desktop"] {
        if !recipe.skeleton.iter().any(|s| s == f) {
            problems.push(format!("AppDir skeleton lacks {f}"));
        }
    }
    problems
}

/// US-RR-38-6: the committed AppImage recipe and its release.yml job hold.
#[test]
fn release_config_builds_and_publishes_appimage() {
    let problems = appimage_problems(&workflow_yml("release.yml"), &committed_recipe());
    assert!(
        problems.is_empty(),
        "release.yml / packaging/appimage violate US-RR-38-6:\n{}",
        problems.join("\n")
    );
}

/// Each AppImage assertion bites on a mutated copy of the committed files.
#[test]
fn appimage_config_rejects_broken_wiring() {
    let release = workflow_yml("release.yml");
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        appimage_problems(&changed, &committed_recipe())
    };
    let rec = |edit: &dyn Fn(&mut Recipe)| {
        let mut r = committed_recipe();
        edit(&mut r);
        appimage_problems(&release, &r)
    };
    let swap = |s: &mut String, from: &str, to: &str| {
        let changed = s.replacen(from, to, 1);
        assert_ne!(&changed, s, "mutation {from:?} did not apply");
        *s = changed;
    };

    assert_flags(
        &rel(
            "APPIMAGETOOL_VERSION: \"1.9.1\"",
            "APPIMAGETOOL_VERSION: \"continuous\"",
        ),
        "APPIMAGETOOL_VERSION is not a pinned release",
    );
    assert_flags(
        &rel("APPIMAGETOOL_SHA256: \"ed4c", "APPIMAGETOOL_SHA256: \"zz4c"),
        "APPIMAGETOOL_SHA256 is not a SHA-256",
    );
    assert_flags(
        &rel(
            "APPIMAGE_RUNTIME_SHA256: \"2fca",
            "APPIMAGE_RUNTIME_SHA256: \"2fc",
        ),
        "APPIMAGE_RUNTIME_SHA256 is not a SHA-256",
    );
    assert_flags(
        &rel("  APPIMAGE_RUNTIME_VERSION: \"20251108\"\n", ""),
        "APPIMAGE_RUNTIME_VERSION is not a pinned release",
    );
    assert_flags(
        &rel(
            "appimagetool/releases/download/${APPIMAGETOOL_VERSION}/",
            "appimagetool/releases/download/continuous/",
        ),
        "without ${APPIMAGETOOL_VERSION}",
    );
    assert_flags(
        &rel(
            "echo \"${APPIMAGETOOL_SHA256}  $RUNNER_TEMP/appimagetool-x86_64.AppImage\" | sha256sum -c -",
            "true",
        ),
        "without checking APPIMAGETOOL_SHA256",
    );
    assert_flags(
        &rel(
            "echo \"${APPIMAGE_RUNTIME_SHA256}  $RUNNER_TEMP/runtime-x86_64\" | sha256sum -c -",
            "true",
        ),
        "without checking APPIMAGE_RUNTIME_SHA256",
    );
    assert_flags(
        &rel("\n  build-appimage:\n", "\n  appimage:\n"),
        "no `build-appimage` job",
    );
    assert_flags(
        &rel("    needs: [plan, build-cli]\n", "    needs: [plan, ci]\n"),
        "`build-appimage` does not need `build-cli`",
    );
    assert_flags(
        &rel(
            "          name: release-cli-x86_64-unknown-linux-gnu\n",
            "          name: release-cli\n",
        ),
        "does not download the release-cli-",
    );
    assert_flags(
        &rel(
            "APPIMAGE: romraider-cli-x86_64.AppImage",
            "APPIMAGE: romraider-cli.AppImage",
        ),
        "does not name the AppImage",
    );
    assert_flags(
        &rel(
            "bash packaging/appimage/build.sh ",
            "bash packaging/build.sh ",
        ),
        "does not run packaging/appimage/build.sh",
    );
    assert_flags(
        &rel(
            "--appimage-extract-and-run --version",
            "--appimage-extract-and-run --help",
        ),
        "does not smoke-test",
    );
    assert_flags(
        &rel("test \"$version\" = \"$expected\"", "true"),
        "does not smoke-test",
    );
    assert_flags(
        &rel("name: release-appimage-x86_64", "name: appimage-x86_64"),
        "`release-appimage-*` artifact",
    );
    assert_flags(
        &rel(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, test-appimage, build-deb, test-deb]\n",
        ),
        "`publish` does not need `build-appimage`",
    );

    assert_flags(
        &rec(&|r| swap(&mut r.build_sh, "--runtime-file \"$APPIMAGE_RUNTIME\" ", "")),
        "--runtime-file",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.build_sh,
                "\"$bin\" \"$appdir/usr/bin/romraider-cli\"",
                "\"$bin\" \"$appdir/romraider-cli\"",
            )
        }),
        "does not install \"$bin\"",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.app_run,
                "/usr/bin/romraider-cli\" \"$@\"",
                "/usr/bin/romraider-cli\"",
            )
        }),
        "AppRun does not exec",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "Exec=romraider-cli", "Exec=rr")),
        "Exec is",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "Type=Application", "Type=Link")),
        "Type is",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "Terminal=true", "Terminal=false")),
        "Terminal is",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "Name=RomRaider CLI\n", "")),
        "has no Name",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "Categories=Utility;", "Categories=Utility")),
        "Categories",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "Icon=romraider-cli", "Icon=romraider")),
        "no icon romraider.svg",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.desktop, "[Desktop Entry]", "[Desktop Action x]")),
        "has no Icon",
    );
    assert_flags(
        &rec(&|r| r.skeleton.retain(|f| f != "AppRun")),
        "skeleton lacks AppRun",
    );
}

// --- AppImage on a clean Ubuntu 22.04 (US-RR-38-3) ---------------------------

/// The job that runs the uploaded AppImage in a clean ubuntu:22.04 container.
const APPIMAGE_TEST_JOB: &str = "test-appimage";
/// The artifact `build-appimage` uploads and `test-appimage` downloads.
const APPIMAGE_ARTIFACT: &str = "release-appimage-x86_64";
/// The `docker run` that is the test: fresh container (`--rm`), no network,
/// unprivileged, only the AppImage's directory mounted, read-only.
const CONTAINER_RUN: &str = "version=$(docker run --rm --network none --user 65534:65534 -v \"$RUNNER_TEMP/appimage:/appimage:ro\" \"$UBUNTU_22_04_IMAGE\" \"/appimage/$APPIMAGE\" --appimage-extract-and-run --version)";
const CONTAINER_EXPECT: &str = "test \"$version\" = \"romraider-cli ${DIST_TAG#v}\"";

/// A job's `run:` scripts with shell line continuations joined.
fn joined_scripts(job: &workflow::Job<'_>) -> Vec<String> {
    job.run_scripts()
        .into_iter()
        .map(|s| s.replace(" \\ ", " "))
        .collect()
}

/// Every way release.yml lets an AppImage ship without having run
/// `romraider-cli --version` on a clean Ubuntu 22.04; empty when it cannot.
fn appimage_container_problems(release: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);

    // The image: ubuntu:22.04, pinned by digest.
    let image = top_env(&wf, "UBUNTU_22_04_IMAGE");
    let pinned = image
        .as_deref()
        .and_then(|i| i.strip_prefix("ubuntu:22.04@sha256:"))
        .is_some_and(|d| d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit()));
    if !pinned {
        problems.push(format!(
            "release.yml UBUNTU_22_04_IMAGE is not ubuntu:22.04 pinned by a sha256 digest ({image:?})"
        ));
    }

    let Some(job) = wf.job(APPIMAGE_TEST_JOB) else {
        problems.push(format!("release.yml has no `{APPIMAGE_TEST_JOB}` job"));
        return problems;
    };
    let needs = job.needs();
    if !needs.iter().any(|n| n == "build-appimage") {
        problems.push(format!(
            "`{APPIMAGE_TEST_JOB}` does not need `build-appimage`"
        ));
    }
    if !needs.iter().any(|n| n == "plan") {
        problems.push(format!(
            "`{APPIMAGE_TEST_JOB}` does not need `plan` (the release version)"
        ));
    }
    let content: Vec<&str> = job.content_lines().collect();
    if !content.contains(&format!("APPIMAGE: {APPIMAGE}").as_str()) {
        problems.push(format!(
            "`{APPIMAGE_TEST_JOB}` does not test the AppImage {APPIMAGE}"
        ));
    }
    let uploaded = wf.job("build-appimage").is_some_and(|j| {
        j.content_lines()
            .any(|l| l == format!("name: {APPIMAGE_ARTIFACT}"))
    });
    let steps = job.steps();
    let download = steps.iter().position(|st| {
        st.id.starts_with("actions/download-artifact@")
            && st
                .content_lines()
                .any(|l| l == format!("name: {APPIMAGE_ARTIFACT}"))
            && st
                .content_lines()
                .any(|l| l == "path: ${{ runner.temp }}/appimage")
    });
    if !uploaded || download.is_none() {
        problems.push(format!(
            "`{APPIMAGE_TEST_JOB}` does not download the `{APPIMAGE_ARTIFACT}` artifact `build-appimage` uploads into $RUNNER_TEMP/appimage"
        ));
    }
    let run = steps
        .iter()
        .position(|st| joined_scripts(st).iter().any(|s| s.contains(CONTAINER_RUN)));
    match run {
        None => problems.push(format!(
            "`{APPIMAGE_TEST_JOB}` does not run the AppImage's --version in a fresh, offline, unprivileged $UBUNTU_22_04_IMAGE container (`{CONTAINER_RUN}`)"
        )),
        Some(r) => {
            let step = &steps[r];
            let script = joined_scripts(step).join("\n");
            if !script.contains(CONTAINER_EXPECT) {
                problems.push(format!(
                    "`{APPIMAGE_TEST_JOB}` does not check the container's output is the release version (`{CONTAINER_EXPECT}`)"
                ));
            }
            if !step
                .map_at("env", 8)
                .contains(&("DIST_TAG".to_owned(), "${{ needs.plan.outputs.dist-tag }}".to_owned()))
            {
                problems.push(format!(
                    "`{APPIMAGE_TEST_JOB}` DIST_TAG is not the planned release tag"
                ));
            }
            if download.is_none_or(|d| d > r) {
                problems.push(format!(
                    "`{APPIMAGE_TEST_JOB}` runs the container before downloading the AppImage"
                ));
            }
        }
    }
    // Clean: nothing installed or added to the container, no host devices.
    for s in joined_scripts(job) {
        for bad in [
            "apt-get",
            "apt install",
            "--privileged",
            "/dev/fuse",
            "--device",
            "--cap-add",
            "docker exec",
            "docker build",
            "docker commit",
        ] {
            if s.contains(bad) {
                problems.push(format!(
                    "`{APPIMAGE_TEST_JOB}` does not test a clean container (`{bad}`)"
                ));
            }
        }
        if s.contains("docker run") && (s.matches(" -v ").count() > 1 || s.contains("--mount")) {
            problems.push(format!(
                "`{APPIMAGE_TEST_JOB}` mounts more than the AppImage into the container"
            ));
        }
    }
    if content.iter().any(|l| l.starts_with("container:")) {
        problems.push(format!(
            "`{APPIMAGE_TEST_JOB}` runs in a job container (the runner's tools are mounted in); use `docker run`"
        ));
    }
    // Not skippable.
    problems.extend(skippable_problems(job, &[]));
    for l in job.lines.iter().filter(|l| workflow::indent(l) == 4) {
        if l.trim().starts_with("if:") {
            problems.push(format!(
                "`{APPIMAGE_TEST_JOB}` has a job-level conditional (`{}`)",
                l.trim()
            ));
        }
    }
    if !wf
        .job("publish")
        .is_some_and(|j| j.needs().iter().any(|n| n == APPIMAGE_TEST_JOB))
    {
        problems.push(format!(
            "`publish` does not need `{APPIMAGE_TEST_JOB}`, so an AppImage that fails on Ubuntu 22.04 still ships"
        ));
    }
    problems
}

/// US-RR-38-3: the uploaded AppImage runs `romraider-cli --version` in a
/// clean ubuntu:22.04 container before anything is published.
#[test]
fn release_config_runs_appimage_on_clean_ubuntu_22_04() {
    let problems = appimage_container_problems(&workflow_yml("release.yml"));
    assert!(
        problems.is_empty(),
        "release.yml violates US-RR-38-3:\n{}",
        problems.join("\n")
    );
}

/// Each clean-container assertion bites on a mutated copy of release.yml.
#[test]
fn appimage_container_config_rejects_broken_wiring() {
    let release = workflow_yml("release.yml");
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        appimage_container_problems(&changed)
    };
    // Mutations of the test job only (its header line to the next job).
    let start = release
        .find("\n  test-appimage:\n")
        .expect("test-appimage job");
    let end = start
        + 1
        + release[start + 1..]
            .find("\n  build-deb:")
            .expect("next job");
    let job = &release[start..end];
    let in_job = |from: &str, to: &str| {
        let changed = job.replacen(from, to, 1);
        assert_ne!(changed, job, "mutation {from:?} did not apply");
        appimage_container_problems(&release.replacen(job, &changed, 1))
    };

    assert!(appimage_container_problems(&release).is_empty());
    assert_flags(
        &rel(
            "UBUNTU_22_04_IMAGE: \"ubuntu:22.04@sha256:",
            "UBUNTU_22_04_IMAGE: \"ubuntu:24.04@sha256:",
        ),
        "not ubuntu:22.04 pinned",
    );
    assert_flags(
        &rel(
            "UBUNTU_22_04_IMAGE: \"ubuntu:22.04@sha256:0a9887931b0e38af2e3487ed39e29436b0535c4f3f52e57745baaf79d1898956\"",
            "UBUNTU_22_04_IMAGE: \"ubuntu:22.04\"",
        ),
        "not ubuntu:22.04 pinned",
    );
    assert_flags(
        &rel("\n  test-appimage:\n", "\n  appimage-smoke:\n"),
        "no `test-appimage` job",
    );
    assert_flags(
        &in_job(
            "    needs: [plan, build-appimage]\n",
            "    needs: [plan, build-cli]\n",
        ),
        "does not need `build-appimage`",
    );
    assert_flags(
        &in_job(
            "    needs: [plan, build-appimage]\n",
            "    needs: [build-appimage]\n",
        ),
        "does not need `plan`",
    );
    assert_flags(
        &in_job(
            "APPIMAGE: romraider-cli-x86_64.AppImage",
            "APPIMAGE: romraider-cli.AppImage",
        ),
        "does not test the AppImage",
    );
    assert_flags(
        &in_job(
            "          name: release-appimage-x86_64\n",
            "          name: release-cli-x86_64-unknown-linux-gnu\n",
        ),
        "does not download the `release-appimage-x86_64` artifact",
    );
    // build-appimage uploads under another name: the download gets nothing.
    assert_flags(
        &rel("name: release-appimage-x86_64", "name: release-appimage"),
        "does not download the `release-appimage-x86_64` artifact",
    );
    assert_flags(
        &in_job("--network none ", ""),
        "fresh, offline, unprivileged",
    );
    assert_flags(
        &in_job("--user 65534:65534 ", ""),
        "fresh, offline, unprivileged",
    );
    assert_flags(
        &in_job("docker run --rm ", "docker run "),
        "fresh, offline, unprivileged",
    );
    assert_flags(&in_job("/appimage:ro\"", "/appimage\""), "fresh, offline");
    assert_flags(
        &in_job("\"$UBUNTU_22_04_IMAGE\" \\\n", "ubuntu:22.04 \\\n"),
        "fresh, offline",
    );
    assert_flags(
        &in_job(
            "--appimage-extract-and-run --version)",
            "--appimage-extract-and-run --help)",
        ),
        "fresh, offline",
    );
    // The smoke test moved onto the runner itself.
    assert_flags(
        &in_job(
            "version=$(docker run --rm --network none --user 65534:65534 \\\n            -v \"$RUNNER_TEMP/appimage:/appimage:ro\" \"$UBUNTU_22_04_IMAGE\" \\\n            \"/appimage/$APPIMAGE\"",
            "version=$(\"$RUNNER_TEMP/appimage/$APPIMAGE\"",
        ),
        "fresh, offline",
    );
    assert_flags(
        &in_job(CONTAINER_EXPECT, "test -n \"$version\""),
        "is the release version",
    );
    assert_flags(
        &in_job(
            "DIST_TAG: ${{ needs.plan.outputs.dist-tag }}",
            "DIST_TAG: ${{ needs.plan.outputs.release-tag }}",
        ),
        "DIST_TAG is not the planned release tag",
    );
    assert_flags(
        &in_job(
            "\"$UBUNTU_22_04_IMAGE\" \\\n",
            "\"$UBUNTU_22_04_IMAGE\" \\\n            sh -c 'apt-get update; apt-get install -y libfuse2; \"$@\"' - \\\n",
        ),
        "not test a clean container (`apt-get`)",
    );
    assert_flags(
        &in_job("--network none ", "--network none --privileged "),
        "not test a clean container (`--privileged`)",
    );
    assert_flags(
        &in_job("--network none ", "--network none --device /dev/fuse "),
        "not test a clean container (`/dev/fuse`)",
    );
    assert_flags(
        &in_job(
            "-v \"$RUNNER_TEMP/appimage:/appimage:ro\" ",
            "-v \"$RUNNER_TEMP/appimage:/appimage:ro\" -v /usr:/usr:ro ",
        ),
        "mounts more than the AppImage",
    );
    assert_flags(
        &in_job(
            "    runs-on: ubuntu-24.04\n",
            "    runs-on: ubuntu-24.04\n    container: ubuntu:22.04\n",
        ),
        "runs in a job container",
    );
    assert_flags(
        &in_job(
            "    timeout-minutes: 10\n",
            "    timeout-minutes: 10\n    if: needs.plan.outputs.publish == 'true'\n",
        ),
        "job-level conditional",
    );
    assert_flags(
        &in_job(
            "    timeout-minutes: 10\n",
            "    timeout-minutes: 10\n    continue-on-error: true\n",
        ),
        "uses `continue-on-error: true`",
    );
    assert_flags(
        &in_job(
            "      - name: Smoke test (romraider-cli --version in a clean ubuntu:22.04 container)\n",
            "      - name: Smoke test (romraider-cli --version in a clean ubuntu:22.04 container)\n        if: github.event_name == 'push'\n",
        ),
        "has a conditional",
    );
    assert_flags(
        &in_job(CONTAINER_EXPECT, &format!("{CONTAINER_EXPECT} || true")),
        "swallows failures",
    );
    // The download moved after the run.
    let download = "      - uses: actions/download-artifact@v4\n        with:\n          name: release-appimage-x86_64\n          path: ${{ runner.temp }}/appimage\n";
    assert!(job.contains(download), "download step not found verbatim");
    let moved = format!("{}{download}", job.replacen(download, "", 1));
    assert_flags(
        &appimage_container_problems(&release.replacen(job, &moved, 1)),
        "runs the container before downloading",
    );
    assert_flags(
        &rel(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, build-appimage, build-deb, test-deb]\n",
        ),
        "`publish` does not need `test-appimage`",
    );
}

// --- .deb (US-RR-38-7) ---------------------------------------------------------

/// The committed .deb recipe: rr-cli's manifest and packaging/deb/build.sh.
struct DebRecipe {
    cli_toml: String,
    build_sh: String,
}

fn committed_deb_recipe() -> DebRecipe {
    DebRecipe {
        cli_toml: repo_file("crates/rr-cli/Cargo.toml"),
        build_sh: repo_file("packaging/deb/build.sh"),
    }
}

/// The assets the .deb must hold, as cargo-deb `[source, dest dir, mode]`
/// (sources relative to crates/rr-cli; `target/release/` is cargo-deb's name
/// for the built binary). The desktop entry and icon are the AppImage's.
const DEB_ASSETS: &[[&str; 3]] = &[
    ["target/release/romraider-cli", "usr/bin/", "755"],
    [
        "../../packaging/appimage/AppDir/romraider-cli.desktop",
        "usr/share/applications/",
        "644",
    ],
    [
        "../../packaging/appimage/AppDir/romraider-cli.svg",
        "usr/share/icons/hicolor/scalable/apps/",
        "644",
    ],
];

/// Every way release.yml, rr-cli's `[package.metadata.deb]` and
/// packaging/deb/build.sh fail the US-RR-38-7 wiring: a pinned cargo-deb
/// packages the tarball's binary with the desktop entry and icon, the job
/// installs and runs it, and `publish` ships it. Empty when it holds.
fn deb_problems(release: &str, recipe: &DebRecipe) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);

    // Pin: an exact version, installed with its own lockfile.
    let pinned = top_env(&wf, "CARGO_DEB_VERSION").is_some_and(|v| {
        let parts: Vec<&str> = v.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    });
    if !pinned {
        problems.push("release.yml CARGO_DEB_VERSION is not an exact x.y.z version".to_owned());
    }
    for job in &wf.jobs {
        for s in job.run_scripts() {
            if s.contains("cargo install cargo-deb")
                && !s
                    .contains("cargo install cargo-deb --version \"${CARGO_DEB_VERSION}\" --locked")
            {
                problems.push(format!(
                    "release.yml job `{}` installs cargo-deb without --version \"${{CARGO_DEB_VERSION}}\" --locked",
                    job.id
                ));
            }
        }
    }

    match wf.job("build-deb") {
        None => problems.push("release.yml has no `build-deb` job".to_owned()),
        Some(job) => {
            if !job.needs().iter().any(|n| n == "build-cli") {
                problems.push("`build-deb` does not need `build-cli`".to_owned());
            }
            let content: Vec<&str> = job.content_lines().collect();
            if !content.contains(&format!("name: release-cli-{TARGET}").as_str()) {
                problems.push(format!(
                    "`build-deb` does not download the release-cli-{TARGET} artifact"
                ));
            }
            let scripts = job.run_scripts().join("\n");
            if !scripts.contains("cargo install cargo-deb ") {
                problems.push("`build-deb` does not install cargo-deb".to_owned());
            }
            if !scripts.contains("bash packaging/deb/build.sh ")
                || !scripts.contains("CARGO_DEB=\"$RUNNER_TEMP/cargo-deb/bin/cargo-deb\"")
            {
                problems.push(
                    "`build-deb` does not run packaging/deb/build.sh with the pinned cargo-deb"
                        .to_owned(),
                );
            }
            if !scripts.contains("\"$RUNNER_TEMP\"/release/romraider-cli_*_amd64.deb)") {
                problems.push(
                    "`build-deb` does not check the name romraider-cli_*_amd64.deb".to_owned(),
                );
            }
            if !scripts.contains("sudo dpkg -i \"$DEB\"")
                || !scripts.contains("version=$(/usr/bin/romraider-cli --version)")
                || !scripts.contains("test \"$version\" = \"$expected\"")
            {
                problems.push(
                    "`build-deb` does not smoke-test the installed romraider-cli --version"
                        .to_owned(),
                );
            }
            if !content.iter().any(|l| l.starts_with("name: release-deb-")) {
                problems.push("`build-deb` does not upload a `release-deb-*` artifact".to_owned());
            }
        }
    }
    if !wf
        .job("publish")
        .is_some_and(|j| j.needs().iter().any(|n| n == "build-deb"))
    {
        problems
            .push("`publish` does not need `build-deb`, so the .deb is not released".to_owned());
    }

    // [package.metadata.deb].
    match toml::from_str::<toml::Table>(&recipe.cli_toml) {
        Err(e) => problems.push(format!("crates/rr-cli/Cargo.toml does not parse: {e}")),
        Ok(cli) => {
            let deb = cli
                .get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("deb"))
                .and_then(|d| d.as_table());
            match deb {
                None => problems.push("rr-cli has no [package.metadata.deb]".to_owned()),
                Some(deb) => {
                    let s = |k: &str| deb.get(k).and_then(|v| v.as_str());
                    if s("name") != Some("romraider-cli") {
                        problems.push(format!(
                            "[package.metadata.deb] name is {:?}, not \"romraider-cli\"",
                            s("name")
                        ));
                    }
                    if s("maintainer").is_none_or(|m| !(m.contains('<') && m.ends_with('>'))) {
                        problems.push(
                            "[package.metadata.deb] maintainer is not `Name <contact>`".to_owned(),
                        );
                    }
                    if s("depends") != Some("$auto") {
                        problems.push(
                            "[package.metadata.deb] depends is not \"$auto\" (libc6 floor)"
                                .to_owned(),
                        );
                    }
                    let assets: Option<Vec<Vec<&str>>> =
                        deb.get("assets").and_then(|a| a.as_array()).map(|a| {
                            a.iter()
                                .map(|e| {
                                    e.as_array()
                                        .map(|f| f.iter().filter_map(|v| v.as_str()).collect())
                                        .unwrap_or_default()
                                })
                                .collect()
                        });
                    let want: Vec<Vec<&str>> = DEB_ASSETS.iter().map(|a| a.to_vec()).collect();
                    if assets.as_ref() != Some(&want) {
                        problems.push(format!(
                            "[package.metadata.deb] assets are {assets:?}, not exactly the binary, desktop entry and icon {want:?}"
                        ));
                    }
                }
            }
        }
    }

    // build.sh: package the given binary as is. The cargo-deb command is the
    // (backslash-continued) line that runs "$cargo_deb", outside comments.
    let b = &recipe.build_sh;
    let joined = b.replace("\\\n", " ");
    let invocation = joined
        .lines()
        .map(str::trim)
        .find(|l| !l.starts_with('#') && l.contains("\"$cargo_deb\""))
        .unwrap_or_default();
    let words: Vec<&str> = invocation.split_whitespace().collect();
    for (flag, why) in [
        ("--no-build", "would build its own binary"),
        ("--no-strip", "would strip the tarball's binary"),
        ("--locked", "may change Cargo.lock"),
        ("--manifest-path", "does not package rr-cli"),
    ] {
        if !words.contains(&flag) {
            problems.push(format!(
                "build.sh does not pass {flag} to cargo-deb ({why})"
            ));
        }
    }
    if !invocation.contains("--manifest-path \"$repo/crates/rr-cli/Cargo.toml\"") {
        problems.push("build.sh does not package crates/rr-cli".to_owned());
    }
    if !b.contains("install -Dm0755 \"$bin\" \"$work/target/release/romraider-cli\"")
        || !b.contains("CARGO_TARGET_DIR=\"$work/target\"")
    {
        problems.push(
            "build.sh does not stage the given binary as cargo-deb's target/release/romraider-cli"
                .to_owned(),
        );
    }
    problems
}

/// The DEB_ASSETS sources exist in the repo.
#[test]
fn deb_assets_exist() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/rr-cli");
    for [src, ..] in DEB_ASSETS {
        if src.starts_with("target/") {
            continue;
        }
        assert!(crate_dir.join(src).is_file(), "missing .deb asset {src}");
    }
}

/// US-RR-38-7: the committed .deb recipe and its release.yml job hold.
#[test]
fn release_config_builds_and_publishes_deb() {
    let problems = deb_problems(&workflow_yml("release.yml"), &committed_deb_recipe());
    assert!(
        problems.is_empty(),
        "release.yml / rr-cli [package.metadata.deb] / packaging/deb violate US-RR-38-7:\n{}",
        problems.join("\n")
    );
}

/// Each .deb assertion bites on a mutated copy of the committed files.
#[test]
fn deb_config_rejects_broken_wiring() {
    let release = workflow_yml("release.yml");
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        deb_problems(&changed, &committed_deb_recipe())
    };
    let rec = |edit: &dyn Fn(&mut DebRecipe)| {
        let mut r = committed_deb_recipe();
        edit(&mut r);
        deb_problems(&release, &r)
    };
    let swap = |s: &mut String, from: &str, to: &str| {
        let changed = s.replacen(from, to, 1);
        assert_ne!(&changed, s, "mutation {from:?} did not apply");
        *s = changed;
    };

    assert_flags(
        &rel("CARGO_DEB_VERSION: \"3.8.0\"", "CARGO_DEB_VERSION: \"3\""),
        "CARGO_DEB_VERSION is not an exact",
    );
    assert_flags(
        &rel("--version \"${CARGO_DEB_VERSION}\" --locked", "--locked"),
        "installs cargo-deb without",
    );
    assert_flags(
        &rel(
            "--version \"${CARGO_DEB_VERSION}\" --locked",
            "--version \"${CARGO_DEB_VERSION}\"",
        ),
        "installs cargo-deb without",
    );
    assert_flags(&rel("\n  build-deb:\n", "\n  deb:\n"), "no `build-deb` job");
    assert_flags(
        &rel(
            "    needs: [plan, build-cli]\n    runs-on: ubuntu-22.04\n    timeout-minutes: 20\n",
            "    needs: [plan, ci]\n    runs-on: ubuntu-22.04\n    timeout-minutes: 20\n",
        ),
        "`build-deb` does not need `build-cli`",
    );
    // The second download of release-cli is build-deb's.
    let second = release
        .match_indices("          name: release-cli-x86_64-unknown-linux-gnu\n")
        .nth(1)
        .map(|(i, _)| i)
        .expect("build-deb downloads release-cli");
    let mut changed = release.clone();
    changed.replace_range(
        second..second + "          name: release-cli-x86_64-unknown-linux-gnu\n".len(),
        "          name: release-cli\n",
    );
    assert_flags(
        &deb_problems(&changed, &committed_deb_recipe()),
        "`build-deb` does not download the release-cli-",
    );
    assert_flags(
        &rel("bash packaging/deb/build.sh ", "bash packaging/build.sh "),
        "does not run packaging/deb/build.sh",
    );
    assert_flags(
        &rel(
            "deb=$(CARGO_DEB=\"$RUNNER_TEMP/cargo-deb/bin/cargo-deb\" \\",
            "deb=$( \\",
        ),
        "with the pinned cargo-deb",
    );
    assert_flags(
        &rel("romraider-cli_*_amd64.deb)", "*.deb)"),
        "romraider-cli_*_amd64.deb",
    );
    assert_flags(
        &rel("sudo dpkg -i \"$DEB\"", "true"),
        "does not smoke-test the installed",
    );
    assert_flags(
        &rel(
            "version=$(/usr/bin/romraider-cli --version)",
            "version=$(/usr/bin/romraider-cli --help)",
        ),
        "does not smoke-test the installed",
    );
    assert_flags(
        &rel("name: release-deb-amd64", "name: deb-amd64"),
        "`release-deb-*` artifact",
    );
    assert_flags(
        &rel(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, build-appimage, test-appimage, test-deb]\n",
        ),
        "`publish` does not need `build-deb`",
    );

    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.cli_toml,
                "\n[package.metadata.deb]\n",
                "\n[package.metadata.x]\n",
            )
        }),
        "has no [package.metadata.deb]",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.cli_toml,
                "[package.metadata.deb]\nname = \"romraider-cli\"\n",
                "[package.metadata.deb]\nname = \"rr-cli\"\n",
            )
        }),
        "name is",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.cli_toml,
                "maintainer = \"Ryan Tregea <",
                "maintainer = \"Ryan Tregea (",
            )
        }),
        "maintainer is not",
    );
    assert_flags(
        &rec(&|r| swap(&mut r.cli_toml, "depends = \"$auto\"", "depends = \"\"")),
        "depends is not",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.cli_toml,
                "[\"target/release/romraider-cli\", \"usr/bin/\", \"755\"]",
                "[\"target/release/romraider-cli\", \"usr/local/bin/\", \"755\"]",
            )
        }),
        "assets are",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.cli_toml,
                "\"usr/share/applications/\",",
                "\"usr/share/\",",
            )
        }),
        "assets are",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.cli_toml,
                "assets = [\n",
                "assets = [\n    [\"../../fixtures/rom.bin\", \"usr/share/romraider-cli/\", \"644\"],\n",
            )
        }),
        "assets are",
    );
    for flag in ["--no-build", "--no-strip", "--locked"] {
        assert_flags(
            &rec(&|r| swap(&mut r.build_sh, &format!(" {flag} --"), " --")),
            &format!("does not pass {flag}"),
        );
    }
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.build_sh,
                "\"$repo/crates/rr-cli/Cargo.toml\"",
                "\"$repo/Cargo.toml\"",
            )
        }),
        "does not package crates/rr-cli",
    );
    assert_flags(
        &rec(&|r| {
            swap(
                &mut r.build_sh,
                "\"$work/target/release/romraider-cli\"",
                "\"$work/romraider-cli\"",
            )
        }),
        "does not stage the given binary",
    );
}

// --- .deb on clean Ubuntu containers (US-RR-38-2) ---------------------------

/// The job that installs the uploaded .deb in clean Ubuntu containers.
const DEB_TEST_JOB: &str = "test-deb";
/// The artifact `build-deb` uploads and `test-deb` downloads.
const DEB_ARTIFACT: &str = "release-deb-amd64";
/// The matrix: each Ubuntu `test-deb` installs on, and the top-level env var
/// holding its digest-pinned image. 22.04 is the oldest the binary's glibc
/// supports.
const DEB_UBUNTUS: &[(&str, &str)] = &[
    ("22.04", "UBUNTU_22_04_IMAGE"),
    ("24.04", "UBUNTU_24_04_IMAGE"),
];
/// The one package in the downloaded artifact.
const DEB_FIND: &str = "debs=(\"$RUNNER_TEMP\"/deb/romraider-cli_*_amd64.deb) test \"${#debs[@]}\" -eq 1 test -f \"${debs[0]}\" deb=$(basename \"${debs[0]}\")";
/// The `docker run` that is the test (whitespace-normalized): a fresh
/// container (`--rm`) of the matrix image, no network, only the package's
/// directory mounted, read-only; inside it, check it is the expected Ubuntu,
/// `dpkg -i` the package (fails on unmet Depends), check the binary is
/// /usr/bin/romraider-cli, and print its `--version` as the last command.
const DEB_CONTAINER_RUN: &str = "version=$(docker run --rm --network none -v \"$RUNNER_TEMP/deb:/deb:ro\" \"$IMAGE\" sh -euc ' . /etc/os-release test \"$VERSION_ID\" = \"$1\" dpkg -i \"/deb/$2\" >&2 test \"$(command -v romraider-cli)\" = /usr/bin/romraider-cli romraider-cli --version ' - \"$UBUNTU\" \"$deb\")";

/// Every way release.yml lets a .deb ship without having been installed and
/// run (`romraider-cli --version`) in clean Ubuntu containers; empty when it
/// cannot.
fn deb_container_problems(release: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);

    // The images: each Ubuntu pinned by digest.
    for (ubuntu, var) in DEB_UBUNTUS {
        let image = top_env(&wf, var);
        let pinned = image
            .as_deref()
            .and_then(|i| i.strip_prefix(&format!("ubuntu:{ubuntu}@sha256:")))
            .is_some_and(|d| d.len() == 64 && d.chars().all(|c| c.is_ascii_hexdigit()));
        if !pinned {
            problems.push(format!(
                "release.yml {var} is not ubuntu:{ubuntu} pinned by a sha256 digest ({image:?})"
            ));
        }
    }

    let Some(job) = wf.job(DEB_TEST_JOB) else {
        problems.push(format!("release.yml has no `{DEB_TEST_JOB}` job"));
        return problems;
    };
    let needs = job.needs();
    if !needs.iter().any(|n| n == "build-deb") {
        problems.push(format!("`{DEB_TEST_JOB}` does not need `build-deb`"));
    }
    if !needs.iter().any(|n| n == "plan") {
        problems.push(format!(
            "`{DEB_TEST_JOB}` does not need `plan` (the release version)"
        ));
    }
    let content: Vec<&str> = job.content_lines().collect();
    for (ubuntu, var) in DEB_UBUNTUS {
        let entry = [
            format!("- ubuntu: \"{ubuntu}\""),
            format!("image-env: {var}"),
        ];
        if !content.windows(2).any(|w| w == entry) {
            problems.push(format!(
                "`{DEB_TEST_JOB}` matrix has no entry for ubuntu:{ubuntu} with image {var}"
            ));
        }
    }
    if content.iter().any(|l| l.starts_with("exclude:")) {
        problems.push(format!("`{DEB_TEST_JOB}` matrix excludes entries"));
    }
    let uploaded = wf.job("build-deb").is_some_and(|j| {
        j.content_lines()
            .any(|l| l == format!("name: {DEB_ARTIFACT}"))
    });
    let steps = job.steps();
    let download = steps.iter().position(|st| {
        st.id.starts_with("actions/download-artifact@")
            && st
                .content_lines()
                .any(|l| l == format!("name: {DEB_ARTIFACT}"))
            && st
                .content_lines()
                .any(|l| l == "path: ${{ runner.temp }}/deb")
    });
    if !uploaded || download.is_none() {
        problems.push(format!(
            "`{DEB_TEST_JOB}` does not download the `{DEB_ARTIFACT}` artifact `build-deb` uploads into $RUNNER_TEMP/deb"
        ));
    }
    let run = steps.iter().position(|st| {
        joined_scripts(st)
            .iter()
            .any(|s| s.contains(DEB_CONTAINER_RUN))
    });
    match run {
        None => problems.push(format!(
            "`{DEB_TEST_JOB}` does not dpkg -i the .deb and run romraider-cli --version in a fresh, offline $IMAGE container (`{DEB_CONTAINER_RUN}`)"
        )),
        Some(r) => {
            let step = &steps[r];
            let script = joined_scripts(step).join("\n");
            if !script.contains(DEB_FIND) {
                problems.push(format!(
                    "`{DEB_TEST_JOB}` does not install exactly the one downloaded package (`{DEB_FIND}`)"
                ));
            }
            if !script.contains(CONTAINER_EXPECT) {
                problems.push(format!(
                    "`{DEB_TEST_JOB}` does not check the container's output is the release version (`{CONTAINER_EXPECT}`)"
                ));
            }
            let env = step.map_at("env", 8);
            for (k, v) in [
                ("DIST_TAG", "${{ needs.plan.outputs.dist-tag }}"),
                ("IMAGE", "${{ env[matrix.image-env] }}"),
                ("UBUNTU", "${{ matrix.ubuntu }}"),
            ] {
                if !env.contains(&(k.to_owned(), v.to_owned())) {
                    problems.push(format!("`{DEB_TEST_JOB}` {k} is not `{v}`"));
                }
            }
            if download.is_none_or(|d| d > r) {
                problems.push(format!(
                    "`{DEB_TEST_JOB}` runs the container before downloading the .deb"
                ));
            }
        }
    }
    // Clean: nothing installed but the package, no dependency forced, no
    // host devices or extra mounts.
    for s in joined_scripts(job) {
        for bad in [
            "apt-get",
            "apt ",
            "--force",
            "--ignore-depends",
            "--privileged",
            "--device",
            "--cap-add",
            "docker exec",
            "docker build",
            "docker commit",
        ] {
            if s.contains(bad) {
                problems.push(format!(
                    "`{DEB_TEST_JOB}` does not test a clean container (`{bad}`)"
                ));
            }
        }
        // `docker run`'s own options: up to the container's command (the
        // script inside it has its own `-v`, `command -v`).
        let options = s
            .split_once("docker run")
            .map(|(_, rest)| rest.split(" sh -euc ").next().unwrap_or(rest));
        if options.is_some_and(|o| {
            o.matches(" -v ").count() > 1
                || o.contains("--mount")
                || o.contains("--volume")
                || o.matches("--network").count() > 1
        }) {
            problems.push(format!(
                "`{DEB_TEST_JOB}` mounts more than the .deb into the container, or networks it"
            ));
        }
    }
    if content.iter().any(|l| l.starts_with("container:")) {
        problems.push(format!(
            "`{DEB_TEST_JOB}` runs in a job container (the runner's tools are mounted in); use `docker run`"
        ));
    }
    // Not skippable.
    problems.extend(skippable_problems(job, &[]));
    for l in job.lines.iter().filter(|l| workflow::indent(l) == 4) {
        if l.trim().starts_with("if:") {
            problems.push(format!(
                "`{DEB_TEST_JOB}` has a job-level conditional (`{}`)",
                l.trim()
            ));
        }
    }
    if !wf
        .job("publish")
        .is_some_and(|j| j.needs().iter().any(|n| n == DEB_TEST_JOB))
    {
        problems.push(format!(
            "`publish` does not need `{DEB_TEST_JOB}`, so a .deb that fails to install on a clean Ubuntu still ships"
        ));
    }
    problems
}

/// US-RR-38-2: the uploaded .deb installs and runs `romraider-cli --version`
/// in clean ubuntu:22.04 and ubuntu:24.04 containers before anything is
/// published.
#[test]
fn release_config_installs_deb_on_clean_ubuntu() {
    let problems = deb_container_problems(&workflow_yml("release.yml"));
    assert!(
        problems.is_empty(),
        "release.yml violates US-RR-38-2:\n{}",
        problems.join("\n")
    );
}

/// Each clean-container .deb assertion bites on a mutated copy of
/// release.yml.
#[test]
fn deb_container_config_rejects_broken_wiring() {
    let release = workflow_yml("release.yml");
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        deb_container_problems(&changed)
    };
    // Mutations of the test job only (its header line to the next job).
    let start = release.find("\n  test-deb:\n").expect("test-deb job");
    let end = start + 1 + release[start + 1..].find("\n  publish:").expect("next job");
    let job = &release[start..end];
    let in_job = |from: &str, to: &str| {
        let changed = job.replacen(from, to, 1);
        assert_ne!(changed, job, "mutation {from:?} did not apply");
        deb_container_problems(&release.replacen(job, &changed, 1))
    };

    assert!(deb_container_problems(&release).is_empty());
    assert_flags(
        &rel(
            "UBUNTU_24_04_IMAGE: \"ubuntu:24.04@sha256:",
            "UBUNTU_24_04_IMAGE: \"ubuntu:22.04@sha256:",
        ),
        "UBUNTU_24_04_IMAGE is not ubuntu:24.04 pinned",
    );
    assert_flags(
        &rel(
            "UBUNTU_24_04_IMAGE: \"ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac\"",
            "UBUNTU_24_04_IMAGE: \"ubuntu:24.04\"",
        ),
        "UBUNTU_24_04_IMAGE is not ubuntu:24.04 pinned",
    );
    assert_flags(
        &rel(
            "UBUNTU_22_04_IMAGE: \"ubuntu:22.04@sha256:0a9887931b0e38af2e3487ed39e29436b0535c4f3f52e57745baaf79d1898956\"",
            "UBUNTU_22_04_IMAGE: \"ubuntu:22.04\"",
        ),
        "UBUNTU_22_04_IMAGE is not ubuntu:22.04 pinned",
    );
    assert_flags(
        &rel("\n  test-deb:\n", "\n  deb-smoke:\n"),
        "no `test-deb` job",
    );
    assert_flags(
        &in_job(
            "    needs: [plan, build-deb]\n",
            "    needs: [plan, build-cli]\n",
        ),
        "does not need `build-deb`",
    );
    assert_flags(
        &in_job("    needs: [plan, build-deb]\n", "    needs: [build-deb]\n"),
        "does not need `plan`",
    );
    // The oldest Ubuntu dropped from the matrix, or pointed at another image.
    assert_flags(
        &in_job(
            "          - ubuntu: \"22.04\"\n            image-env: UBUNTU_22_04_IMAGE\n",
            "",
        ),
        "no entry for ubuntu:22.04",
    );
    assert_flags(
        &in_job(
            "            image-env: UBUNTU_22_04_IMAGE\n",
            "            image-env: UBUNTU_24_04_IMAGE\n",
        ),
        "no entry for ubuntu:22.04",
    );
    assert_flags(
        &in_job(
            "          - ubuntu: \"24.04\"\n",
            "          - ubuntu: \"23.10\"\n",
        ),
        "no entry for ubuntu:24.04",
    );
    assert_flags(
        &in_job(
            "            image-env: UBUNTU_24_04_IMAGE\n",
            "            image-env: UBUNTU_24_04_IMAGE\n        exclude:\n          - ubuntu: \"22.04\"\n",
        ),
        "matrix excludes entries",
    );
    assert_flags(
        &in_job(
            "          name: release-deb-amd64\n",
            "          name: release-cli-x86_64-unknown-linux-gnu\n",
        ),
        "does not download the `release-deb-amd64` artifact",
    );
    // build-deb uploads under another name: the download gets nothing.
    assert_flags(
        &rel("name: release-deb-amd64", "name: release-deb"),
        "does not download the `release-deb-amd64` artifact",
    );
    assert_flags(
        &in_job(
            "path: ${{ runner.temp }}/deb\n",
            "path: ${{ runner.temp }}/pkg\n",
        ),
        "does not download the `release-deb-amd64` artifact",
    );
    for (from, to) in [
        ("--network none ", ""),
        ("docker run --rm ", "docker run "),
        ("/deb:ro\"", "/deb\""),
        ("\"$IMAGE\" sh -euc", "ubuntu:22.04 sh -euc"),
        (
            "dpkg -i \"/deb/$2\" >&2\n",
            "dpkg --unpack \"/deb/$2\" >&2\n",
        ),
        (
            "              . /etc/os-release\n              test \"$VERSION_ID\" = \"$1\"\n",
            "",
        ),
        (
            "              test \"$(command -v romraider-cli)\" = /usr/bin/romraider-cli\n",
            "",
        ),
        (
            "romraider-cli --version\n            '",
            "romraider-cli --help\n            '",
        ),
        // Not the last command: its exit status is no longer the container's.
        (
            "romraider-cli --version\n            '",
            "romraider-cli --version\n              true\n            '",
        ),
    ] {
        assert_flags(&in_job(from, to), "fresh, offline $IMAGE container");
    }
    // The smoke test moved onto the runner itself.
    assert_flags(
        &in_job(
            "version=$(docker run --rm --network none \\\n",
            "sudo dpkg -i \"${debs[0]}\"\n          version=$(romraider-cli --version)\n          true $(docker run --rm --network none \\\n",
        ),
        "fresh, offline $IMAGE container",
    );
    assert_flags(
        &in_job("          test \"${#debs[@]}\" -eq 1\n", ""),
        "exactly the one downloaded package",
    );
    assert_flags(
        &in_job(CONTAINER_EXPECT, "test -n \"$version\""),
        "is the release version",
    );
    assert_flags(
        &in_job(
            "DIST_TAG: ${{ needs.plan.outputs.dist-tag }}",
            "DIST_TAG: ${{ needs.plan.outputs.release-tag }}",
        ),
        "DIST_TAG is not",
    );
    assert_flags(
        &in_job(
            "IMAGE: ${{ env[matrix.image-env] }}",
            "IMAGE: ${{ env.UBUNTU_24_04_IMAGE }}",
        ),
        "IMAGE is not",
    );
    assert_flags(
        &in_job("UBUNTU: ${{ matrix.ubuntu }}", "UBUNTU: \"22.04\""),
        "UBUNTU is not",
    );
    assert_flags(
        &in_job(
            "dpkg -i \"/deb/$2\" >&2\n",
            "apt-get update >&2; apt-get install -y \"/deb/$2\" >&2\n              dpkg -i \"/deb/$2\" >&2\n",
        ),
        "not test a clean container (`apt-get`)",
    );
    assert_flags(
        &in_job(
            "dpkg -i \"/deb/$2\" >&2\n",
            "dpkg --force-depends -i \"/deb/$2\" >&2\n              dpkg -i \"/deb/$2\" >&2\n",
        ),
        "not test a clean container (`--force`)",
    );
    assert_flags(
        &in_job(
            "dpkg -i \"/deb/$2\" >&2\n",
            "dpkg --ignore-depends=libc6 -i \"/deb/$2\" >&2\n              dpkg -i \"/deb/$2\" >&2\n",
        ),
        "not test a clean container (`--ignore-depends`)",
    );
    assert_flags(
        &in_job("--network none ", "--network none --privileged "),
        "not test a clean container (`--privileged`)",
    );
    assert_flags(
        &in_job(
            "-v \"$RUNNER_TEMP/deb:/deb:ro\" ",
            "-v \"$RUNNER_TEMP/deb:/deb:ro\" -v /usr/lib:/usr/lib:ro ",
        ),
        "mounts more than the .deb",
    );
    assert_flags(
        &in_job("--network none ", "--network none --network host "),
        "mounts more than the .deb into the container, or networks it",
    );
    assert_flags(
        &in_job(
            "    runs-on: ubuntu-24.04\n",
            "    runs-on: ubuntu-24.04\n    container: ubuntu:22.04\n",
        ),
        "runs in a job container",
    );
    assert_flags(
        &in_job(
            "    timeout-minutes: 10\n",
            "    timeout-minutes: 10\n    if: needs.plan.outputs.publish == 'true'\n",
        ),
        "job-level conditional",
    );
    assert_flags(
        &in_job(
            "    timeout-minutes: 10\n",
            "    timeout-minutes: 10\n    continue-on-error: true\n",
        ),
        "uses `continue-on-error: true`",
    );
    assert_flags(
        &in_job(
            "      - name: Smoke test (dpkg -i, romraider-cli --version in a clean ubuntu container)\n",
            "      - name: Smoke test (dpkg -i, romraider-cli --version in a clean ubuntu container)\n        if: matrix.ubuntu == '24.04'\n",
        ),
        "has a conditional",
    );
    assert_flags(
        &in_job(CONTAINER_EXPECT, &format!("{CONTAINER_EXPECT} || true")),
        "swallows failures",
    );
    // The download moved after the run.
    let download = "      - uses: actions/download-artifact@v4\n        with:\n          name: release-deb-amd64\n          path: ${{ runner.temp }}/deb\n";
    assert!(job.contains(download), "download step not found verbatim");
    let moved = format!("{}{download}", job.replacen(download, "", 1));
    assert_flags(
        &deb_container_problems(&release.replacen(job, &moved, 1)),
        "runs the container before downloading",
    );
    assert_flags(
        &rel(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb]\n",
        ),
        "`publish` does not need `test-deb`",
    );
}

// --- Release artifact content check (US-RR-38-8) ------------------------------

/// The `publish` step that runs the content check, and its command.
const CONTENT_CHECK_STEP: &str = "Release artifact content check";
const CONTENT_CHECK: &str = "cargo run --locked --package xtask -- check-release-artifacts assets";

/// Every way release.yml lets `publish` upload assets the content check has
/// not passed; empty when it cannot.
fn content_check_problems(release: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);
    let Some(job) = wf.job("publish") else {
        return vec!["release.yml has no `publish` job".to_owned()];
    };
    let steps = job.steps();
    let position = |pred: &dyn Fn(&workflow::Job<'_>) -> bool| steps.iter().position(pred);
    let Some(check) = position(&|st| st.id == CONTENT_CHECK_STEP) else {
        return vec![format!("`publish` has no `{CONTENT_CHECK_STEP}` step")];
    };
    let step = &steps[check];
    let scripts = step.run_scripts();
    if scripts != [CONTENT_CHECK] {
        problems.push(format!(
            "`{CONTENT_CHECK_STEP}` runs {scripts:?}, not exactly `{CONTENT_CHECK}`"
        ));
    }
    for l in step.content_lines() {
        let l = l.trim_start_matches("- ");
        if l.starts_with("if:") || l.starts_with("continue-on-error") {
            problems.push(format!(
                "`{CONTENT_CHECK_STEP}` can be skipped or ignored (`{l}`)"
            ));
        }
    }
    let checkout = position(&|st| st.id == "actions/checkout@v5");
    if checkout.is_none_or(|c| c > check) {
        problems.push(
            "`publish` does not check out the repository (xtask) before the content check"
                .to_owned(),
        );
    }
    let download = position(&|st| st.id.starts_with("actions/download-artifact@"));
    match download {
        Some(d) if d < check => {
            if !steps[d].content_lines().any(|l| l == "path: assets") {
                problems.push(
                    "`publish` does not download the release artifacts into assets/, the directory the content check reads"
                        .to_owned(),
                );
            }
            if checkout.is_some_and(|c| c > d) {
                problems.push(
                    "`publish` checks out after downloading, which cleans assets/".to_owned(),
                );
            }
        }
        _ => problems.push(
            "`publish` does not download the release artifacts before the content check".to_owned(),
        ),
    }
    if position(&|st| st.id == "SHA256SUMS").is_none_or(|s| s > check) {
        problems.push(
            "`publish` does not write SHA256SUMS before the content check, so SHA256SUMS is not checked"
                .to_owned(),
        );
    }
    // Nothing is published before the check, and nothing after it uploads
    // from anywhere but assets/.
    for (i, st) in steps.iter().enumerate() {
        for s in st.run_scripts() {
            let publishes = s.contains("gh release") || s.contains("gh api -X");
            if publishes && i < check {
                problems.push(format!(
                    "`publish` step `{}` publishes before the content check",
                    st.id
                ));
            }
            if s.contains("gh release create") && !s.trim_end().ends_with(" assets/*") {
                problems.push(format!(
                    "`publish` step `{}` uploads something other than assets/*",
                    st.id
                ));
            }
        }
        if i < check
            && st
                .content_lines()
                .any(|l| l.trim_start_matches("- ").starts_with("uses: softprops/"))
        {
            problems.push(format!(
                "`publish` step `{}` publishes before the content check",
                st.id
            ));
        }
    }
    // US-RR-38-4: what is checked is what is published. Nothing after the
    // check can add to or change assets/: no action (a second download, an
    // uploader), no step working in assets/, and no script touching assets/
    // beyond listing it and handing `assets/*` to `gh release create`.
    for st in &steps[check + 1..] {
        if st
            .content_lines()
            .any(|l| l.trim_start_matches("- ").starts_with("uses:"))
        {
            problems.push(format!(
                "`publish` step `{}` runs an action after the content check, which could change assets/",
                st.id
            ));
        }
        if st
            .content_lines()
            .any(|l| l.trim_start_matches("- ") == "working-directory: assets")
        {
            problems.push(format!(
                "`publish` step `{}` works in assets/ after the content check",
                st.id
            ));
        }
        for s in st.run_scripts() {
            let rest = s.replace("ls -l assets", "");
            let rest = if rest.contains("gh release create") {
                rest.trim_end()
                    .strip_suffix(" assets/*")
                    .unwrap_or(&rest)
                    .to_owned()
            } else {
                rest
            };
            if rest.contains("assets") {
                problems.push(format!(
                    "`publish` step `{}` touches assets/ after the content check",
                    st.id
                ));
            }
        }
    }
    // Every release-* artifact reaches assets/ before the check: one
    // download, of every `release-*` artifact, flattened into assets/ (no
    // subdirectory the check would skip past), after every job that uploads
    // one.
    let downloads: Vec<&workflow::Job<'_>> = steps
        .iter()
        .filter(|st| st.id.starts_with("actions/download-artifact@"))
        .collect();
    if downloads.len() > 1 {
        problems.push("`publish` downloads artifacts more than once".to_owned());
    }
    if let Some(d) = downloads.first() {
        let lines: Vec<&str> = d.content_lines().collect();
        if !lines.contains(&"pattern: release-*") || lines.iter().any(|l| l.starts_with("name:")) {
            problems.push(
                "`publish` does not download every `release-*` artifact (`pattern: release-*`)"
                    .to_owned(),
            );
        }
        if !lines.contains(&"merge-multiple: true") {
            problems.push(
                "`publish` does not flatten the artifacts into assets/ (`merge-multiple: true`)"
                    .to_owned(),
            );
        }
    }
    for j in &wf.jobs {
        let uploads_release = j.steps().iter().any(|st| {
            st.id.starts_with("actions/upload-artifact@")
                && st.content_lines().any(|l| l.starts_with("name: release-"))
        });
        if uploads_release && !needs_transitively(&wf, "publish", j.id) {
            problems.push(format!(
                "job `{}` uploads a release-* artifact but `publish` does not need it, so its output may miss the content check",
                j.id
            ));
        }
    }
    problems
}

/// Whether `from` needs `target` in `wf`, directly or through other jobs.
fn needs_transitively(wf: &Workflow<'_>, from: &str, target: &str) -> bool {
    let mut seen: Vec<String> = Vec::new();
    let mut todo = vec![from.to_owned()];
    while let Some(id) = todo.pop() {
        let Some(job) = wf.job(&id) else { continue };
        for need in job.needs() {
            if need == target {
                return true;
            }
            if !seen.contains(&need) {
                seen.push(need.clone());
                todo.push(need);
            }
        }
    }
    false
}

/// US-RR-38-8: `publish` runs the content check on every asset before
/// anything is uploaded.
#[test]
fn release_config_checks_artifact_contents_before_publishing() {
    let problems = content_check_problems(&workflow_yml("release.yml"));
    assert!(
        problems.is_empty(),
        "release.yml violates US-RR-38-8:\n{}",
        problems.join("\n")
    );
}

/// Each content-check assertion bites on a mutated copy of release.yml.
#[test]
fn content_check_config_rejects_broken_wiring() {
    let release = workflow_yml("release.yml");
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        content_check_problems(&changed)
    };
    let step = "      - name: Release artifact content check\n        run: cargo run --locked --package xtask -- check-release-artifacts assets\n";
    assert!(
        release.contains(step),
        "content check step not found verbatim"
    );

    assert_flags(
        &rel(step, ""),
        "has no `Release artifact content check` step",
    );
    assert_flags(
        &rel(
            "check-release-artifacts assets\n",
            "check-release-artifacts assets || true\n",
        ),
        "not exactly",
    );
    assert_flags(
        &rel(
            "check-release-artifacts assets\n",
            "check-release-artifacts dist\n",
        ),
        "not exactly",
    );
    assert_flags(
        &rel(
            "      - name: Release artifact content check\n",
            "      - name: Release artifact content check\n        if: needs.plan.outputs.publish == 'true'\n",
        ),
        "can be skipped or ignored",
    );
    assert_flags(
        &rel(
            "      - name: Release artifact content check\n",
            "      - name: Release artifact content check\n        continue-on-error: true\n",
        ),
        "can be skipped or ignored",
    );
    // The check moved after the publish step.
    let moved = release.replacen(step, "", 1).replacen(
        "          gh release create \"$RELEASE_TAG\" \"${flags[@]}\" assets/*\n",
        &format!("          gh release create \"$RELEASE_TAG\" \"${{flags[@]}}\" assets/*\n{step}"),
        1,
    );
    assert_ne!(moved, release);
    assert_flags(
        &content_check_problems(&moved),
        "`Publish GitHub Release` publishes before the content check",
    );
    assert_flags(
        &rel(
            "    steps:\n      # The content check below is this commit's xtask. Checked out before\n      # the download, because checkout cleans the workspace.\n      - uses: actions/checkout@v5\n        with:\n          persist-credentials: false\n      - uses: actions/download-artifact@v4\n",
            "    steps:\n      - uses: actions/download-artifact@v4\n",
        ),
        "does not check out the repository",
    );
    assert_flags(
        &rel(
            "          pattern: release-*\n          path: assets\n",
            "          pattern: release-*\n          path: dist\n",
        ),
        "does not download the release artifacts into assets/",
    );
    assert_flags(
        &rel(
            "gh release create \"$RELEASE_TAG\" \"${flags[@]}\" assets/*",
            "gh release create \"$RELEASE_TAG\" \"${flags[@]}\" assets/* \"$RUNNER_TEMP\"/*",
        ),
        "uploads something other than assets/*",
    );
    assert_flags(
        &rel("      - name: SHA256SUMS\n", "      - name: Checksums\n"),
        "does not write SHA256SUMS before the content check",
    );
}

/// US-RR-38-4: each "what is checked is what is published" assertion bites
/// on a mutated copy of release.yml: nothing added to assets/ after the
/// check, and every release-* artifact downloaded into it before.
#[test]
fn content_check_config_rejects_unchecked_assets() {
    let release = workflow_yml("release.yml");
    let rel = |from: &str, to: &str| {
        let changed = release.replacen(from, to, 1);
        assert_ne!(changed, release, "mutation {from:?} did not apply");
        content_check_problems(&changed)
    };
    let after_check = "check-release-artifacts assets\n";
    assert_flags(
        &rel(
            after_check,
            "check-release-artifacts assets\n      - uses: actions/download-artifact@v4\n        with:\n          name: extra\n          path: assets\n",
        ),
        "runs an action after the content check",
    );
    assert_flags(
        &rel(
            after_check,
            "check-release-artifacts assets\n      - name: Extras\n        working-directory: assets\n        run: echo\n",
        ),
        "works in assets/ after the content check",
    );
    assert_flags(
        &rel(
            after_check,
            "check-release-artifacts assets\n      - name: Extras\n        run: cp \"$RUNNER_TEMP/stock.bin\" assets/\n",
        ),
        "`Extras` touches assets/ after the content check",
    );
    assert_flags(
        &rel(
            "          ls -l assets\n          flags=(",
            "          cp \"$RUNNER_TEMP/stock.bin\" assets/\n          ls -l assets\n          flags=(",
        ),
        "`Publish GitHub Release` touches assets/ after the content check",
    );
    assert_flags(
        &rel(
            "          pattern: release-*\n",
            "          pattern: release-cli-*\n",
        ),
        "does not download every `release-*` artifact",
    );
    assert_flags(
        &rel("          merge-multiple: true\n", ""),
        "does not flatten the artifacts",
    );
    assert_flags(
        &rel(
            "          merge-multiple: true\n",
            "          merge-multiple: true\n      - uses: actions/download-artifact@v4\n        with:\n          path: assets\n",
        ),
        "downloads artifacts more than once",
    );
    // A job uploading a release-* artifact that `publish` does not wait
    // for (test-deb needs build-deb, so drop both edges).
    assert_flags(
        &rel(
            "    needs: [plan, build-cli, build-appimage, test-appimage, build-deb, test-deb]\n",
            "    needs: [plan, build-cli, build-appimage, test-appimage]\n",
        ),
        "job `build-deb` uploads a release-* artifact but `publish` does not need it",
    );
}

// --- The nightly publish, executed (US-RR-38-1) -------------------------------

/// The `release-*` artifacts, by the job that uploads each: what `publish`
/// downloads into assets/, so everything a release attaches besides
/// SHA256SUMS: the tarball, the AppImage and the .deb.
const RELEASE_UPLOADS: [(&str, &str); 3] = [
    ("build-cli", "release-cli-${{ matrix.target }}"),
    ("build-appimage", "release-appimage-x86_64"),
    ("build-deb", "release-deb-amd64"),
];

/// Every way the `release-*` artifacts release.yml uploads differ from
/// [`RELEASE_UPLOADS`] (one tarball target, each artifact the job's
/// `release/` directory); empty when they match.
fn release_upload_problems(release: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let wf = Workflow::parse(release);
    let mut found: Vec<(String, String)> = Vec::new();
    for j in &wf.jobs {
        for st in j.steps() {
            if !st.id.starts_with("actions/upload-artifact@") {
                continue;
            }
            let Some(name) = st
                .value_at("name", 10)
                .filter(|n| n.starts_with("release-"))
            else {
                continue;
            };
            if st.value_at("path", 10).as_deref() != Some("${{ runner.temp }}/release/*") {
                problems.push(format!(
                    "job `{}` uploads `{name}` from somewhere other than its release/ directory",
                    j.id
                ));
            }
            found.push((j.id.to_owned(), name));
        }
    }
    let mut want: Vec<(String, String)> = RELEASE_UPLOADS
        .iter()
        .map(|(j, n)| ((*j).to_owned(), (*n).to_owned()))
        .collect();
    found.sort();
    want.sort();
    if found != want {
        problems.push(format!(
            "the release-* artifacts are {found:?}, not exactly the tarball, AppImage and .deb {want:?}"
        ));
    }
    let targets = wf.job("build-cli").map_or(0, |j| {
        j.content_lines()
            .filter(|l| l.starts_with("- target:"))
            .count()
    });
    if targets != 1 {
        problems.push(format!(
            "`build-cli` builds {targets} targets, so a release would not attach exactly one tarball"
        ));
    }
    problems
}

/// US-RR-38-1: the only `release-*` artifacts are the tarball, AppImage and
/// .deb (on every platform; the executed check below is Unix only).
#[test]
fn release_uploads_are_tarball_appimage_and_deb() {
    let problems = release_upload_problems(&workflow_yml("release.yml"));
    assert!(
        problems.is_empty(),
        "release.yml violates US-RR-38-1:\n{}",
        problems.join("\n")
    );
}

/// The publish path run for real, under bash with stub `gh` and `dist`.
/// Unix only: it needs bash, sha256sum and jq, as the ubuntu runners have.
#[cfg(unix)]
mod nightly_publish {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use xtask::gen_synthetic::sha256_hex;

    use super::workflow::{self, Workflow};
    use super::{assert_flags, release_upload_problems, workflow_yml};

    /// The commit being released, and an older one the nightly was at.
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const OLD_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const REPO: &str = "Biztactix-Ryan/RomRaider";
    const DEFAULT_BRANCH: &str = "rust-rewrite";
    const NIGHTLY: &str = "nightly";
    /// What the stub `dist plan` announces.
    const VERSION_TAG: &str = "v0.1.0";
    /// One file per `release-*` artifact, named as the build jobs name them
    /// (`build-cli` `Stage release asset`, `APPIMAGE`, build.sh's .deb).
    const BUILT: [&str; 3] = [
        "romraider-cli-x86_64-unknown-linux-gnu.tar.gz",
        "romraider-cli-x86_64.AppImage",
        "romraider-cli_0.1.0-1_amd64.deb",
    ];

    /// Stub `gh`: logs each call to `$SIM_DIR/gh.log` (one line, arguments
    /// separated by 0x1f) and answers as GitHub would for a repository whose
    /// `nightly` tag and release exist iff `$SIM_DIR/tag` / `$SIM_DIR/release`
    /// do. Any other call fails, so the script cannot do anything unseen.
    const FAKE_GH: &str = r#"#!/bin/sh
log="$SIM_DIR/gh.log"
for a in "$@"; do printf '%s\037' "$a" >> "$log"; done
printf '\n' >> "$log"
ref="repos/$SIM_REPO/git/ref/tags/nightly"
case "$1 $2" in
  "api $ref")
    [ -f "$SIM_DIR/tag" ] || { echo 'gh: Not Found (HTTP 404)' >&2; exit 1; }
    if [ "$3" = --jq ]; then cat "$SIM_DIR/tag"; fi
    exit 0 ;;
  "api -X")
    if [ "$3 $4" = "DELETE repos/$SIM_REPO/git/refs/tags/nightly" ] && [ -f "$SIM_DIR/tag" ]; then
      rm -f "$SIM_DIR/tag"; exit 0
    fi ;;
  "release view")
    if [ "$3" = nightly ] && [ -f "$SIM_DIR/release" ]; then exit 0; fi
    echo 'release not found' >&2; exit 1 ;;
  "release delete")
    if [ "$3" = nightly ] && [ -f "$SIM_DIR/release" ]; then
      rm -f "$SIM_DIR/release"
      case " $* " in *" --cleanup-tag "*) rm -f "$SIM_DIR/tag" ;; esac
      exit 0
    fi ;;
  "release create")
    if [ "$3" = nightly ] && [ -f "$SIM_DIR/release" ]; then
      echo 'fake gh: release nightly already exists' >&2; exit 1
    fi
    if [ "$3" = nightly ] && [ -f "$SIM_DIR/tag" ]; then
      echo 'fake gh: tag nightly still exists, so the release would not move to the new commit' >&2; exit 1
    fi
    exit 0 ;;
esac
echo "fake gh: unexpected call: $*" >&2
exit 2
"#;

    /// One run of release.yml on the nightly channel.
    struct Scenario {
        label: &'static str,
        event: &'static str,
        /// The commit `nightly` (tag and release) is at before the run.
        nightly_at: Option<&'static str>,
    }

    const SCENARIOS: [Scenario; 4] = [
        Scenario {
            label: "first scheduled nightly",
            event: "schedule",
            nightly_at: None,
        },
        Scenario {
            label: "scheduled nightly at a new commit",
            event: "schedule",
            nightly_at: Some(OLD_SHA),
        },
        Scenario {
            label: "manual run from the default branch",
            event: "workflow_dispatch",
            nightly_at: Some(OLD_SHA),
        },
        Scenario {
            label: "scheduled nightly, commit already published",
            event: "schedule",
            nightly_at: Some(SHA),
        },
    ];

    /// What a simulated run did.
    struct Sim {
        /// `plan`'s `$GITHUB_OUTPUT`.
        plan: BTreeMap<String, String>,
        /// Every `gh` call, in order, as argv without `gh`.
        gh: Vec<Vec<String>>,
        /// Whether the `Publish GitHub Release` step ran.
        published: bool,
        /// assets/SHA256SUMS as written by the `SHA256SUMS` step.
        sums: String,
        /// SHA-256 of each built file, by name.
        digests: BTreeMap<String, String>,
        /// The release notes file the create call names, if it exists.
        notes: Option<String>,
    }

    /// An empty directory for one simulation, unique per process and thread
    /// (simulations on one thread run one after another).
    fn fresh_dir() -> Result<PathBuf, String> {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("nightly-publish")
            .join(format!(
                "{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).map_err(|e| format!("create sim dir: {e}"))?;
        Ok(dir)
    }

    fn write_exe(path: &Path, body: &str) -> Result<(), String> {
        fs::write(path, body).map_err(|e| format!("write stub: {e}"))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod stub: {e}"))
    }

    /// Resolves a workflow `${{ ... }}` value for this run; `Err` on any
    /// expression the simulation does not model.
    fn resolve(
        value: &str,
        event: &str,
        plan: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        let Some(expr) = value
            .strip_prefix("${{")
            .and_then(|v| v.strip_suffix("}}"))
            .map(str::trim)
        else {
            return Ok(value.to_owned());
        };
        if let Some(out) = expr.strip_prefix("needs.plan.outputs.") {
            return plan
                .get(out)
                .cloned()
                .ok_or_else(|| format!("`plan` has no output `{out}`"));
        }
        let v = match expr {
            "github.event_name" => event,
            "github.ref" => "refs/heads/rust-rewrite",
            "github.ref_name" | "github.event.repository.default_branch" => DEFAULT_BRANCH,
            "inputs.dry-run || false" => "false",
            "github.sha" => SHA,
            "github.token" => "fake-token",
            "github.repository" => REPO,
            _ => return Err(format!("unmodelled expression `{value}`")),
        };
        Ok(v.to_owned())
    }

    /// Runs `script` as GitHub's `shell: bash` does, in a clean environment.
    fn run_bash(
        dir: &Path,
        name: &str,
        script: &str,
        cwd: &Path,
        env: &[(String, String)],
    ) -> Result<(), String> {
        let file = dir.join(format!("{name}.sh"));
        fs::write(&file, script).map_err(|e| e.to_string())?;
        let path = format!(
            "{}:{}",
            dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out = Command::new("bash")
            .args(["--noprofile", "--norc", "-eo", "pipefail"])
            .arg(&file)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", path)
            .env("HOME", dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .output()
            .map_err(|e| format!("cannot run bash: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "step `{name}` failed ({}):\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }

    /// Runs `plan`, then `publish`'s `SHA256SUMS` and (if its `if:` holds)
    /// `Publish GitHub Release` steps of `release` for `sc`, with the three
    /// [`BUILT`] files downloaded into assets/.
    fn simulate(release: &str, sc: &Scenario) -> Result<Sim, String> {
        let wf = Workflow::parse(release);
        let dir = fresh_dir()?;
        write_exe(&dir.join("bin/gh"), FAKE_GH)?;
        write_exe(
            &dir.join("bin/dist"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'dist (stub)' ;;\n  plan) echo '{{\"announcement_tag\":\"{VERSION_TAG}\",\"announcement_is_prerelease\":false}}' ;;\n  *) echo \"stub dist: unexpected call: $*\" >&2; exit 2 ;;\nesac\n"
            ),
        )?;
        if let Some(at) = sc.nightly_at {
            fs::write(dir.join("tag"), format!("{at}\n")).map_err(|e| e.to_string())?;
            fs::write(dir.join("release"), "").map_err(|e| e.to_string())?;
        }
        let runner_temp = dir.join("runner");
        fs::create_dir_all(&runner_temp).map_err(|e| e.to_string())?;
        let output = runner_temp.join("github_output");
        let mut base: Vec<(String, String)> = wf
            .section("env")
            .iter()
            .filter(|l| workflow::indent(l) == 2 && !workflow::is_blank_or_comment(l))
            .filter_map(|l| l.trim().split_once(':'))
            .map(|(k, v)| {
                (
                    k.trim().to_owned(),
                    v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned(),
                )
            })
            .collect();
        for (k, v) in [
            ("SIM_DIR", dir.display().to_string()),
            ("SIM_REPO", REPO.to_owned()),
            ("GITHUB_REPOSITORY", REPO.to_owned()),
            ("GITHUB_OUTPUT", output.display().to_string()),
            ("GITHUB_WORKSPACE", dir.display().to_string()),
            ("RUNNER_TEMP", runner_temp.display().to_string()),
        ] {
            base.push((k.to_owned(), v));
        }
        let env_of = |job: &workflow::Job<'_>,
                      at: usize,
                      plan: &BTreeMap<String, String>,
                      into: &mut Vec<(String, String)>|
         -> Result<(), String> {
            for (k, v) in job.map_at("env", at) {
                into.push((k, resolve(&v, sc.event, plan)?));
            }
            Ok(())
        };

        // plan
        let no_plan = BTreeMap::new();
        let plan_job = wf.job("plan").ok_or("release.yml has no `plan` job")?;
        let mut env = base.clone();
        env_of(plan_job, 4, &no_plan, &mut env)?;
        let plan_step = plan_job
            .steps()
            .into_iter()
            .find(|s| s.value_at("id", 8).as_deref() == Some("plan"))
            .ok_or("`plan` has no step `id: plan`")?;
        env_of(&plan_step, 8, &no_plan, &mut env)?;
        let script = plan_step.run_block().ok_or("`plan` step has no `run: |`")?;
        run_bash(&dir, "plan", &script, &dir, &env)?;
        let plan: BTreeMap<String, String> = fs::read_to_string(&output)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();

        let mut sums = String::new();
        let mut digests = BTreeMap::new();
        let mut published = false;
        if plan.get("build").map(String::as_str) == Some("true") {
            // The download: one file per release-* artifact, in assets/.
            let assets = dir.join("assets");
            fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
            for name in BUILT {
                let bytes = format!("stand-in for {name}\n");
                fs::write(assets.join(name), &bytes).map_err(|e| e.to_string())?;
                digests.insert(name.to_owned(), sha256_hex(bytes.as_bytes()));
            }
            let publish = wf
                .job("publish")
                .ok_or("release.yml has no `publish` job")?;
            let mut job_env = base.clone();
            env_of(publish, 4, &plan, &mut job_env)?;
            let steps = publish.steps();
            let step = |id: &str| {
                steps
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or(format!("`publish` has no `{id}` step"))
            };

            let sums_step = step("SHA256SUMS")?;
            let cwd = dir.join(
                sums_step
                    .value_at("working-directory", 8)
                    .unwrap_or_default(),
            );
            let script = sums_step
                .run_block()
                .ok_or("`SHA256SUMS` has no `run: |`")?;
            run_bash(&dir, "sha256sums", &script, &cwd, &job_env)?;
            sums = fs::read_to_string(assets.join("SHA256SUMS")).unwrap_or_default();

            let pub_step = step("Publish GitHub Release")?;
            // The step's `if:` verbatim (value_at would strip the quotes).
            let cond = pub_step
                .content_lines()
                .find_map(|l| l.strip_prefix("if:"))
                .map(str::trim);
            if cond != Some("needs.plan.outputs.publish == 'true'") {
                return Err(format!(
                    "`Publish GitHub Release` runs `if: {cond:?}`, not on plan's `publish`, so the nightly never publishes"
                ));
            }
            if plan.get("publish").map(String::as_str) == Some("true") {
                let mut env = job_env.clone();
                env_of(pub_step, 8, &plan, &mut env)?;
                let script = pub_step
                    .run_block()
                    .ok_or("`Publish GitHub Release` has no `run: |`")?;
                run_bash(&dir, "publish", &script, &dir, &env)?;
                published = true;
            }
        }

        let gh: Vec<Vec<String>> = fs::read_to_string(dir.join("gh.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| {
                l.split('\x1f')
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .collect();
        let notes = gh
            .iter()
            .find(|c| c.starts_with(&["release".to_owned(), "create".to_owned()]))
            .and_then(|c| c.iter().skip_while(|a| *a != "--notes-file").nth(1))
            .and_then(|p| fs::read_to_string(p).ok());
        let _ = fs::remove_dir_all(&dir);
        Ok(Sim {
            plan,
            gh,
            published,
            sums,
            digests,
            notes,
        })
    }

    /// `gh release create <tag>`'s flags (with their values) and positional
    /// arguments (the files it attaches).
    fn parse_create(args: &[String]) -> (BTreeMap<String, String>, Vec<String>) {
        let mut flags = BTreeMap::new();
        let mut files = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if matches!(
                a.as_str(),
                "--title" | "--notes-file" | "--target" | "--notes"
            ) {
                flags.insert(a.clone(), it.next().cloned().unwrap_or_default());
            } else if a.starts_with('-') {
                flags.insert(a.clone(), String::new());
            } else {
                files.push(a.clone());
            }
        }
        files.sort();
        (flags, files)
    }

    /// Every way release.yml's nightly fails to publish a pre-release of
    /// exactly the tarball, AppImage, .deb and SHA256SUMS over them; empty
    /// when it does in every [`SCENARIOS`] run.
    pub(super) fn nightly_publish_problems(release: &str) -> Vec<String> {
        let mut problems = release_upload_problems(release);
        for sc in &SCENARIOS {
            let label = sc.label;
            let sim = match simulate(release, sc) {
                Ok(sim) => sim,
                Err(e) => {
                    problems.push(format!("{label}: {e}"));
                    continue;
                }
            };
            let creates: Vec<usize> = (0..sim.gh.len())
                .filter(|&i| {
                    sim.gh[i].get(..2) == Some(&["release".to_owned(), "create".to_owned()])
                })
                .collect();
            if sc.nightly_at == Some(SHA) {
                if sim.plan.get("build").map(String::as_str) != Some("false")
                    || sim.published
                    || !creates.is_empty()
                {
                    problems.push(format!(
                        "{label}: rebuilds or republishes a commit that already is `nightly` (plan {:?})",
                        sim.plan
                    ));
                }
                continue;
            }
            let out = |k: &str| sim.plan.get(k).map(String::as_str);
            for (k, want) in [
                ("build", "true"),
                ("publish", "true"),
                ("channel", "nightly"),
                ("release-tag", NIGHTLY),
            ] {
                if out(k) != Some(want) {
                    problems.push(format!(
                        "{label}: plan output `{k}` is {:?}, not {want:?}",
                        out(k)
                    ));
                }
            }
            if out("prerelease") != Some("true") {
                problems.push(format!(
                    "{label}: plan output `prerelease` is {:?}, so the nightly is not a pre-release",
                    out("prerelease")
                ));
            }
            let [create] = creates[..] else {
                problems.push(format!(
                    "{label}: `gh release create` ran {} times, not once (gh calls {:?})",
                    creates.len(),
                    sim.gh
                ));
                continue;
            };
            if create + 1 != sim.gh.len() {
                problems.push(format!(
                    "{label}: `gh` is called after `gh release create`: {:?}",
                    &sim.gh[create + 1..]
                ));
            }
            let call = &sim.gh[create];
            if call.get(2).map(String::as_str) != Some(NIGHTLY) {
                problems.push(format!(
                    "{label}: creates release {:?}, not `{NIGHTLY}`",
                    call.get(2)
                ));
            }
            let (flags, files) = parse_create(call.get(3..).unwrap_or_default());
            if !flags.contains_key("--prerelease") {
                problems.push(format!(
                    "{label}: `gh release create` lacks --prerelease, so the nightly is not a pre-release ({call:?})"
                ));
            }
            for bad in ["--draft", "--verify-tag", "--latest"] {
                if flags.contains_key(bad) {
                    problems.push(format!("{label}: `gh release create` passes {bad}"));
                }
            }
            if flags.get("--target").map(String::as_str) != Some(SHA) {
                problems.push(format!(
                    "{label}: `gh release create` --target is {:?}, so it does not pin the nightly to the built commit",
                    flags.get("--target")
                ));
            }
            if !flags
                .get("--title")
                .is_some_and(|t| t.starts_with("Nightly ") && t.contains(&SHA[..12]))
            {
                problems.push(format!(
                    "{label}: title {:?} does not name the nightly and its commit",
                    flags.get("--title")
                ));
            }
            if !sim
                .notes
                .as_deref()
                .is_some_and(|n| n.contains(SHA) && n.contains("sha256sum -c SHA256SUMS"))
            {
                problems.push(format!(
                    "{label}: release notes {:?} do not name the commit and how to verify SHA256SUMS",
                    sim.notes
                ));
            }
            let mut want: Vec<String> = BUILT
                .iter()
                .chain(&["SHA256SUMS"])
                .map(|n| format!("assets/{n}"))
                .collect();
            want.sort();
            if files != want {
                problems.push(format!(
                    "{label}: the release attaches {files:?}, not exactly the tarball, AppImage, .deb and SHA256SUMS {want:?}"
                ));
            }
            let mut covered: Vec<(String, String)> = sim
                .sums
                .lines()
                .filter_map(|l| l.split_once("  "))
                .map(|(h, n)| (n.to_owned(), h.to_owned()))
                .collect();
            covered.sort();
            let want_sums: Vec<(String, String)> = sim
                .digests
                .iter()
                .map(|(n, h)| (n.clone(), h.clone()))
                .collect();
            if covered != want_sums || sim.sums.lines().count() != BUILT.len() {
                problems.push(format!(
                    "{label}: SHA256SUMS covers {covered:?}, not exactly the tarball, AppImage and .deb with their digests {want_sums:?}"
                ));
            }
            if sc.nightly_at.is_some() {
                let deleted = sim.gh[..create].iter().any(|c| {
                    c.get(..3)
                        == Some(&[
                            "release".to_owned(),
                            "delete".to_owned(),
                            NIGHTLY.to_owned(),
                        ])
                });
                if !deleted {
                    problems.push(format!(
                        "{label}: the previous nightly release is not deleted before the new one is created"
                    ));
                }
            }
        }
        problems
    }

    /// US-RR-38-1: the nightly publishes a pre-release with exactly the
    /// tarball, AppImage, .deb and SHA256SUMS over them, scheduled or
    /// manual, first or moving; an unchanged commit publishes nothing.
    #[test]
    fn nightly_publishes_prerelease_with_tarball_appimage_deb_and_sha256sums() {
        let problems = nightly_publish_problems(&workflow_yml("release.yml"));
        assert!(
            problems.is_empty(),
            "release.yml's nightly violates US-RR-38-1:\n{}",
            problems.join("\n")
        );
    }

    /// Each nightly-publish assertion bites on a mutated copy of release.yml.
    #[test]
    fn nightly_publish_rejects_broken_wiring() {
        let release = workflow_yml("release.yml");
        let rel = |from: &str, to: &str| {
            let changed = release.replacen(from, to, 1);
            assert_ne!(changed, release, "mutation {from:?} did not apply");
            nightly_publish_problems(&changed)
        };
        assert_flags(
            &rel("            flags+=(--prerelease)\n", "            :\n"),
            "lacks --prerelease",
        );
        assert_flags(
            &rel(
                "            prerelease=true\n",
                "            prerelease=false\n",
            ),
            "plan output `prerelease` is Some(\"false\")",
        );
        assert_flags(
            &rel(
                "              release_tag=\"$NIGHTLY_TAG\"\n",
                "              release_tag=latest\n",
            ),
            "plan output `release-tag`",
        );
        assert_flags(
            &rel(
                "\"${flags[@]}\" assets/*\n",
                "\"${flags[@]}\" assets/*.deb\n",
            ),
            "the release attaches",
        );
        assert_flags(
            &rel(
                "sha256sum -- \"${files[@]}\" > SHA256SUMS\n",
                "sha256sum -- \"${files[@]:1}\" > SHA256SUMS\n",
            ),
            "SHA256SUMS covers",
        );
        assert_flags(
            &rel(
                "              gh release delete \"$RELEASE_TAG\" --cleanup-tag --yes\n",
                "              :\n",
            ),
            "release nightly already exists",
        );
        assert_flags(
            &rel(
                "            flags+=(--target \"$SHA\")\n",
                "            :\n",
            ),
            "does not pin the nightly to the built commit",
        );
        assert_flags(
            &rel(
                "                  build=false\n                  publish=false\n",
                "                  :\n",
            ),
            "republishes a commit that already is `nightly`",
        );
        assert_flags(
            &rel(
                "      - name: Publish GitHub Release\n        if: needs.plan.outputs.publish == 'true'\n",
                "      - name: Publish GitHub Release\n        if: github.event_name == 'push'\n",
            ),
            "the nightly never publishes",
        );
        assert_flags(
            &rel("name: release-deb-amd64\n", "name: deb-amd64\n"),
            "not exactly the tarball, AppImage and .deb",
        );
        assert_flags(
            &rel(
                "          - target: x86_64-unknown-linux-gnu\n",
                "          - target: x86_64-unknown-linux-gnu\n          - target: aarch64-unknown-linux-gnu\n",
            ),
            "`build-cli` builds 2 targets",
        );
    }
}
