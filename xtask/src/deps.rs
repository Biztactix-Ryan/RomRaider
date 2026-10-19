//! Workspace dependency-rule checker (ARCHITECTURE.md "Crate Map" / "Dependency rule").
//!
//! The rule, as encoded here:
//!
//! 1. `rr-core` depends on nothing in the workspace: no normal or build edge to any
//!    workspace crate. Its only permitted workspace dev-dependency is the dev-only
//!    test harness (`rr-testkit`).
//! 2. Only `rr-link`, `rr-cli` and `rr-app` touch the OS: no other crate may reach a
//!    hardware crate (`serialport`, `libloading`, `nusb`, `winreg`) directly or
//!    transitively over normal/build edges. Dev edges are ignored. The walk does not
//!    descend into the sanctioned crates themselves, so `rr-comms -> rr-link ->
//!    serialport` is allowed (that is the Crate Map's intended layering) while
//!    `rr-rom -> some-crate -> serialport` is a violation.
//! 3. `rr-testkit` is dev-dependency only: no crate may take a normal or build edge
//!    to it.
//! 4. `rr-core` performs no I/O: besides rule 2's hardware crates, it may not reach
//!    any crate in [`Rules::core_forbidden_io`] (filesystem, directory, watcher,
//!    temp-file, mmap and async-runtime / socket crates) over normal/build edges,
//!    directly or transitively.
//!
//! What "OS/IO crates" means, and the limit of a metadata check: `cargo metadata`
//! sees crate *names*, not what code inside a crate calls. So for `rr-core` the
//! forbidden set is the hardware list (rule 2) plus the named I/O list (rule 4). A
//! third-party crate that quietly calls `std::fs` internally, or `std::fs` used in
//! `rr-core`'s own source, is not visible to this checker; that is caught by code
//! review of `rr-core`'s (currently empty) `[dependencies]` table, not here.
//!
//! Every allowlist lives in [`RULES`] and nowhere else. The checker runs over a
//! small internal graph model ([`DepGraph`]) so it can be unit-tested against
//! synthetic graphs; [`DepGraph::from_metadata`] builds that model from real
//! `cargo metadata` output.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::Path;

use anyhow::{Context as _, Result};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};

/// The single allowlist table for the workspace dependency rule.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// The pure crate that must not depend on any workspace crate.
    pub core_crate: &'static str,
    /// Workspace crates `core_crate` may use as dev-dependencies (tests only).
    pub core_dev_allowed: &'static [&'static str],
    /// Crates that talk to hardware / the OS device layer.
    pub hardware_crates: &'static [&'static str],
    /// Workspace crates allowed to depend on `hardware_crates`.
    pub hardware_allowed: &'static [&'static str],
    /// Workspace crates that may only ever be dev-dependencies.
    pub dev_only_crates: &'static [&'static str],
    /// Non-hardware OS/IO crates `core_crate` must not reach over non-dev edges
    /// (see the module docs for what a metadata check can and cannot see).
    pub core_forbidden_io: &'static [&'static str],
}

/// THE allowlist. Edit here, and only here, when the Crate Map changes.
pub const RULES: Rules = Rules {
    core_crate: "rr-core",
    core_dev_allowed: &["rr-testkit"],
    hardware_crates: &["serialport", "libloading", "nusb", "winreg"],
    hardware_allowed: &["rr-link", "rr-cli", "rr-app"],
    dev_only_crates: &["rr-testkit"],
    core_forbidden_io: &[
        // filesystem / directories
        "directories",
        "dirs",
        "walkdir",
        "notify",
        "tempfile",
        "fs-err",
        "fs_extra",
        "memmap2",
        // async runtimes / sockets / OS event loops
        "tokio",
        "async-std",
        "mio",
        "socket2",
    ],
};

/// Kind of a dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    /// `[dependencies]`
    Normal,
    /// `[build-dependencies]`
    Build,
    /// `[dev-dependencies]`
    Dev,
}

impl EdgeKind {
    /// Whether this edge ships with (or builds) the crate, i.e. is not test-only.
    #[must_use]
    pub fn is_non_dev(self) -> bool {
        !matches!(self, Self::Dev)
    }
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "normal",
            Self::Build => "build",
            Self::Dev => "dev",
        })
    }
}

