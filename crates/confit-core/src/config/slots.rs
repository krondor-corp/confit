//! A per-repo, per-machine ledger of `branch -> port slot` assignments.
//!
//! Stored inside the git common dir (`git rev-parse --git-common-dir`), so
//! it's shared by every worktree of a repo but never committed and never
//! synced between machines -- which matches its scope exactly, since
//! worktrees are inherently local to one machine.
//!
//! Slots are handed out lowest-first and any branch no longer checked out
//! anywhere is proactively pruned before each assignment, so two branches
//! that happen to be active at the same time can never be handed the same
//! slot.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git::Git;

/// Slots run `1..=MAX_SLOT`; `0` is reserved for primary branches and never
/// enters the ledger. Kept single-digit so `band + lane + slot` preserves
/// the "last digit is the worktree slot" convention when lanes are spaced
/// by 10.
pub const MAX_SLOT: u8 = 9;

fn state_path(git: &Git) -> Result<PathBuf> {
    Ok(git.common_dir()?.join("confit").join("ports.toml"))
}

/// Take an exclusive advisory lock (`flock`) on a sibling `.lock` file.
/// The lock is tied to the open file descriptor, so it's released
/// automatically when the guard drops -- including on process death, which
/// makes it safe against a crashed holder without any staleness heuristics.
/// The lock file itself is left in place; only the lock matters, not the
/// file's existence.
fn open_lock(ledger_path: &Path) -> Result<fd_lock::RwLock<fs::File>> {
    let lock_path = ledger_path.with_extension("lock");
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| Error::Io {
            path: lock_path.clone(),
            source: e,
        })?;
    Ok(fd_lock::RwLock::new(file))
}

fn corrupt<E: std::fmt::Display>(path: &Path) -> impl Fn(E) -> Error + '_ {
    move |e| {
        Error::Runtime(format!(
            "port-slot ledger {} is corrupted ({e}); fix or delete it and re-run",
            path.display()
        ))
    }
}

/// Read the ledger. A missing file is an empty ledger; an unreadable or
/// unparseable one is an error -- never silently treated as empty, since
/// that would re-deal slots already owned by live worktrees.
fn load(path: &Path) -> Result<BTreeMap<String, u8>> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => {
            return Err(Error::Io {
                path: path.to_path_buf(),
                source: e,
            })
        }
    };
    content
        .parse::<toml::Value>()
        .map_err(corrupt(path))?
        .try_into()
        .map_err(corrupt(path))
}

