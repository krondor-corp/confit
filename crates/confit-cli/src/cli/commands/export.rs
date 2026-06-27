use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, ValueEnum};

use crate::cli::ctx::Ctx;
use crate::cli::op::Op;
use crate::cli::ui;

#[derive(Args, Debug, Clone)]
pub struct Export {
    /// Config section(s) to export; later sections win on key conflicts
    pub sections: Vec<String>,
    /// Named env profile to export (resolves [env.<name>] in confit.toml)
    #[arg(long)]
    pub profile: Option<String>,
    /// Write to FILE (0600, atomic, gitignore-guarded) instead of stdout
    #[arg(short = 'o', long)]
    pub out: Option<PathBuf>,
    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Dotenv)]
    pub format: Format,
    /// Emit real secret values (required when any value is a secret)
    #[arg(long)]
    pub reveal: bool,
    /// Uppercase key names
    #[arg(long)]
    pub upper: bool,
    /// Prepend a prefix to every key name
    #[arg(long)]
    pub prefix: Option<String>,
    /// Skip provider evaluation
    #[arg(long)]
    pub no_eval: bool,
    /// Write even if the target file is not gitignored
    #[arg(long)]
    pub force: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// KEY='value' lines, sourceable with `set -a; . file`
    Dotenv,
    /// export KEY='value' lines, for `eval "$(...)"`
    Shell,
    /// A JSON object of key/value pairs
    Json,
}

pub struct ExportOutput(pub String);

impl fmt::Display for ExportOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
    #[error("nothing to export: pass --profile <name> or one or more <SECTION>s")]
    NoSource,
    #[error("keys collide when uppercased: '{0}' and '{1}' both become '{2}'")]
    KeyCollision(String, String, String),
    #[error(
        "refusing to emit secret values without --reveal (secret keys: {0}); \
         pass --reveal to materialize real values"
    )]
    SecretsHidden(String),
    #[error(
        "refusing to write secrets to non-gitignored path '{0}'; \
         add it to .gitignore or pass --force"
    )]
    NotGitignored(String),
    #[error("failed to write '{path}': {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn check_upper_collisions(pairs: &[confit_core::config::EnvPair]) -> Result<(), ExportError> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for p in pairs {
        let upper = p.key.to_uppercase();
        if let Some(prev) = seen.get(&upper) {
            if *prev != p.key {
                return Err(ExportError::KeyCollision(
                    prev.to_string(),
                    p.key.clone(),
                    upper,
                ));
            }
        }
        seen.insert(upper, &p.key);
    }
    Ok(())
}

/// Wrap a value in single quotes, escaping embedded single quotes so the
/// result is safe to source from a POSIX shell.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn json_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

enum GitignoreStatus {
    Ignored,
    NotIgnored,
    /// Not a git repo, git unavailable, or otherwise undeterminable.
    Unknown,
}

fn gitignore_status(path: &Path) -> GitignoreStatus {
    match Command::new("git")
        .args(["check-ignore", "-q", "--"])
        .arg(path)
        .output()
    {
        Ok(out) => match out.status.code() {
            Some(0) => GitignoreStatus::Ignored,
            Some(1) => GitignoreStatus::NotIgnored,
            // 128 = not in a git work tree; anything else is unexpected.
            _ => GitignoreStatus::Unknown,
        },
        Err(_) => GitignoreStatus::Unknown,
    }
}

fn write_atomic(path: &Path, content: &str) -> Result<(), ExportError> {
    let io_err = |source| ExportError::Io {
        path: path.to_path_buf(),
        source,
    };
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let tmp = {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "env".to_string());
        let tmp_name = format!(".{file_name}.tmp.{}", std::process::id());
        match parent {
            Some(dir) => dir.join(tmp_name),
            None => PathBuf::from(tmp_name),
        }
    };

    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(io_err)?;
        f.write_all(content.as_bytes()).map_err(io_err)?;
        f.sync_all().map_err(io_err)?;
    }
    // Enforce 0600 regardless of umask, then atomically swap into place.
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).map_err(io_err)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(e)
    })?;
    Ok(())
}

impl Op for Export {
    type Output = ExportOutput;
    type Error = ExportError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        // Compose the list of sections to resolve: the profile (if any) first,
        // then any explicit sections, so explicit sections win on conflict.
        let mut paths: Vec<String> = Vec::new();
        let mut profile_vars = HashMap::new();
        if let Some(name) = &self.profile {
            let profile_path = format!("env.{name}");
            // A profile may pin its own vars via [env.<name>.vars]; layer them in
            // so e.g. `stage` resolves without `--set` at the call site.
            profile_vars = confit_core::config::read_profile_vars(&profile_path)?;
            paths.push(profile_path);
        }
        paths.extend(self.sections.iter().cloned());
        if paths.is_empty() {
            return Err(ExportError::NoSource);
        }

        let pairs = confit_core::config::env_multi_with_vars(
            &paths,
            &profile_vars,
            !self.no_eval,
            ctx.vars(),
        )?;
        if self.upper {
            check_upper_collisions(&pairs)?;
        }

        // Refuse to emit masked junk: a file meant to be sourced must hold real
        // values, so any secret present requires an explicit --reveal.
        if !self.reveal {
            let secret_keys: Vec<&str> = pairs
                .iter()
                .filter(|p| p.secret)
                .map(|p| p.key.as_str())
                .collect();
            if !secret_keys.is_empty() {
                return Err(ExportError::SecretsHidden(secret_keys.join(", ")));
            }
        }

        let prefix = self.prefix.as_deref().unwrap_or("");
        let named: Vec<(String, &str)> = pairs
            .iter()
            .map(|p| {
                let mut name = format!("{prefix}{}", p.key);
                if self.upper {
                    name = name.to_uppercase();
                }
                (name, p.value.as_str())
            })
            .collect();

        let content = render(&named, self.format);

        match &self.out {
            Some(path) => {
                match gitignore_status(path) {
                    GitignoreStatus::NotIgnored if !self.force => {
                        return Err(ExportError::NotGitignored(path.display().to_string()));
                    }
                    GitignoreStatus::Unknown => {
                        ui::warning(&format!(
                            "could not confirm '{}' is gitignored (not a git repo?); writing anyway",
                            path.display()
                        ));
                    }
                    _ => {}
                }
                write_atomic(path, &content)?;
                let keys: Vec<&str> = named.iter().map(|(k, _)| k.as_str()).collect();
                ui::success(&format!(
                    "wrote {} vars to {} ({})",
                    keys.len(),
                    path.display(),
                    keys.join(", ")
                ));
                // Keep secret values out of stdout entirely when writing a file.
                Ok(ExportOutput(String::new()))
            }
            None => Ok(ExportOutput(content)),
        }
    }
}

fn render(named: &[(String, &str)], format: Format) -> String {
    match format {
        Format::Dotenv => named
            .iter()
            .map(|(k, v)| format!("{k}={}\n", shell_quote(v)))
            .collect(),
        Format::Shell => named
            .iter()
            .map(|(k, v)| format!("export {k}={}\n", shell_quote(v)))
            .collect(),
        Format::Json => {
            let body: Vec<String> = named
                .iter()
                .map(|(k, v)| format!("  {}: {}", json_quote(k), json_quote(v)))
                .collect();
            if body.is_empty() {
                "{}\n".to_string()
            } else {
                format!("{{\n{}\n}}\n", body.join(",\n"))
            }
        }
    }
}
