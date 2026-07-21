use std::fmt;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::Op;

#[derive(Args, Debug, Clone)]
pub struct Keys {
    pub section: String,
}

pub struct KeysOutput(pub Vec<String>);

impl fmt::Display for KeysOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for k in &self.0 {
            writeln!(f, "{k}")?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeysError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
}

impl Op for Keys {
    type Output = KeysOutput;
    type Error = KeysError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        let cfg = confit_core::config::Config::build(None, ctx.vars(), None)?;
        let keys = cfg.keys(&self.section)?;
        Ok(KeysOutput(keys))
    }
}