fn save(path: &Path, slots: &BTreeMap<String, u8>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let out = toml::to_string(slots)
        .map_err(|e| Error::Runtime(format!("serializing port-slot ledger: {e}")))?;
    fs::write(path, out).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Look up (or assign) `branch`'s slot in `1..=MAX_SLOT`, reusing its
/// existing assignment if it already has one. Proactively prunes any branch
/// no longer checked out in any worktree of this repo first, so slots pack
/// as tightly as possible. The ledger is only rewritten when it actually
/// changed.
pub fn assign(git: &Git, branch: &str) -> Result<u8> {
    let path = state_path(git)?;
    let mut lock = open_lock(&path)?;
    let _guard = lock.write().map_err(|e| Error::Io {
        path: path.with_extension("lock"),
        source: e,
    })?;

    let mut slots = load(&path)?;

    let live: HashSet<String> = git
        .worktrees()?
        .into_iter()
        .filter_map(|w| w.branch)
        .collect();
    let before = slots.len();
    slots.retain(|b, _| live.contains(b) || b == branch);
    let mut changed = slots.len() != before;

    let slot = match slots.get(branch) {
        Some(&slot) => slot,
        None => {
            let used: HashSet<u8> = slots.values().copied().collect();
            let slot = (1..=MAX_SLOT).find(|s| !used.contains(s)).ok_or_else(|| {
                Error::Runtime(format!(
                    "no free port slot for branch '{branch}': all {MAX_SLOT} slots are claimed by \
                     other active worktrees of this repo (`git worktree list` to see them, \
                     `git worktree remove` to free one up)"
                ))
            })?;
            slots.insert(branch.to_string(), slot);
            changed = true;
            slot
        }
    };

    if changed {
        save(&path, &slots)?;
    }
    Ok(slot)
}

/// Read the ledger without mutating it (diagnostics / integrity checks).
/// Propagates corruption as an error rather than an empty map.
pub fn read(git: &Git) -> Result<BTreeMap<String, u8>> {
    load(&state_path(git)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{add_worktree, init_repo_with_commit};

    #[test]
    fn test_assign_lowest_free_slot() {
        let dir = init_repo_with_commit();
        let git = Git::new(dir.path());

        let wt_a = tempfile::tempdir().unwrap();
        add_worktree(dir.path(), wt_a.path().join("wt").as_path(), "feature/a");
        let wt_b = tempfile::tempdir().unwrap();
        add_worktree(dir.path(), wt_b.path().join("wt").as_path(), "feature/b");

        let slot_a = assign(&git, "feature/a").unwrap();
        let slot_b = assign(&git, "feature/b").unwrap();
        assert_eq!(slot_a, 1);
        assert_eq!(slot_b, 2);
        // Reassigning is stable.
        assert_eq!(assign(&git, "feature/a").unwrap(), 1);
    }

    #[test]
    fn test_assign_reclaims_pruned_slot() {
        let dir = init_repo_with_commit();
        let git = Git::new(dir.path());

        let wt_a = tempfile::tempdir().unwrap();
        let wt_a_path = wt_a.path().join("wt");
        add_worktree(dir.path(), &wt_a_path, "feature/a");
        assert_eq!(assign(&git, "feature/a").unwrap(), 1);

        // Remove the worktree -- feature/a is no longer checked out anywhere.
        assert!(std::process::Command::new("git")
            .args(["worktree", "remove", "-f", wt_a_path.to_str().unwrap()])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());

        let wt_b = tempfile::tempdir().unwrap();
        add_worktree(dir.path(), wt_b.path().join("wt").as_path(), "feature/b");
        // feature/b should reclaim slot 1, the lowest free one, not get slot 2.
        assert_eq!(assign(&git, "feature/b").unwrap(), 1);
    }

    #[test]
    fn test_assign_exhaustion_errors() {
        let dir = init_repo_with_commit();
        let git = Git::new(dir.path());

        for i in 0..MAX_SLOT {
            let wt = tempfile::tempdir().unwrap();
            let branch = format!("feature/{i}");
            add_worktree(dir.path(), wt.path().join("wt").as_path(), &branch);
            assign(&git, &branch).unwrap();
            std::mem::forget(wt); // keep the worktree alive for the test's duration
        }

        let extra_wt = tempfile::tempdir().unwrap();
        add_worktree(
            dir.path(),
            extra_wt.path().join("wt").as_path(),
            "feature/overflow",
        );
        let result = assign(&git, "feature/overflow");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no free port slot"));
    }

    #[test]
    fn test_corrupted_ledger_errors_instead_of_resetting() {
        let dir = init_repo_with_commit();
        let git = Git::new(dir.path());
        let path = state_path(&git).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not valid toml [[[").unwrap();

        let read_err = read(&git).unwrap_err().to_string();
        assert!(read_err.contains("corrupted"), "got: {read_err}");

        let wt = tempfile::tempdir().unwrap();
        add_worktree(dir.path(), wt.path().join("wt").as_path(), "feature/a");
        let assign_err = assign(&git, "feature/a").unwrap_err().to_string();
        assert!(assign_err.contains("corrupted"), "got: {assign_err}");
        // The corrupted file must not have been overwritten.
        assert_eq!(fs::read_to_string(&path).unwrap(), "not valid toml [[[");
    }

    #[test]
    fn test_noop_assign_does_not_rewrite_ledger() {
        let dir = init_repo_with_commit();
        let git = Git::new(dir.path());

        let wt = tempfile::tempdir().unwrap();
        add_worktree(dir.path(), wt.path().join("wt").as_path(), "feature/a");
        assert_eq!(assign(&git, "feature/a").unwrap(), 1);

        let path = state_path(&git).unwrap();
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Same branch, nothing pruned: the file must not be rewritten.
        assert_eq!(assign(&git, "feature/a").unwrap(), 1);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }

    #[test]
    fn test_branch_names_with_special_chars_round_trip() {
        let dir = init_repo_with_commit();
        let git = Git::new(dir.path());

        // Quotes and non-ASCII are legal in git branch names; the ledger
        // must round-trip them without corrupting itself.
        let branch = "feature/we\"ird-tëst";
        let wt = tempfile::tempdir().unwrap();
        add_worktree(dir.path(), wt.path().join("wt").as_path(), branch);
        let slot = assign(&git, branch).unwrap();
        assert_eq!(read(&git).unwrap().get(branch), Some(&slot));
        assert_eq!(assign(&git, branch).unwrap(), slot);
    }
}
