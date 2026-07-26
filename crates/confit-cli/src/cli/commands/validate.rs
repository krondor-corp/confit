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
        let bc = confit_core::config::Config::build(None, ctx.vars(), None)?;
        let mut results = bc.validate();
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
        // If there's a [ports] section, also check it against this host:
        // collisions, privileged/out-of-range ports, ports inside the
        // host's ephemeral range, service ports already bound, and ledger
        // corruption. Read-only and cheap, so there's no reason to gate it
        // behind a flag or section filter.
        if let Some(resolved) = &bc.ports {
            let issues = confit_core::config::check_host(resolved, &bc.config_dir)?;
            for issue in &issues {
                let line = format!("ports.{}: {}", issue.path, issue.message);
                match issue.severity {
                    confit_core::config::Severity::Error => {
                        ui::failure(&line);
                        failures += 1;
                    }
                    confit_core::config::Severity::Warning => ui::warning(&line),
                }
            }
            if issues.is_empty() {
                ui::success("ports: no issues found on this host");
            }
        }

        if failures > 0 {
            return Err(ValidateError::Failures(failures));
        }
        ui::success(&format!("all {} values ok", results.len()));
        Ok(ValidateOutput)
    }
}
