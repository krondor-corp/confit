//! Dev port bands: `[ports]` gives each project a fixed 100-port band, fixed
//! ports for shared infra within it, and per-worktree ports for HTTP
//! services. Services get `band + lane + slot`, where `slot` is a small
//! per-branch integer (0 for a primary branch, else 1..=9) handed out by
//! [`crate::slots`] -- the lowest one not already claimed by another
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
//! expands (say the branch is assigned slot 3) into `ports.branch`,
//! `ports.branch_slug`, `ports.slot`, `ports.infra.postgres = 4300`,
//! `ports.services.app = 4353`, etc. — ordinary values, resolved through the
//! same `{ref}` pipeline as everything else in confit.toml.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::error::{Error, Result};
use crate::git::Git;

const DEFAULT_PRIMARY_BRANCHES: &[&str] = &["main", "master"];

/// The declared shape of a `[ports]` table, before expansion. Deserialized
/// directly from the raw `toml::Value` -- no manual `.get()`/`.as_*()`
/// field-by-field poking.
#[derive(Debug, Deserialize)]
struct PortsSpec {
    band: i64,
    #[serde(default)]
    primary_branches: Vec<String>,
    #[serde(default)]
    infra: BTreeMap<String, i64>,
    #[serde(default)]
    services: BTreeMap<String, i64>,
}

/// The shape of a `[ports]` table after [`expand_ports`]: what confit.toml's
/// `{ports.*}` refs resolve against, and what [`check_host`] reads back.
/// `infra`/`services` here hold fully-resolved ports, not offsets/lanes.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResolvedPorts {
    band: i64,
    branch: String,
    branch_slug: String,
    slot: u8,
    primary_branches: Vec<String>,
    #[serde(default)]
    infra: BTreeMap<String, i64>,
    #[serde(default)]
    services: BTreeMap<String, i64>,
}

/// The current branch name, via [`Git::current_branch`].
pub fn current_branch(cwd: &Path) -> Result<String> {
    Git::new(cwd).current_branch()
}

/// Lowercase, alnum-and-dash only, collapsed and trimmed — safe for DB names,
/// bucket names, container names, etc.
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
    }
    out.trim_end_matches('-').to_string()
}

fn is_primary_branch(branch: &str, primary_branches: &[String]) -> bool {
    if primary_branches.is_empty() {
        DEFAULT_PRIMARY_BRANCHES.contains(&branch)
    } else {
        primary_branches.iter().any(|p| p == branch)
    }
}