#[derive(Debug, Clone)]
struct Pkg {
    name: String,
    workspace: bool,
    /// target package id -> edge kinds
    deps: BTreeMap<String, BTreeSet<EdgeKind>>,
}

/// Minimal resolved dependency graph: packages keyed by an opaque id, matched by name.
#[derive(Debug, Clone, Default)]
pub struct DepGraph {
    pkgs: BTreeMap<String, Pkg>,
}

impl DepGraph {
    /// Empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) a package. `id` is opaque and unique; `name` is the crate name.
    pub fn add_package(&mut self, id: &str, name: &str, workspace: bool) {
        self.pkgs.insert(
            id.to_owned(),
            Pkg {
                name: name.to_owned(),
                workspace,
                deps: BTreeMap::new(),
            },
        );
    }

    /// Add an edge `from -> to` of `kind`. Unknown ids are ignored by the checker.
    pub fn add_edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        if let Some(p) = self.pkgs.get_mut(from) {
            p.deps.entry(to.to_owned()).or_default().insert(kind);
        }
    }

    /// Look up the id of the workspace package called `name`.
    #[must_use]
    pub fn workspace_id(&self, name: &str) -> Option<&str> {
        self.pkgs
            .iter()
            .find(|(_, p)| p.workspace && p.name == name)
            .map(|(id, _)| id.as_str())
    }

    /// Names of all workspace packages, sorted.
    #[must_use]
    pub fn workspace_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .pkgs
            .values()
            .filter(|p| p.workspace)
            .map(|p| p.name.as_str())
            .collect();
        names.sort_unstable();
        names
    }

    /// Build the model from `cargo metadata` output (requires the resolve graph).
    pub fn from_metadata(meta: &Metadata) -> Result<Self> {
        let resolve = meta
            .resolve
            .as_ref()
            .context("cargo metadata has no resolve graph (was --no-deps used?)")?;
        let members: BTreeSet<_> = meta.workspace_members.iter().collect();
        let mut g = Self::new();
        for p in &meta.packages {
            g.add_package(&p.id.repr, p.name.as_ref(), members.contains(&p.id));
        }
        for node in &resolve.nodes {
            for dep in &node.deps {
                for k in &dep.dep_kinds {
                    let kind = match k.kind {
                        DependencyKind::Development => EdgeKind::Dev,
                        DependencyKind::Build => EdgeKind::Build,
                        // Unknown future kinds are treated as shipping edges (fail closed).
                        DependencyKind::Normal | DependencyKind::Unknown => EdgeKind::Normal,
                    };
                    g.add_edge(&node.id.repr, &dep.pkg.repr, kind);
                }
            }
        }
        Ok(g)
    }

    /// Run `cargo metadata` for the workspace at `manifest_path` and build the model.
    pub fn load(manifest_path: &Path) -> Result<Self> {
        let meta = MetadataCommand::new()
            .manifest_path(manifest_path)
            .exec()
            .with_context(|| format!("running cargo metadata on {}", manifest_path.display()))?;
        Self::from_metadata(&meta)
    }

    fn name<'a>(&'a self, id: &'a str) -> &'a str {
        self.pkgs.get(id).map_or(id, |p| p.name.as_str())
    }
}

/// One broken rule, naming the offending crate and edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// `RULES.core_crate` has a forbidden edge to a workspace crate.
    CoreDependsOnWorkspace {
        core: String,
        dep: String,
        kind: EdgeKind,
    },
    /// A non-allowlisted crate reaches a hardware crate over non-dev edges.
    HardwareReachable {
        from: String,
        hardware: String,
        path: Vec<String>,
    },
    /// `RULES.core_crate` reaches a forbidden OS/IO crate over non-dev edges.
    CoreReachesIo {
        core: String,
        io: String,
        path: Vec<String>,
    },
    /// A dev-only crate is used as a normal/build dependency.
    DevOnlyAsNonDev {
        from: String,
        dep: String,
        kind: EdgeKind,
    },
    /// A workspace crate named in [`RULES`] does not exist (stale allowlist).
    UnknownRuleCrate { name: String },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreDependsOnWorkspace { core, dep, kind } => write!(
                f,
                "{core} must not depend on workspace crate {dep}: edge `{core} -> {dep}` ({kind})"
            ),
            Self::HardwareReachable {
                from,
                hardware,
                path,
            } => write!(
                f,
                "{from} reaches hardware crate {hardware} via non-dev edges `{}`; only {} may depend on hardware crates",
                path.join(" -> "),
                RULES.hardware_allowed.join(", ")
            ),
            Self::CoreReachesIo { core, io, path } => write!(
                f,
                "{core} must perform no I/O, but reaches OS/IO crate {io} via non-dev edges `{}`",
                path.join(" -> ")
            ),
            Self::DevOnlyAsNonDev { from, dep, kind } => write!(
                f,
                "{dep} is dev-dependency only, but {from} uses it as a {kind} dependency: edge `{from} -> {dep}` ({kind})"
            ),
            Self::UnknownRuleCrate { name } => {
                write!(
                    f,
                    "dependency rules name workspace crate {name}, which is not in the workspace"
                )
            }
        }
    }
}

