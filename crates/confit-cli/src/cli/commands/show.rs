use std::collections::HashMap;
use std::fmt;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::Op;

#[derive(Args, Debug, Clone)]
pub struct Show {
    /// Config section to display
    pub section: String,
    /// Output as YAML
    #[arg(long)]
    pub yaml: bool,
    /// Skip provider evaluation
    #[arg(long)]
    pub no_eval: bool,
    /// Prefix lines with 'export' (env format only)
    #[arg(long)]
    pub export: bool,
    /// Uppercase key names (env format only)
    #[arg(long)]
    pub upper: bool,
    /// Wrap output under a top-level key (yaml format only)
    #[arg(long)]
    pub wrap: Option<String>,
    /// Show secret values instead of masking them
    #[arg(long)]
    pub reveal: bool,
}

pub struct ShowOutput(pub String);

impl fmt::Display for ShowOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ShowError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
    #[error("keys collide when uppercased: '{0}' and '{1}' both become '{2}'")]
    KeyCollision(String, String, String),
}

fn check_upper_collisions(pairs: &[confit_core::config::EnvPair]) -> Result<(), ShowError> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for p in pairs {
        let upper = p.key.to_uppercase();
        if let Some(prev) = seen.get(&upper) {
            if *prev != p.key {
                return Err(ShowError::KeyCollision(
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

impl Op for Show {
    type Output = ShowOutput;
    type Error = ShowError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        if self.yaml {
            let output = confit_core::config::yaml_section(
                &self.section,
                !self.no_eval,
                ctx.vars(),
                self.wrap.as_deref(),
                self.reveal,
            )?;
            return Ok(ShowOutput(output));
        }

        let pairs = confit_core::config::env(&self.section, !self.no_eval, ctx.vars())?;
        if self.upper {
            check_upper_collisions(&pairs)?;
        }

        let mut output = String::new();
        for p in &pairs {
            let name = if self.upper {
                p.key.to_uppercase()
            } else {
                p.key.clone()
            };
            let value = if p.secret && !self.reveal {
                "***"
            } else {
                &p.value
            };
            let prefix = if self.export { "export " } else { "" };
            output.push_str(&format!("{prefix}{name}={value}\n"));
        }
        Ok(ShowOutput(output))
    }
}
