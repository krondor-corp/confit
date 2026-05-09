use std::collections::HashMap;
use std::process;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::{NoOutput, Op};

#[derive(Args, Debug, Clone)]
pub struct Run {
    /// Config section to inject as env vars
    pub section: String,
    /// Skip provider evaluation
    #[arg(long)]
    pub no_eval: bool,
    /// Uppercase key names
    #[arg(long)]
    pub upper: bool,
    /// Command to run
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
    #[error("exec failed: {0}")]
    Exec(std::io::Error),
    #[error("keys collide when uppercased: '{0}' and '{1}' both become '{2}'")]
    KeyCollision(String, String, String),
}

fn check_upper_collisions(pairs: &[confit_core::config::EnvPair]) -> Result<(), RunError> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for p in pairs {
        let upper = p.key.to_uppercase();
        if let Some(prev) = seen.get(&upper) {
            if *prev != p.key {
                return Err(RunError::KeyCollision(
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

impl Op for Run {
    type Output = NoOutput;
    type Error = RunError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        use std::os::unix::process::CommandExt;

        let pairs = confit_core::config::env(&self.section, !self.no_eval, ctx.vars())?;
        if self.upper {
            check_upper_collisions(&pairs)?;
        }
        let mut cmd = process::Command::new(&self.command[0]);
        cmd.args(&self.command[1..]);
        for p in &pairs {
            let name = if self.upper {
                p.key.to_uppercase()
            } else {
                p.key.clone()
            };
            cmd.env(name, &p.value);
        }
        Err(RunError::Exec(cmd.exec()))
    }
}