/// Check `graph` against [`RULES`]. Empty result means the workspace complies.
#[must_use]
pub fn check(graph: &DepGraph) -> Vec<Violation> {
    check_with(graph, &RULES)
}

/// Check `graph` against an explicit rule table.
#[must_use]
pub fn check_with(graph: &DepGraph, rules: &Rules) -> Vec<Violation> {
    let mut out = Vec::new();
    check_rule_names(graph, rules, &mut out);
    check_core(graph, rules, &mut out);
    check_core_io(graph, rules, &mut out);
    check_hardware(graph, rules, &mut out);
    check_dev_only(graph, rules, &mut out);
    out
}

fn check_rule_names(graph: &DepGraph, rules: &Rules, out: &mut Vec<Violation>) {
    let named = std::iter::once(rules.core_crate)
        .chain(rules.core_dev_allowed.iter().copied())
        .chain(rules.hardware_allowed.iter().copied())
        .chain(rules.dev_only_crates.iter().copied());
    let mut seen = BTreeSet::new();
    for name in named {
        if seen.insert(name) && graph.workspace_id(name).is_none() {
            out.push(Violation::UnknownRuleCrate {
                name: name.to_owned(),
            });
        }
    }
}

fn check_core(graph: &DepGraph, rules: &Rules, out: &mut Vec<Violation>) {
    let Some(core_id) = graph.workspace_id(rules.core_crate) else {
        return;
    };
    let Some(core) = graph.pkgs.get(core_id) else {
        return;
    };
    for (dep_id, kinds) in &core.deps {
        let Some(dep) = graph.pkgs.get(dep_id) else {
            continue;
        };
        if !dep.workspace {
            continue;
        }
        for &kind in kinds {
            let allowed =
                kind == EdgeKind::Dev && rules.core_dev_allowed.contains(&dep.name.as_str());
            if !allowed {
                out.push(Violation::CoreDependsOnWorkspace {
                    core: core.name.clone(),
                    dep: dep.name.clone(),
                    kind,
                });
            }
        }
    }
}

fn check_core_io(graph: &DepGraph, rules: &Rules, out: &mut Vec<Violation>) {
    let Some(core_id) = graph.workspace_id(rules.core_crate) else {
        return;
    };
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([core_id]);
    let mut visited = BTreeSet::from([core_id]);
    while let Some(id) = queue.pop_front() {
        let Some(pkg) = graph.pkgs.get(id) else {
            continue;
        };
        for (dep_id, kinds) in &pkg.deps {
            if !kinds.iter().any(|k| k.is_non_dev()) || !visited.insert(dep_id.as_str()) {
                continue;
            }
            parent.insert(dep_id.as_str(), id);
            let Some(dep) = graph.pkgs.get(dep_id) else {
                continue;
            };
            if rules.core_forbidden_io.contains(&dep.name.as_str()) {
                let mut path = vec![dep_id.as_str()];
                let mut cur = dep_id.as_str();
                while let Some(&p) = parent.get(cur) {
                    path.push(p);
                    cur = p;
                }
                path.reverse();
                out.push(Violation::CoreReachesIo {
                    core: rules.core_crate.to_owned(),
                    io: dep.name.clone(),
                    path: path.into_iter().map(|i| graph.name(i).to_owned()).collect(),
                });
                continue;
            }
            queue.push_back(dep_id.as_str());
        }
    }
}

