use std::fmt;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::Op;
use crate::cli::ui;

#[derive(Args, Debug, Clone)]
pub struct Validate {
    /// Limit validation to a specific section
    pub section: Option<String>,
}

pub struct ValidateOutput;

impl fmt::Display for ValidateOutput {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValidateError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
    #[error("{0} value(s) failed to resolve")]
    Failures(usize),
}

impl Op for Validate {
    type Output = ValidateOutput;
    type Error = ValidateError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        let mut results = confit_core::config::validate(ctx.vars())?;
        if let Some(ref s) = self.section {
            let prefix = format!("{s}.");
            results.retain(|(p, _, _)| p == s || p.starts_with(&prefix));
        }
        let mut failures = 0;
        for (path, ok, err) in &results {
            if *ok {
                ui::success(path);
            } else {
                ui::failure(&format!("{path}: {err}"));
                failures += 1;
            }
        }
        if failures > 0 {
            return Err(ValidateError::Failures(failures));
        }
        ui::success(&format!("all {} values ok", results.len()));
        Ok(ValidateOutput)
    }
}
