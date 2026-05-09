use std::convert::Infallible;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::{NoOutput, Op};
use crate::cli::ui;

#[derive(Args, Debug, Clone)]
pub struct Version;

impl Op for Version {
    type Output = NoOutput;
    type Error = Infallible;

    fn run(&self, _ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        eprintln!(
            "{} {}",
            ui::bold("confit"),
            ui::highlight(env!("CARGO_PKG_VERSION"))
        );
        Ok(NoOutput)
    }
}
