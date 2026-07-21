use std::collections::HashMap;
use std::process;

use clap::Args;

use crate::cli::ctx::Ctx;
use crate::cli::op::{NoOutput, Op};

#[derive(Args, Debug, Clone)]
pub struct Ssh {
    /// Config path to an SSH private key (repeatable)
    #[arg(long, required = true)]
    pub key: Vec<String>,
    /// Command to run
    #[arg(trailing_var_arg = true, required = true)]
    pub command: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("{0}")]
    Core(#[from] confit_core::error::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("ssh-agent failed to start")]
    AgentStart,
    #[error("ssh-add failed: {0}")]
    SshAdd(String),
}

impl Op for Ssh {
    type Output = NoOutput;
    type Error = SshError;

    fn run(&self, ctx: &Ctx) -> Result<Self::Output, Self::Error> {
        let cfg = confit_core::config::Config::build(None, ctx.vars(), None)?;
        let mut openssh_keys = Vec::new();
        for k in &self.key {
            let resolved = cfg.resolve(k, true)?;
            openssh_keys.push(confit_core::ssh::to_openssh(&resolved.value)?);
        }

        let agent_output = process::Command::new("ssh-agent").arg("-s").output()?;
        if !agent_output.status.success() {
            return Err(SshError::AgentStart);
        }
        let agent_stdout = String::from_utf8_lossy(&agent_output.stdout);
        let mut agent_env: HashMap<String, String> = HashMap::new();
        for line in agent_stdout.lines() {
            if let Some((var_eq, _)) = line.split_once(';') {
                if let Some((var, val)) = var_eq.split_once('=') {
                    agent_env.insert(var.trim().to_string(), val.to_string());
                }
            }
        }

        let agent_pid = agent_env.get("SSH_AGENT_PID").cloned();

        let mut child_env: HashMap<String, String> = std::env::vars().collect();
        child_env.extend(agent_env);
        child_env.insert("CONFIT_SSH".into(), "1".into());

        for key_pem in &openssh_keys {
            let add_result = process::Command::new("ssh-add")
                .arg("-")
                .stdin(process::Stdio::piped())
                .stdout(process::Stdio::piped())
                .stderr(process::Stdio::piped())
                .envs(&child_env)
                .spawn()
                .and_then(|mut child| {
                    if let Some(ref mut stdin) = child.stdin {
                        std::io::Write::write_all(stdin, key_pem.as_bytes())?;
                    }
                    child.wait_with_output()
                })?;
            if !add_result.status.success() {
                let stderr = String::from_utf8_lossy(&add_result.stderr);
                cleanup_agent(agent_pid.as_deref());
                return Err(SshError::SshAdd(stderr.trim().to_string()));
            }
        }

        let result = process::Command::new(&self.command[0])
            .args(&self.command[1..])
            .envs(&child_env)
            .status();

        cleanup_agent(agent_pid.as_deref());

        let status = result?;
        process::exit(status.code().unwrap_or(1));
    }
}

fn cleanup_agent(pid: Option<&str>) {
    if let Some(pid_str) = pid {
        let _ = process::Command::new("kill").arg(pid_str).status();
    }
}
