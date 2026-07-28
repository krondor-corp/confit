//! Shared test fixtures for modules that need a real git repo.

use std::path::Path;
use std::process::Command;

/// A fresh `git init`'d repo in a tempdir (unborn HEAD, no commits).
pub fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap()
        .success());
    dir
}

/// Like [`init_repo`], plus one commit -- required before `git worktree add`.
pub fn init_repo_with_commit() -> tempfile::TempDir {
    let dir = init_repo();
    std::fs::write(dir.path().join("f.txt"), "x").unwrap();
    assert!(Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
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
        .unwrap()
        .success());
    dir
}

/// `git worktree add -q -b <branch> <wt_path>` from `main_repo`.
pub fn add_worktree(main_repo: &Path, wt_path: &Path, branch: &str) {
    assert!(Command::new("git")
        .args([
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            wt_path.to_str().unwrap(),
        ])
        .current_dir(main_repo)
        .status()
        .unwrap()
        .success());
}
