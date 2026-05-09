use std::path::PathBuf;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::{NoOutput, Op};
use crate::cli::ui;

const TEMPLATE: &str = r#"# confit.toml — config resolver with interpolation and providers
#
# Docs: https://krondor-corp.github.io/confit/docs/install/

# Variables can be overridden with --set or CONFIT_VAR_* env vars.
[vars]
stage = "dev"

# Providers map scheme:// URIs to shell commands.
# {path} is replaced with the URI path, {stage} etc. come from [vars].
#
# [providers.op]
# cmd = "op read {uri}"
#
# [providers.tf]
# cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

# Your config sections go here. Reference vars with {vars.name},
# use $(command) for shell eval, and scheme:// for providers.
#
# [app]
# name = "myapp-{vars.stage}"
# port = 3000
#
# [credentials]
# api_key = "secret://op://vault/item/credential"
"#;

#[derive(Args, Debug, Clone)]
pub struct Init;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("confit.toml already exists")]
    AlreadyExists,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl Op for Init {
    type Output = NoOutput;
    type Error = InitError;

    fn run(&self, _ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        let path = PathBuf::from("confit.toml");
        if path.exists() {
            return Err(InitError::AlreadyExists);
        }
        std::fs::write(&path, TEMPLATE)?;
        ui::success("created confit.toml");
        Ok(NoOutput)
    }
}
