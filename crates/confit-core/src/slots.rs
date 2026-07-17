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
//! slot (unlike hashing the branch name, which collides by construction
//! once enough branches are live).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// A crude but sufficient mutual-exclusion lock: exclusive-create a sibling
/// `.lock` file, retrying briefly; a lock older than the deadline is assumed
/// stale (its owner crashed) and stolen.
struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(_) => {
                    return Ok(FileLock {
                        path: path.to_path_buf(),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if Instant::now() > deadline {
                        let _ = fs::remove_file(path);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    return Err(Error::Io {
                        path: path.to_path_buf(),
                        source: e,
                    })
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Escape a branch name as a TOML basic string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn load(path: &Path) -> BTreeMap<String, u8> {
    let Ok(content) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(parsed) = content.parse::<toml::Value>() else {
        return BTreeMap::new();
    };
    let Some(table) = parsed.as_table() else {
        return BTreeMap::new();
    };
    table
        .iter()
        .filter_map(|(k, v)| {
            v.as_integer()
                .and_then(|n| u8::try_from(n).ok())
                .map(|n| (k.clone(), n))
        })
        .collect()
}

fn save(path: &Path, slots: &BTreeMap<String, u8>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let mut out = String::new();
    for (branch, slot) in slots {
        out.push_str(&format!("{} = {slot}\n", quote(branch)));
    }
    fs::write(path, out).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Look up (or assign) `branch`'s slot in `1..=MAX_SLOT`, reusing its
/// existing assignment if it already has one. Proactively prunes any branch
/// no longer checked out in any worktree of this repo first, so slots pack
/// as tightly as possible.
pub fn assign(git: &Git, branch: &str) -> Result<u8> {
    let path = state_path(git)?;
    let _lock = FileLock::acquire(&path.with_extension("lock"))?;

    let mut slots = load(&path);

    let live: HashSet<String> = git
        .worktrees()?
        .into_iter()
        .filter_map(|w| w.branch)
        .collect();
    slots.retain(|b, _| live.contains(b) || b == branch);

    if let Some(&slot) = slots.get(branch) {
        save(&path, &slots)?;
        return Ok(slot);
    }

    let used: HashSet<u8> = slots.values().copied().collect();
    let slot = (1..=MAX_SLOT).find(|s| !used.contains(s)).ok_or_else(|| {
        Error::Runtime(format!(
            "no free port slot for branch '{branch}': all {MAX_SLOT} slots are claimed by \
             other active worktrees of this repo (`git worktree list` to see them, \
             `git worktree remove` to free one up)"
        ))
    })?;

    slots.insert(branch.to_string(), slot);
    save(&path, &slots)?;
    Ok(slot)
}

/// Read the ledger without mutating it (diagnostics / integrity checks).
pub fn read(git: &Git) -> Result<BTreeMap<String, u8>> {
    Ok(load(&state_path(git)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(dir.path().join("f.txt"), "x").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap();
        Command::new("git")
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
        dir
    }

    fn add_worktree(main: &Path, wt_path: &Path, branch: &str) {
        assert!(Command::new("git")
            .args([
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                wt_path.to_str().unwrap(),
            ])
            .current_dir(main)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn test_assign_lowest_free_slot() {
        let dir = init_repo();
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
        let dir = init_repo();
        let git = Git::new(dir.path());

        let wt_a = tempfile::tempdir().unwrap();
        let wt_a_path = wt_a.path().join("wt");
        add_worktree(dir.path(), &wt_a_path, "feature/a");
        assert_eq!(assign(&git, "feature/a").unwrap(), 1);

        // Remove the worktree -- feature/a is no longer checked out anywhere.
        assert!(Command::new("git")
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
        let dir = init_repo();
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
}
