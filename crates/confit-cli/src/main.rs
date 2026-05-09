mod cli;

use std::process;

use clap::Parser;

use cli::ctx::Ctx;
use cli::op::Op;
use cli::ui;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    let ctx = match Ctx::from_cwd(&cli.set_vars) {
        Ok(ctx) => ctx,
        Err(e) => {
            ui::failure(&e.to_string());
            process::exit(1);
        }
    };

    match cli.command.run(&ctx) {
        Ok(output) => {
            let s = output.to_string();
            if !s.is_empty() {
                print!("{s}");
            }
        }
        Err(e) => {
            ui::print_error(&e);
            process::exit(1);
        }
    }
}
