//! Every place confit shells out to `git` goes through here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::{Error, Result};

/// Whether a path is matched by a gitignore rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreStatus {
    Ignored,
    NotIgnored,
    /// Not a git repo, git unavailable, or otherwise undeterminable.
    Unknown,
}

/// A `git` invocation scoped to one working directory.
pub struct Git {
    cwd: PathBuf,
}

impl Git {
    pub fn new(cwd: impl AsRef<Path>) -> Self {
        Git {
            cwd: cwd.as_ref().to_path_buf(),
        }
    }

    fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.cwd)
            .output()
    }

    /// The current branch name.
    ///
    /// Tries `git symbolic-ref --short HEAD` first (works on an unborn HEAD,
    /// e.g. a freshly `git init`'d repo with no commits yet), falling back to
    /// `git rev-parse --abbrev-ref HEAD` for a detached HEAD.
    pub fn current_branch(&self) -> Result<String> {
        let symbolic = self
            .run(&["symbolic-ref", "--short", "-q", "HEAD"])
            .map_err(|e| Error::Runtime(format!("git symbolic-ref --short HEAD: {e}")))?;
        if symbolic.status.success() {
            return Ok(String::from_utf8_lossy(&symbolic.stdout).trim().to_string());
        }

        let output = self
            .run(&["rev-parse", "--abbrev-ref", "HEAD"])
            .map_err(|e| Error::Runtime(format!("git rev-parse --abbrev-ref HEAD: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Runtime(format!(
                "Could not determine the current git branch (are you in a git repo?): {}",
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Whether `path` is matched by a gitignore rule, via `git check-ignore`.
    pub fn check_ignore(&self, path: &Path) -> IgnoreStatus {
        match Command::new("git")
            .args(["check-ignore", "-q", "--"])
            .arg(path)
            .current_dir(&self.cwd)
            .output()
        {
            Ok(out) => match out.status.code() {
                Some(0) => IgnoreStatus::Ignored,
                Some(1) => IgnoreStatus::NotIgnored,
                // 128 = not in a git work tree; anything else is unexpected.
                _ => IgnoreStatus::Unknown,
            },
            Err(_) => IgnoreStatus::Unknown,
        }
    }

    /// The repo's common `.git` directory (absolute), shared by the main
    /// checkout and every `git worktree add`'d worktree of the same repo.
    /// The right place for machine-local, per-repo state that must be
    /// visible from any worktree but never committed.
    pub fn common_dir(&self) -> Result<PathBuf> {
        let output = self
            .run(&["rev-parse", "--git-common-dir"])
            .map_err(|e| Error::Runtime(format!("git rev-parse --git-common-dir: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Runtime(format!(
                "git rev-parse --git-common-dir failed (are you in a git repo?): {}",
                stderr.trim()
            )));
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let path = PathBuf::from(raw);
        Ok(if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        })
    }

    /// Every worktree of this repo (`git worktree list --porcelain`),
    /// including the current one.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let output = self
            .run(&["worktree", "list", "--porcelain"])
            .map_err(|e| Error::Runtime(format!("git worktree list: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Runtime(format!(
                "git worktree list failed: {}",
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_worktree_list(&stdout))
    }
}

/// One entry from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// `None` for a detached HEAD.
    pub branch: Option<String>,
}

fn parse_worktree_list(porcelain: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;

    for line in porcelain.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(p) = path.take() {
                worktrees.push(Worktree {
                    path: p,
                    branch: branch.take(),
                });
            }
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        }
    }
    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        dir
    }

    #[test]
    fn test_current_branch_unborn_head() {
        let dir = init_repo();
        let branch = Git::new(dir.path()).current_branch().unwrap();
        assert!(!branch.is_empty());
    }

    #[test]
    fn test_current_branch_not_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = Git::new(dir.path()).current_branch();
        assert!(result.is_err());
    }

    #[test]
    fn test_check_ignore() {
        let dir = init_repo();
        std::fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();
        let git = Git::new(dir.path());
        assert_eq!(git.check_ignore(Path::new(".env")), IgnoreStatus::Ignored);
        assert_eq!(
            git.check_ignore(Path::new("confit.toml")),
            IgnoreStatus::NotIgnored
        );
    }

    #[test]
    fn test_check_ignore_not_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        assert_eq!(
            git.check_ignore(Path::new("whatever")),
            IgnoreStatus::Unknown
        );
    }

    #[test]
    fn test_common_dir_is_shared_across_worktrees() {
        let dir = init_repo();
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

        let extra = tempfile::tempdir().unwrap();
        let wt_path = extra.path().join("wt");
        assert!(Command::new("git")
            .args([
                "worktree",
                "add",
                "-q",
                "-b",
                "feature/x",
                wt_path.to_str().unwrap(),
            ])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());

        let main_common = Git::new(dir.path()).common_dir().unwrap();
        let wt_common = Git::new(&wt_path).common_dir().unwrap();
        assert_eq!(
            main_common.canonicalize().unwrap(),
            wt_common.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_parse_worktree_list() {
        let porcelain = "worktree /repo/main\nHEAD abc123\nbranch refs/heads/main\n\n\
             worktree /repo/wt-feature\nHEAD def456\nbranch refs/heads/feature/x\n\n\
             worktree /repo/wt-detached\nHEAD 789abc\ndetached\n";
        let worktrees = parse_worktree_list(porcelain);
        assert_eq!(worktrees.len(), 3);
        assert_eq!(worktrees[0].path, PathBuf::from("/repo/main"));
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(worktrees[1].branch.as_deref(), Some("feature/x"));
        assert_eq!(worktrees[2].branch, None);
    }

    #[test]
    fn test_worktrees_single_repo() {
        let dir = init_repo();
        let worktrees = Git::new(dir.path()).worktrees().unwrap();
        assert_eq!(worktrees.len(), 1);
        assert_eq!(
            worktrees[0].path.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_worktrees_multiple() {
        let dir = init_repo();
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

        let extra = tempfile::tempdir().unwrap();
        let wt_path = extra.path().join("wt");
        assert!(Command::new("git")
            .args([
                "worktree",
                "add",
                "-q",
                "-b",
                "feature/other",
                wt_path.to_str().unwrap(),
            ])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());

        let worktrees = Git::new(dir.path()).worktrees().unwrap();
        assert_eq!(worktrees.len(), 2);
        assert!(worktrees
            .iter()
            .any(|w| w.branch.as_deref() == Some("feature/other")));
    }
}
