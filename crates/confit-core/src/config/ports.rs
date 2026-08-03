//! Dev port bands: `[ports]` gives each project a fixed 100-port band, fixed
//! ports for shared infra within it, and per-worktree ports for HTTP
//! services. Services get `band + lane + slot`, where `slot` is a small
//! per-branch integer (0 for a primary branch, else 1..=9) handed out by
//! [`super::slots`] -- the lowest one not already claimed by another
//! currently checked-out worktree of the same repo -- so two active
//! branches can never collide on the same ports.
//!
//! ```toml
//! [ports]
//! band = 4300
//!
//! [ports.infra]
//! postgres = 0
//! redis = 1
//!
//! [ports.services]
//! app = 50
//! site = 70
//! ```
//!
//! [`resolve`] turns this into a [`ResolvedPorts`] (say the branch is
//! assigned slot 3): `slug`, `slot`, `infra.postgres = 4300`,
//! `services.app = 4353`, etc. [`super`] mirrors those fields back into the
//! generic config tree so confit.toml's `{ports.*}` refs resolve against
//! them like any other value.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::error::{Error, Result};
use crate::git::Git;

const DEFAULT_PRIMARY_BRANCHES: &[&str] = &["main", "master"];

/// Slugs are truncated to this length -- safe under common DNS-label
/// (63 char) and bucket/container name limits.
const MAX_SLUG_LEN: usize = 63;

/// The declared shape of a `[ports]` table, before resolution. Deserialized
/// directly from the raw `toml::Value` -- no manual `.get()`/`.as_*()`
/// field-by-field poking. Unknown fields are rejected: the resolved mirror
/// replaces the raw table in the config tree, so a silently-ignored typo
/// (`[ports.service]`) would otherwise just vanish.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PortsSpec {
    band: i64,
    #[serde(default)]
    primary_branches: Vec<String>,
    #[serde(default)]
    infra: BTreeMap<String, i64>,
    #[serde(default)]
    services: BTreeMap<String, i64>,
}

/// A `[ports]` table after [`PortsSpec::resolve`]. `infra`/`services` hold
/// fully-resolved ports here, not offsets/lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPorts {
    pub band: i64,
    /// The current branch, lowercased, cleaned to `[a-z0-9-]`, and
    /// length-capped -- safe to use in a database name, bucket name, or
    /// container name.
    pub slug: String,
    pub slot: u8,
    /// Fully-resolved ports (not offsets), keyed by name.
    #[serde(default)]
    pub infra: BTreeMap<String, i64>,
    /// Fully-resolved ports (not lanes), keyed by name.
    #[serde(default)]
    pub services: BTreeMap<String, i64>,
}

/// The current branch name, via [`Git::current_branch`].
pub fn current_branch(cwd: &Path) -> Result<String> {
    Git::new(cwd).current_branch()
}

/// Lowercase, alnum-and-dash only, collapsed, trimmed, and length-capped --
/// safe for DB names, bucket names, container names, etc.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= MAX_SLUG_LEN {
            break;
        }
    }
    out.truncate(MAX_SLUG_LEN);
    out.trim_end_matches('-').to_string()
}

fn is_primary_branch(branch: &str, primary_branches: &[String]) -> bool {
    if primary_branches.is_empty() {
        DEFAULT_PRIMARY_BRANCHES.contains(&branch)
    } else {
        primary_branches.iter().any(|p| p == branch)
    }
}

impl PortsSpec {
    /// Shape-validate a raw `[ports]` table. Pure -- no git, no filesystem
    /// -- so it can run eagerly at `Config::build` while the effectful
    /// [`PortsSpec::resolve`] stays deferred.
    pub(super) fn parse(ports: Value) -> Result<Self> {
        ports
            .try_into()
            .map_err(|e| Error::Runtime(format!("[ports]: {e}")))
    }