fn check_hardware(graph: &DepGraph, rules: &Rules, out: &mut Vec<Violation>) {
    let is_sanctioned = |p: &Pkg| p.workspace && rules.hardware_allowed.contains(&p.name.as_str());
    for (start_id, start) in &graph.pkgs {
        if !start.workspace || is_sanctioned(start) {
            continue;
        }
        // BFS over non-dev edges, recording parents for a readable path.
        let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
        let mut queue = VecDeque::from([start_id.as_str()]);
        let mut visited = BTreeSet::from([start_id.as_str()]);
        let mut reported = BTreeSet::new();
        while let Some(id) = queue.pop_front() {
            let Some(pkg) = graph.pkgs.get(id) else {
                continue;
            };
            for (dep_id, kinds) in &pkg.deps {
                if !kinds.iter().any(|k| k.is_non_dev()) || !visited.insert(dep_id.as_str()) {
                    continue;
                }
                parent.insert(dep_id.as_str(), id);
                let Some(dep) = graph.pkgs.get(dep_id) else {
                    continue;
                };
                if rules.hardware_crates.contains(&dep.name.as_str()) {
                    if reported.insert(dep.name.as_str()) {
                        let mut path = vec![dep_id.as_str()];
                        let mut cur = dep_id.as_str();
                        while let Some(&p) = parent.get(cur) {
                            path.push(p);
                            cur = p;
                        }
                        path.reverse();
                        out.push(Violation::HardwareReachable {
                            from: start.name.clone(),
                            hardware: dep.name.clone(),
                            path: path.into_iter().map(|i| graph.name(i).to_owned()).collect(),
                        });
                    }
                    continue;
                }
                if is_sanctioned(dep) {
                    continue; // sanctioned OS boundary; its own deps are its business
                }
                queue.push_back(dep_id.as_str());
            }
        }
    }
}

