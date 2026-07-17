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
        // Whenever [ports] is in scope, also check it against this host:
        // collisions, privileged/out-of-range ports, ports inside the
        // host's ephemeral range, service ports already bound, and ledger
        // corruption. Read-only and cheap, so there's no reason to gate it
        // behind a flag.
        let ports_in_scope = match &self.section {
            None => true,
            Some(s) => s == "ports" || s.starts_with("ports."),
        };
        if ports_in_scope {
            let bc = confit_core::config::build_config(None, ctx.vars())?;
            if let Ok(ports) = confit_core::config::get(&bc.config, "ports") {
                let issues = confit_core::ports::check_host(ports, &bc.config_dir)?;
                for issue in &issues {
                    let line = format!("ports.{}: {}", issue.path, issue.message);
                    match issue.severity {
                        confit_core::ports::Severity::Error => {
                            ui::failure(&line);
                            failures += 1;
                        }
                        confit_core::ports::Severity::Warning => ui::warning(&line),
                    }
                }
                if issues.is_empty() {
                    ui::success("ports: no issues found on this host");
                }
            }
        }

        if failures > 0 {
            return Err(ValidateError::Failures(failures));
        }
        ui::success(&format!("all {} values ok", results.len()));
        Ok(ValidateOutput)
    }
}