    /// Resolve against git state: `infra.*` becomes `band + offset`,
    /// `services.*` becomes `band + lane + slot`, and `slug`/`slot` are
    /// derived from `branch`. `slot` comes from the ledger in
    /// [`super::slots`], which requires `cwd` to be inside a git working
    /// tree.
    pub(super) fn resolve(&self, branch: &str, cwd: &Path) -> Result<ResolvedPorts> {
        let slug = slugify(branch);
        let slot = if is_primary_branch(branch, &self.primary_branches) {
            0
        } else {
            super::slots::assign(&Git::new(cwd), branch)?
        };

        Ok(ResolvedPorts {
            band: self.band,
            slug,
            slot,
            infra: self
                .infra
                .iter()
                .map(|(name, offset)| (name.clone(), self.band + offset))
                .collect(),
            services: self
                .services
                .iter()
                .map(|(name, lane)| (name.clone(), self.band + lane + slot as i64))
                .collect(),
        })
    }
}

/// Parse and resolve a raw `[ports]` table in one step (test convenience;
/// production code goes through `Config`, which parses eagerly and
/// resolves lazily).
#[cfg(test)]
pub(super) fn resolve(ports: &Value, branch: &str, cwd: &Path) -> Result<ResolvedPorts> {
    PortsSpec::parse(ports.clone())?.resolve(branch, cwd)
}

/// How serious a [`HostIssue`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A real correctness problem: ports collide, or aren't valid TCP ports.
    Error,
    /// Worth knowing, not necessarily wrong (e.g. a port is currently bound,
    /// which is expected for `infra.*`).
    Warning,
}

/// One finding from [`check_host`].
#[derive(Debug, Clone)]
pub struct HostIssue {
    /// Dotted path relative to `ports`, e.g. `infra.postgres`.
    pub path: String,
    pub severity: Severity,
    pub message: String,
}

