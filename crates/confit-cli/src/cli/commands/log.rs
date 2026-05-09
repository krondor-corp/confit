use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::{NoOutput, Op};
use crate::cli::ui;

#[derive(Args, Debug, Clone)]
pub struct Log {
    pub message: String,
    /// Green success style
    #[arg(long)]
    pub ok: bool,
    /// Red error style
    #[arg(long)]
    pub err: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {}

impl Op for Log {
    type Output = NoOutput;
    type Error = LogError;

    fn run(&self, _ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        if self.ok {
            ui::success(&self.message);
        } else if self.err {
            ui::failure(&self.message);
        } else {
            ui::progress(&self.message);
        }
        Ok(NoOutput)
    }
}