fn check_dev_only(graph: &DepGraph, rules: &Rules, out: &mut Vec<Violation>) {
    for pkg in graph.pkgs.values() {
        for (dep_id, kinds) in &pkg.deps {
            let Some(dep) = graph.pkgs.get(dep_id) else {
                continue;
            };
            if !dep.workspace || !rules.dev_only_crates.contains(&dep.name.as_str()) {
                continue;
            }
            for &kind in kinds.iter().filter(|k| k.is_non_dev()) {
                out.push(Violation::DevOnlyAsNonDev {
                    from: pkg.name.clone(),
                    dep: dep.name.clone(),
                    kind,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compliant synthetic workspace mirroring the ARCHITECTURE.md Crate Map.
    fn baseline() -> DepGraph {
        let mut g = DepGraph::new();
        for c in [
            "rr-core",
            "rr-defs",
            "rr-rom",
            "rr-link",
            "rr-comms",
            "rr-live",
            "rr-testkit",
            "rr-cli",
            "rr-app",
            "xtask",
        ] {
            g.add_package(c, c, true);
        }
        for ext in [
            "serialport",
            "libloading",
            "nusb",
            "winreg",
            "thiserror",
            "mio",
        ] {
            g.add_package(ext, ext, false);
        }
        let n = EdgeKind::Normal;
        let edges: &[(&str, &[&str])] = &[
            ("rr-defs", &["rr-core", "thiserror"]),
            ("rr-rom", &["rr-core", "rr-defs"]),
            (
                "rr-link",
                &["rr-core", "serialport", "libloading", "nusb", "winreg"],
            ),
            ("rr-comms", &["rr-core", "rr-defs", "rr-link"]),
            ("rr-live", &["rr-core", "rr-defs", "rr-comms", "rr-link"]),
            (
                "rr-testkit",
                &[
                    "rr-core", "rr-defs", "rr-rom", "rr-link", "rr-comms", "rr-live",
                ],
            ),
            (
                "rr-cli",
                &[
                    "rr-core", "rr-defs", "rr-rom", "rr-link", "rr-comms", "rr-live",
                ],
            ),
            (
                "rr-app",
                &[
                    "rr-core", "rr-defs", "rr-rom", "rr-link", "rr-comms", "rr-live",
                ],
            ),
            ("serialport", &["mio"]),
        ];
        for (from, tos) in edges {
            for to in *tos {
                g.add_edge(from, to, n);
            }
        }
        for c in [
            "rr-core", "rr-defs", "rr-rom", "rr-comms", "rr-live", "rr-cli", "rr-app",
        ] {
            g.add_edge(c, "rr-testkit", EdgeKind::Dev);
        }
        g
    }

    #[test]
    fn baseline_passes() {
        assert_eq!(check(&baseline()), vec![]);
    }

    /// Proves US-RR-1-2 (workspace half, synthetic graph).
    #[test]
    fn core_depending_on_defs_fails() {
        let mut g = baseline();
        g.add_edge("rr-core", "rr-defs", EdgeKind::Normal);
        let v = check(&g);
        assert!(v.contains(&Violation::CoreDependsOnWorkspace {
            core: "rr-core".into(),
            dep: "rr-defs".into(),
            kind: EdgeKind::Normal,
        }));
        let msg = v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(msg.contains("rr-core -> rr-defs"), "{msg}");
    }

    #[test]
    fn core_dev_or_build_dep_on_non_testkit_workspace_crate_fails() {
        let mut g = baseline();
        g.add_edge("rr-core", "rr-rom", EdgeKind::Dev);
        g.add_edge("rr-core", "rr-link", EdgeKind::Build);
        let v = check(&g);
        assert!(
            v.iter().any(|x| matches!(x,
            Violation::CoreDependsOnWorkspace { dep, kind: EdgeKind::Dev, .. } if dep == "rr-rom"))
        );
        assert!(v.iter().any(|x| matches!(x,
            Violation::CoreDependsOnWorkspace { dep, kind: EdgeKind::Build, .. } if dep == "rr-link")));
    }

    /// Proves US-RR-1-2 (hardware half, synthetic graph): `rr-core` reaching
    /// any hardware crate is reported.
    #[test]
    fn core_reaching_hardware_fails() {
        for &hw in RULES.hardware_crates {
            let mut g = baseline();
            g.add_edge("rr-core", hw, EdgeKind::Normal);
            assert!(
                check(&g).iter().any(|x| matches!(x,
                Violation::HardwareReachable { from, hardware, .. } if from == "rr-core" && hardware == hw)),
                "rr-core -> {hw} not reported"
            );
        }
    }

    /// Proves US-RR-1-2 (OS/IO half): `rr-core` reaching any named OS/IO crate,
    /// directly or through a third-party crate, is reported; dev edges are not.
    #[test]
    fn core_reaching_any_forbidden_io_crate_fails() {
        for &io in RULES.core_forbidden_io {
            let mut g = baseline();
            g.add_package(io, io, false);
            g.add_edge("rr-core", io, EdgeKind::Normal);
            assert!(
                check(&g).contains(&Violation::CoreReachesIo {
                    core: "rr-core".into(),
                    io: io.into(),
                    path: vec!["rr-core".into(), io.into()],
                }),
                "rr-core -> {io} not reported"
            );

            let mut g = baseline();
            g.add_package(io, io, false);
            g.add_package("helper", "helper", false);
            g.add_edge("rr-core", "helper", EdgeKind::Build);
            g.add_edge("helper", io, EdgeKind::Normal);
            assert!(
                check(&g).contains(&Violation::CoreReachesIo {
                    core: "rr-core".into(),
                    io: io.into(),
                    path: vec!["rr-core".into(), "helper".into(), io.into()],
                }),
                "rr-core -> helper -> {io} not reported"
            );

            let mut g = baseline();
            g.add_package(io, io, false);
            g.add_edge("rr-core", io, EdgeKind::Dev);
            assert_eq!(
                check(&g),
                vec![],
                "dev edge rr-core -> {io} must be allowed"
            );
        }
    }

    #[test]
    fn io_crates_are_only_forbidden_for_core() {
        let mut g = baseline();
        g.add_package("tempfile", "tempfile", false);
        g.add_edge("rr-rom", "tempfile", EdgeKind::Normal);
        assert_eq!(check(&g), vec![]);
    }

    /// Proves US-RR-1-3: the hardware set and its allowlist are exactly what the
    /// criterion names, so dropping a crate from either table fails here.
    #[test]
    fn hardware_tables_match_the_crate_map() {
        assert_eq!(
            RULES.hardware_crates,
            &["serialport", "libloading", "nusb", "winreg"]
        );
        assert_eq!(RULES.hardware_allowed, &["rr-link", "rr-cli", "rr-app"]);
    }

    /// Proves US-RR-1-3 (table-driven, synthetic graph): every workspace crate
    /// other than rr-link/rr-cli/rr-app taking a normal or build edge to each of
    /// serialport, libloading, nusb and winreg is reported, directly and via a
    /// third-party crate; the three sanctioned crates may.
    #[test]
    fn every_unsanctioned_crate_reaching_every_hardware_crate_fails() {
        let base = baseline();
        for from in base.workspace_names() {
            for &hw in RULES.hardware_crates {
                for kind in [EdgeKind::Normal, EdgeKind::Build] {
                    let mut g = baseline();
                    g.add_edge(from, hw, kind);
                    let direct = check(&g);

                    let mut g = baseline();
                    g.add_package("wrapper", "wrapper", false);
                    g.add_edge(from, "wrapper", kind);
                    g.add_edge("wrapper", hw, EdgeKind::Normal);
                    let transitive = check(&g);

                    if RULES.hardware_allowed.contains(&from) {
                        assert_eq!(direct, vec![], "{from} -> {hw} ({kind}) must be allowed");
                        assert_eq!(
                            transitive,
                            vec![],
                            "{from} -> wrapper -> {hw} must be allowed"
                        );
                    } else {
                        assert!(
                            direct.contains(&Violation::HardwareReachable {
                                from: from.into(),
                                hardware: hw.into(),
                                path: vec![from.into(), hw.into()],
                            }),
                            "{from} -> {hw} ({kind}) not reported: {direct:?}"
                        );
                        assert!(
                            transitive.contains(&Violation::HardwareReachable {
                                from: from.into(),
                                hardware: hw.into(),
                                path: vec![from.into(), "wrapper".into(), hw.into()],
                            }),
                            "{from} -> wrapper -> {hw} ({kind}) not reported: {transitive:?}"
                        );
                    }
                }
            }
        }
    }

    /// Proves US-RR-1-3 (single example with message check).
    #[test]
    fn rom_depending_on_serialport_fails() {
        let mut g = baseline();
        g.add_edge("rr-rom", "serialport", EdgeKind::Normal);
        let v = check(&g);
        assert!(v.contains(&Violation::HardwareReachable {
            from: "rr-rom".into(),
            hardware: "serialport".into(),
            path: vec!["rr-rom".into(), "serialport".into()],
        }));
        let msg = v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(msg.contains("rr-rom -> serialport"), "{msg}");
    }

    #[test]
    fn transitive_hardware_via_third_party_fails() {
        let mut g = baseline();
        g.add_package("serial-wrapper", "serial-wrapper", false);
        g.add_edge("serial-wrapper", "winreg", EdgeKind::Normal);
        g.add_edge("rr-defs", "serial-wrapper", EdgeKind::Build);
        assert!(check(&g).contains(&Violation::HardwareReachable {
            from: "rr-defs".into(),
            hardware: "winreg".into(),
            path: vec!["rr-defs".into(), "serial-wrapper".into(), "winreg".into()],
        }));
    }

    #[test]
    fn hardware_as_dev_dependency_is_ignored() {
        let mut g = baseline();
        g.add_edge("rr-rom", "libloading", EdgeKind::Dev);
        assert_eq!(check(&g), vec![]);
    }

    /// Proves US-RR-6-3: a normal dependency on rr-testkit is reported.
    #[test]
    fn defs_normal_dep_on_testkit_fails() {
        let mut g = baseline();
        g.add_edge("rr-defs", "rr-testkit", EdgeKind::Normal);
        let v = check(&g);
        assert!(v.contains(&Violation::DevOnlyAsNonDev {
            from: "rr-defs".into(),
            dep: "rr-testkit".into(),
            kind: EdgeKind::Normal,
        }));
        let msg = v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(msg.contains("rr-defs -> rr-testkit"), "{msg}");
    }

    /// Proves US-RR-6-3: a build dependency on rr-testkit is reported too, even
    /// from a crate that is otherwise privileged (rr-cli).
    #[test]
    fn build_dep_on_testkit_fails_even_from_allowed_crates() {
        let mut g = baseline();
        g.add_edge("rr-cli", "rr-testkit", EdgeKind::Build);
        assert!(check(&g).iter().any(|x| matches!(x,
            Violation::DevOnlyAsNonDev { from, kind: EdgeKind::Build, .. } if from == "rr-cli")));
    }

    #[test]
    fn stale_allowlist_name_is_reported() {
        let mut g = baseline();
        g.pkgs.remove("rr-link");
        assert!(check(&g).contains(&Violation::UnknownRuleCrate {
            name: "rr-link".into()
        }));
    }
}
