use std::fmt;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::Op;

#[derive(Args, Debug, Clone)]
pub struct Resolve {
    pub path: String,
    /// Skip provider evaluation (op://, tf://, etc.)
    #[arg(long)]
    pub no_eval: bool,
    /// Show secret values instead of masking them
    #[arg(long)]
    pub reveal: bool,
}

pub struct ResolveOutput(pub String);

impl fmt::Display for ResolveOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
}

impl Op for Resolve {
    type Output = ResolveOutput;
    type Error = ResolveError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        let cfg = confit_core::config::Config::build(None, ctx.vars(), None)?;
        let resolved = cfg.resolve(&self.path, !self.no_eval)?;
        let display = if resolved.secret && !self.reveal {
            "***".to_string()
        } else {
            resolved.value
        };
        Ok(ResolveOutput(display))
    }
}