/// Expand a `[ports]` table: resolves `infra.*` to `band + offset`,
/// `services.*` to `band + lane + slot`, and adds `branch`, `branch_slug`,
/// `slot` as plain values alongside `band`. `slot` comes from the ledger in
/// [`crate::slots`], which requires `cwd` to be inside a git working tree.
pub fn expand_ports(ports: &Value, branch: &str, cwd: &Path) -> Result<Value> {
    let spec: PortsSpec = ports
        .clone()
        .try_into()
        .map_err(|e| Error::Runtime(format!("[ports]: {e}")))?;

    let slug = slugify(branch);
    let slot = if is_primary_branch(branch, &spec.primary_branches) {
        0
    } else {
        crate::slots::assign(&Git::new(cwd), branch)?
    };

    let primary_branches = if spec.primary_branches.is_empty() {
        DEFAULT_PRIMARY_BRANCHES
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        spec.primary_branches
    };

    let resolved = ResolvedPorts {
        band: spec.band,
        branch: branch.to_string(),
        branch_slug: slug,
        slot,
        primary_branches,
        infra: spec
            .infra
            .into_iter()
            .map(|(name, offset)| (name, spec.band + offset))
            .collect(),
        services: spec
            .services
            .into_iter()
            .map(|(name, lane)| (name, spec.band + lane + slot as i64))
            .collect(),
    };

    Value::try_from(&resolved).map_err(|e| Error::Runtime(format!("[ports]: {e}")))
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
        read("net.inet.ip.portrange.hifirst")?,
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn host_ephemeral_range() -> Option<(u16, u16)> {
    None
}

/// Validate an already-[`expand_ports`]-ed `[ports]` table against this
/// host: within-file port collisions, privileged/out-of-range ports, ports
/// inside the host's ephemeral range, service ports already bound, and (by
/// reading the [`crate::slots`] ledger) two branches somehow sharing a slot
/// -- which should be structurally impossible via [`expand_ports`], but is
/// worth catching if the ledger file was hand-edited or corrupted.
pub fn check_host(expanded_ports: &Value, cwd: &Path) -> Result<Vec<HostIssue>> {
    let resolved: ResolvedPorts = expanded_ports
        .clone()
        .try_into()
        .map_err(|e| Error::Runtime(format!("[ports]: {e}")))?;

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

    // Ledger integrity: expand_ports hands out each slot at most once, so
    // two branches sharing a slot here means the ledger file was edited or
    // corrupted out from under confit.
    if let Ok(ledger) = crate::slots::read(&Git::new(cwd)) {
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

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        dir
    }

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("feature/foo-bar"), "feature-foo-bar");
        assert_eq!(slugify("Feature/FOO_BAR!!"), "feature-foo-bar");
        assert_eq!(slugify("main"), "main");
        assert_eq!(slugify("--weird--"), "weird");
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
    fn test_expand_ports_infra_and_services() {
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
        let expanded = expand_ports(&ports, "main", dir.path()).unwrap();
        let table = expanded.as_table().unwrap();
        assert_eq!(table["band"].as_integer().unwrap(), 4300);
        assert_eq!(table["branch"].as_str().unwrap(), "main");
        assert_eq!(table["branch_slug"].as_str().unwrap(), "main");
        assert_eq!(table["slot"].as_integer().unwrap(), 0);

        let infra = table["infra"].as_table().unwrap();
        assert_eq!(infra["postgres"].as_integer().unwrap(), 4300);
        assert_eq!(infra["redis"].as_integer().unwrap(), 4301);

        let services = table["services"].as_table().unwrap();
        assert_eq!(services["app"].as_integer().unwrap(), 4350);
        assert_eq!(services["site"].as_integer().unwrap(), 4370);
    }

    #[test]
    fn test_expand_ports_feature_branch_offsets_services_not_infra() {
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
        let expanded = expand_ports(&ports, "feature/thing", dir.path()).unwrap();
        let table = expanded.as_table().unwrap();
        let slot = table["slot"].as_integer().unwrap();
        assert!((1..=9).contains(&slot));
        assert_eq!(
            table["infra"].as_table().unwrap()["postgres"]
                .as_integer()
                .unwrap(),
            4300
        );
        assert_eq!(
            table["services"].as_table().unwrap()["app"]
                .as_integer()
                .unwrap(),
            4300 + 50 + slot
        );
    }

    #[test]
    fn test_expand_ports_requires_band() {
        let ports: Value = "[infra]\npostgres = 0".parse().unwrap();
        let result = expand_ports(&ports, "main", Path::new("."));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("band"));
    }

    #[test]
    fn test_expand_ports_persists_primary_branches() {
        let ports: Value = "band = 4300".parse().unwrap();
        let expanded = expand_ports(&ports, "main", Path::new(".")).unwrap();
        let table = expanded.as_table().unwrap();
        let primary: Vec<&str> = table["primary_branches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(primary, vec!["main", "master"]);
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
        let expanded = expand_ports(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&expanded, dir.path()).unwrap();
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
        let expanded = expand_ports(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&expanded, dir.path()).unwrap();
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
        let expanded = expand_ports(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&expanded, dir.path()).unwrap();
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
        let expanded = expand_ports(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&expanded, dir.path()).unwrap();
        assert!(
            issues
                .iter()
                .all(|i| i.severity == Severity::Warning && i.message.contains("already bound")),
            "unexpected issues: {issues:?}"
        );
    }

    #[test]
    fn test_expand_ports_gives_distinct_slots_across_worktrees() {
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

        // These two branch names hash to the same bucket under the old
        // fnv1a-based scheme -- confirming the stateful ledger no longer
        // cares is the whole point of this test.
        let mut slots = Vec::new();
        for (i, branch) in ["feature/b7", "feature/b36"].iter().enumerate() {
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
            let expanded = expand_ports(&ports, branch, dir.path()).unwrap();
            slots.push(expanded["slot"].as_integer().unwrap());
            std::mem::forget(wt); // keep the worktree alive for the rest of the test
            let _ = i;
        }
        assert_ne!(slots[0], slots[1]);

        let expanded = expand_ports(&ports, "feature/b7", dir.path()).unwrap();
        let issues = check_host(&expanded, dir.path()).unwrap();
        assert!(
            issues.is_empty(),
            "expected no host issues, got: {issues:?}"
        );
    }

    #[test]
    fn test_check_host_detects_corrupted_ledger() {
        let dir = init_repo();
        let git = Git::new(dir.path());
        let ledger_path = git.common_dir().unwrap().join("confit").join("ports.toml");
        std::fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
        std::fs::write(&ledger_path, "\"feature/a\" = 3\n\"feature/b\" = 3\n").unwrap();

        // "main" is primary, so expand_ports never touches the ledger here --
        // this isolates check_host's integrity check from assign()'s own pruning.
        let ports: Value = "band = 20000\n[services]\napp = 50".parse().unwrap();
        let expanded = expand_ports(&ports, "main", dir.path()).unwrap();
        let issues = check_host(&expanded, dir.path()).unwrap();
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Error
                && i.message.contains("claimed by multiple branches")));
    }
}
