pub mod commands;
pub mod ctx;
pub mod op;
pub mod ui;

use clap::Parser;

use commands::{init, keys, log, resolve, run, show, ssh, update, validate, version};
use op::command_enum;

#[derive(Parser)]
#[command(
    name = "confit",
    about = "Config resolver with interpolation and provider evaluation",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Set runtime var (KEY=VALUE), repeatable
    #[arg(long = "set", global = true)]
    pub set_vars: Vec<String>,

    #[command(subcommand)]
    pub command: Command,
}

command_enum! {
    #[derive(clap::Subcommand)]
    pub enum Command {
        /// Create a confit.toml in the current directory
        Init(init::Init),
        /// Print the resolved value at a dotted config path
        Resolve(resolve::Resolve),
        /// Display a config section as KEY=VALUE or YAML
        Show(show::Show),
        /// List key names under a config section
        Keys(keys::Keys),
        /// Run a command with a config section injected as env vars
        Run(run::Run),
        /// Validate that all config values resolve
        Validate(validate::Validate),
        /// Run a command with SSH keys loaded into a temporary agent
        Ssh(ssh::Ssh),
        /// Print a styled log message to stderr
        Log(log::Log),
        /// Update confit to the latest release
        Update(update::Update),
        /// Print version information
        Version(version::Version),
    }
}
