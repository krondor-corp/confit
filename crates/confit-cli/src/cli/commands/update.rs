use std::path::PathBuf;
use std::process::{Command, Stdio};

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::{NoOutput, Op};
use crate::cli::ui;

const GITHUB_REPO: &str = "krondor-corp/confit";
const INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/krondor-corp/confit/main/install.sh";

#[derive(Args, Debug, Clone)]
pub struct Update {
    /// Force reinstall even if already up to date
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("{0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallMethod {
    Script(PathBuf),
    Cargo(PathBuf),
    Source(PathBuf),
    Unknown(PathBuf),
}

impl InstallMethod {
    fn label(&self) -> &str {
        match self {
            InstallMethod::Script(_) => "install script (~/.local/bin)",
            InstallMethod::Cargo(_) => "cargo install (~/.cargo/bin)",
            InstallMethod::Source(_) => "source build (target/)",
            InstallMethod::Unknown(_) => "unknown",
        }
    }
}

impl Op for Update {
    type Output = NoOutput;
    type Error = UpdateError;

    fn run(&self, _ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        let method = detect_installation()?;
        let current = env!("CARGO_PKG_VERSION");

        ui::progress(&format!("confit {} ({})", current, method.label()));

        let latest = fetch_latest_version()?;
        let newer = is_newer(&latest, current);

        if !newer && !self.force {
            ui::success(&format!("already up to date ({})", current));
            return Ok(NoOutput);
        }

        if newer {
            ui::progress(&format!("{} → {}", ui::dim(current), ui::highlight(&latest)));
        } else {
            ui::progress("forcing reinstall");
        }

        match method {
            InstallMethod::Script(_) => run_install_script()?,
            InstallMethod::Cargo(_) | InstallMethod::Source(_) => {
                eprintln!();
                ui::warning("development build detected — installing release to ~/.local/bin");
                run_install_script()?;
            }
            InstallMethod::Unknown(ref path) => {
                eprintln!();
                ui::warning(&format!("unknown install method: {}", path.display()));
                eprintln!("  run manually: curl -fsSL {} | bash", INSTALL_SCRIPT_URL);
                return Ok(NoOutput);
            }
        }

        ui::success("updated");
        Ok(NoOutput)
    }
}

fn detect_installation() -> Result<InstallMethod, UpdateError> {
    let exe = std::env::current_exe()?;
    let s = exe.to_string_lossy();
    if s.contains("/.local/bin/") {
        Ok(InstallMethod::Script(exe))
    } else if s.contains("/.cargo/bin/") {
        Ok(InstallMethod::Cargo(exe))
    } else if s.contains("/target/") {
        Ok(InstallMethod::Source(exe))
    } else {
        Ok(InstallMethod::Unknown(exe))
    }
}

fn fetch_latest_version() -> Result<String, UpdateError> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            &format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest"),
        ])
        .output()?;

    if !output.status.success() {
        return Err(UpdateError::Failed(format!(
            "failed to fetch latest version: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    for line in body.lines() {
        if line.contains("\"tag_name\"") {
            if let Some(i) = line.find(':') {
                let v = line[i + 1..].trim().trim_end_matches(',').trim_matches('"');
                return Ok(v.trim_start_matches('v').to_string());
            }
        }
    }

    Err(UpdateError::Failed("could not parse version from GitHub response".to_string()))
}

fn run_install_script() -> Result<(), UpdateError> {
    let status = Command::new("bash")
        .args(["-c", &format!("curl -fsSL {INSTALL_SCRIPT_URL} | bash")])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        return Err(UpdateError::Failed("install script failed".to_string()));
    }
    Ok(())
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let p: Vec<u32> = v
            .trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        (*p.first().unwrap_or(&0), *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0))
    };
    parse(latest) > parse(current)
}