/// Read this host's ephemeral (dynamic, OS-assigned outbound) port range, if
/// determinable. A `[ports]` band that overlaps it risks the OS handing out
/// one of its ports for an unrelated outbound connection.
#[cfg(target_os = "linux")]
fn host_ephemeral_range() -> Option<(u16, u16)> {
    let content = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").ok()?;
    let mut parts = content.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

#[cfg(target_os = "macos")]
fn host_ephemeral_range() -> Option<(u16, u16)> {
    use std::process::Command;
    let read = |key: &str| -> Option<u16> {
        let out = Command::new("sysctl").args(["-n", key]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    };
    Some((
        read("net.inet.ip.portrange.first")?,
        read("net.inet.ip.portrange.last")?,
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn host_ephemeral_range() -> Option<(u16, u16)> {
    None
}

/// Validate a [`ResolvedPorts`] against this host: within-file port
/// collisions, privileged/out-of-range ports, ports inside the host's
/// ephemeral range, service ports already bound, and (by reading the
/// [`super::slots`] ledger) two branches somehow sharing a slot -- which
/// should be structurally impossible via [`resolve`], but is worth catching
/// if the ledger file was hand-edited or corrupted.
pub fn check_host(resolved: &ResolvedPorts, cwd: &Path) -> Result<Vec<HostIssue>> {
    let infra_ports: Vec<(String, i64)> = resolved
        .infra
        .iter()
        .map(|(name, port)| (format!("infra.{name}"), *port))
        .collect();
    let service_ports: Vec<(String, i64)> = resolved
        .services
        .iter()
        .map(|(name, port)| (format!("services.{name}"), *port))
        .collect();
    let all_ports: Vec<(String, i64)> = infra_ports
        .iter()
        .chain(service_ports.iter())
        .cloned()
        .collect();

    let mut issues = Vec::new();

    // Within-file collisions: two names resolving to the same port.
    for i in 0..all_ports.len() {
        for j in (i + 1)..all_ports.len() {
            if all_ports[i].1 == all_ports[j].1 {
                issues.push(HostIssue {
                    path: all_ports[i].0.clone(),
                    severity: Severity::Error,
                    message: format!(
                        "collides with ports.{} (both resolve to {})",
                        all_ports[j].0, all_ports[i].1
                    ),
                });
            }
        }
    }

    // Range checks, informed by this host's actual ephemeral range where available.
    let ephemeral = host_ephemeral_range();
    for (name, port) in &all_ports {
        if !(0..=65535).contains(port) {
            issues.push(HostIssue {
                path: name.clone(),
                severity: Severity::Error,
                message: format!("{port} is not a valid TCP port (0-65535)"),
            });
            continue;
        }
        if *port < 1024 {
            issues.push(HostIssue {
                path: name.clone(),
                severity: Severity::Warning,
                message: format!(
                    "{port} is a privileged port (<1024); binding it may require root"
                ),
            });
        }
        if let Some((lo, hi)) = ephemeral {
            if *port >= lo as i64 && *port <= hi as i64 {
                issues.push(HostIssue {
                    path: name.clone(),
                    severity: Severity::Warning,
                    message: format!(
                        "{port} falls inside this host's ephemeral port range \
                         ({lo}-{hi}); the OS may hand it out for an outbound connection"
                    ),
                });
            }
        }
    }

    // Cross-slot lane spacing: every branch's services resolve to
    // band + lane + slot with slot in 0..=MAX_SLOT, so two lanes closer
    // than MAX_SLOT+1 apart can land on the same port for two different
    // branches. The current branch's resolved services all share one slot,
    // so pairwise resolved differences equal lane differences.
    for i in 0..service_ports.len() {
        for j in (i + 1)..service_ports.len() {
            let diff = (service_ports[i].1 - service_ports[j].1).abs();
            if diff > 0 && diff <= super::slots::MAX_SLOT as i64 {
                issues.push(HostIssue {
                    path: service_ports[i].0.clone(),
                    severity: Severity::Error,
                    message: format!(
                        "lane is only {diff} apart from ports.{}; lanes must be at least \
                         {} apart or two branches' worktree slots can resolve to the same port",
                        service_ports[j].0,
                        super::slots::MAX_SLOT + 1
                    ),
                });
            }
        }
    }

    // Infra offsets inside a service's slot window collide with that
    // service on some branch: service lane + slot sweeps lane..=lane+MAX_SLOT.
    let slot = resolved.slot as i64;
    for (svc_name, svc_port) in &service_ports {
        let lane_start = svc_port - slot;
        for (infra_name, infra_port) in &infra_ports {
            if *infra_port != *svc_port
                && (lane_start..=lane_start + super::slots::MAX_SLOT as i64).contains(infra_port)
            {
                issues.push(HostIssue {
                    path: infra_name.clone(),
                    severity: Severity::Error,
                    message: format!(
                        "fixed port {infra_port} falls inside ports.{svc_name}'s worktree-slot \
                         window ({lane_start}-{}); a branch assigned the matching slot would \
                         collide with it",
                        lane_start + super::slots::MAX_SLOT as i64
                    ),
                });
            }
        }
    }

    // Live bind check for services only -- infra ports are expected to
    // already be held by the container/process that owns them. A privileged
    // port fails to bind as non-root regardless of whether it's free, so
    // only AddrInUse counts as "already bound" here.
    for (name, port) in &service_ports {
        if !(0..=65535).contains(port) {
            continue;
        }
        if let Err(e) = TcpListener::bind(("127.0.0.1", *port as u16)) {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                issues.push(HostIssue {
                    path: name.clone(),
                    severity: Severity::Warning,
                    message: format!("{port} is already bound on this host"),
                });
            }
        }
    }

    // Ledger integrity: assignment hands out each slot at most once, so two
    // branches sharing a slot here means the ledger file was edited out
    // from under confit. An unreadable/unparseable ledger is itself an
    // issue, not something to skip silently.
    match super::slots::read(&Git::new(cwd)) {
        Ok(ledger) => {
            let mut by_slot: std::collections::HashMap<u8, Vec<&String>> =
                std::collections::HashMap::new();
            for (b, s) in &ledger {
                by_slot.entry(*s).or_default().push(b);
            }
            for (slot, branches) in by_slot {
                if branches.len() > 1 {
                    issues.push(HostIssue {
                        path: "slot".into(),
                        severity: Severity::Error,
                        message: format!(
                            "slot {slot} is claimed by multiple branches in the local ledger \
                             (.git/confit/ports.toml): {} -- delete the stale entry and re-run",
                            branches
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
            }
        }
        Err(e) => issues.push(HostIssue {
            path: "slot".into(),
            severity: Severity::Error,
            message: e.to_string(),
        }),
    }

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::init_repo;

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("feature/foo-bar"), "feature-foo-bar");
        assert_eq!(slugify("Feature/FOO_BAR!!"), "feature-foo-bar");
        assert_eq!(slugify("main"), "main");
        assert_eq!(slugify("--weird--"), "weird");
    }

    #[test]
    fn test_slugify_truncates_long_branch_names() {
        let long = "a".repeat(200);
        let slug = slugify(&long);
        assert_eq!(slug.len(), MAX_SLUG_LEN);
        assert_eq!(slug, "a".repeat(MAX_SLUG_LEN));
    }

    #[test]
    fn test_slugify_truncation_trims_trailing_dash() {
        // 62 a's + '/' -> the '/' becomes a dash right at the cutoff.
        let name = format!("{}/rest-of-branch-name", "a".repeat(62));
        let slug = slugify(&name);
        assert!(slug.len() <= MAX_SLUG_LEN);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn test_is_primary_branch_default() {
        assert!(is_primary_branch("main", &[]));
        assert!(is_primary_branch("master", &[]));
        assert!(!is_primary_branch("feature/a", &[]));
    }

    #[test]
    fn test_is_primary_branch_custom() {
        let primary = vec!["trunk".to_string()];
        assert!(is_primary_branch("trunk", &primary));
        // "main" is no longer special once primary_branches is set explicitly.
        assert!(!is_primary_branch("main", &primary));
    }

    #[test]
    fn test_resolve_infra_and_services() {
        let ports: Value = r#"
            band = 4300
            [infra]
            postgres = 0
            redis = 1
            [services]
            app = 50
            site = 70
        "#
        .parse()
        .unwrap();

        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        assert_eq!(resolved.band, 4300);
        assert_eq!(resolved.slug, "main");
        assert_eq!(resolved.slot, 0);
        assert_eq!(resolved.infra["postgres"], 4300);
        assert_eq!(resolved.infra["redis"], 4301);
        assert_eq!(resolved.services["app"], 4350);
        assert_eq!(resolved.services["site"], 4370);
    }

    #[test]
    fn test_resolve_feature_branch_offsets_services_not_infra() {
        let ports: Value = r#"
            band = 4300
            [infra]
            postgres = 0
            [services]
            app = 50
        "#
        .parse()
        .unwrap();

        let dir = init_repo();
        let resolved = resolve(&ports, "feature/thing", dir.path()).unwrap();
        assert!((1..=9).contains(&resolved.slot));
        assert_eq!(resolved.infra["postgres"], 4300);
        assert_eq!(resolved.services["app"], 4300 + 50 + resolved.slot as i64);
    }

    #[test]
    fn test_resolve_requires_band() {
        let ports: Value = "[infra]\npostgres = 0".parse().unwrap();
        let result = resolve(&ports, "main", Path::new("."));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("band"));
    }

    #[test]
    fn test_parse_rejects_unknown_fields() {
        // A typo'd sub-table must fail loudly: the resolved mirror replaces
        // the raw table, so an ignored unknown field would silently vanish.
        let ports: Value = "band = 4300\n[service]\napp = 50".parse().unwrap();
        let err = PortsSpec::parse(ports).unwrap_err().to_string();
        assert!(err.contains("service"), "got: {err}");
    }

    #[test]
    fn test_check_host_detects_within_file_collision() {
        let ports: Value = r#"
            band = 4300
            [infra]
            postgres = 0
            redis = 0
        "#
        .parse()
        .unwrap();
        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.message.contains("collides")));
    }

    #[test]
    fn test_check_host_flags_privileged_port() {
        let ports: Value = r#"
            band = 80
            [infra]
            web = 0
        "#
        .parse()
        .unwrap();
        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Warning && i.message.contains("privileged")));
    }

    #[test]
    fn test_check_host_flags_bound_service_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // main -> slot 0, so app resolves to exactly band + 0 + 0 = port.
        let ports: Value = format!("band = {port}\n[services]\napp = 0")
            .parse()
            .unwrap();
        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(issues.iter().any(|i| i.message.contains("already bound")));
        drop(listener);
    }

    #[test]
    fn test_check_host_no_issues_for_clean_config() {
        // Below both Linux's (32768-60999) and macOS's (~49152-65535)
        // default ephemeral ranges, so this doesn't trip the ephemeral-range
        // warning on either CI or a dev machine.
        let ports: Value = r#"
            band = 20000
            [infra]
            postgres = 0
            [services]
            app = 50
        "#
        .parse()
        .unwrap();
        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(
            issues
                .iter()
                .all(|i| i.severity == Severity::Warning && i.message.contains("already bound")),
            "unexpected issues: {issues:?}"
        );
    }

    #[test]
    fn test_resolve_gives_distinct_slots_across_worktrees() {
        let dir = init_repo();
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                "init",
            ])
            .current_dir(dir.path())
            .status()
            .unwrap();

        let ports: Value = "band = 20000\n[services]\napp = 50".parse().unwrap();

        // Two arbitrary branch names, each checked out in its own worktree.
        let mut slots = Vec::new();
        for branch in ["feature/b7", "feature/b36"] {
            let wt = tempfile::tempdir().unwrap();
            let wt_path = wt.path().join("wt");
            assert!(std::process::Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    branch,
                    wt_path.to_str().unwrap(),
                ])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success());
            let resolved = resolve(&ports, branch, dir.path()).unwrap();
            slots.push(resolved.slot);
            std::mem::forget(wt); // keep the worktree alive for the rest of the test
        }
        assert_ne!(slots[0], slots[1]);

        let resolved = resolve(&ports, "feature/b7", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(
            issues.is_empty(),
            "expected no host issues, got: {issues:?}"
        );
    }

    #[test]
    fn test_check_host_detects_duplicate_slot_in_ledger() {
        let dir = init_repo();
        let git = Git::new(dir.path());
        let ledger_path = git.common_dir().unwrap().join("confit").join("ports.toml");
        std::fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
        std::fs::write(&ledger_path, "\"feature/a\" = 3\n\"feature/b\" = 3\n").unwrap();

        // "main" is primary, so resolve() never touches the ledger here --
        // this isolates check_host's integrity check from assign()'s own pruning.
        let ports: Value = "band = 20000\n[services]\napp = 50".parse().unwrap();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Error
                && i.message.contains("claimed by multiple branches")));
    }

    #[test]
    fn test_check_host_surfaces_unparseable_ledger() {
        let dir = init_repo();
        let git = Git::new(dir.path());
        let ledger_path = git.common_dir().unwrap().join("confit").join("ports.toml");
        std::fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
        std::fs::write(&ledger_path, "not toml [[[").unwrap();

        let ports: Value = "band = 20000\n[services]\napp = 50".parse().unwrap();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.message.contains("corrupted")),
            "got: {issues:?}"
        );
    }

    #[test]
    fn test_check_host_flags_lanes_closer_than_slot_range() {
        // app=50 and site=55: slot 1's site (band+56) equals slot 6's app
        // (band+56) on another branch, so lanes <10 apart are an error.
        let ports: Value = "band = 20000\n[services]\napp = 50\nsite = 55"
            .parse()
            .unwrap();
        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.message.contains("lanes must be")),
            "got: {issues:?}"
        );
    }

    #[test]
    fn test_check_host_flags_infra_inside_service_slot_window() {
        // infra web=52 sits in service app's slot window [50, 59]: the
        // branch assigned slot 2 would resolve app to the same port.
        let ports: Value = "band = 20000\n[infra]\nweb = 52\n[services]\napp = 50"
            .parse()
            .unwrap();
        let dir = init_repo();
        let resolved = resolve(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&resolved, dir.path()).unwrap();
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.message.contains("slot window")),
            "got: {issues:?}"
        );
    }
}
