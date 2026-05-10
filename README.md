<p align="center">
  <img src="wiki/assets/images/favicon.svg" width="72" height="72" alt="confit logo">
</p>

# confit
<!-- test: verify daemon spawn for confit -->

[![CI](https://github.com/krondor-corp/confit/actions/workflows/test.yml/badge.svg)](https://github.com/krondor-corp/confit/actions/workflows/test.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-violet.svg)](https://opensource.org/licenses/MIT)
[![Docs](https://img.shields.io/badge/docs-confit.krondor.org-violet)](https://krondor-corp.github.io/confit)

**One config file. Interpolation, secrets, and pluggable providers.**

Define your config in a single `confit.toml` with `{variable}` interpolation, `$(shell)` evaluation, and `scheme://` providers for secrets managers and infrastructure tools. Resolve values, inject env vars, load SSH keys — no more sourcing dotfiles or wiring up glue scripts.

**[Read the docs](https://krondor-corp.github.io/confit)** for guides, recipes, and reference.

## Features

- **Interpolation** — Reference values across sections with `{path.to.value}`
- **Shell evaluation** — Inline `$(command)` expressions resolved at runtime
- **Pluggable providers** — Map `scheme://` URIs to any CLI tool (1Password, Terraform, AWS SSM, etc.)
- **Secret masking** — Wrap any value with `secret://` to mask it in output; real values pass through to `run` and `ssh`
- **SSH agent** — Load private keys from config into a temporary agent, exec your command, clean up
- **Config discovery** — Walks up from the current directory to find `confit.toml` automatically
- **Variable precedence** — `[vars]` defaults → `CONFIT_VAR_*` env → `--set` CLI overrides

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/krondor-corp/confit/main/install.sh | bash
```

## Quick Start

```bash
confit init
```

This creates a commented `confit.toml` in the current directory. Edit it:

```toml
[vars]
stage = "dev"

[providers.op]
cmd = "op read {uri}"

[app]
name = "myapp-{vars.stage}"
port = 3000

[credentials]
api_key = "secret://op://vault/item/credential"
```

Then use it:

```bash
confit resolve app.name                  # myapp-dev
confit resolve credentials.api_key       # ***
confit show credentials --export --upper # export API_KEY=***
confit run credentials --upper -- node server.js
```

## Commands

| Command | Description |
|---------|-------------|
| `confit init` | Create a `confit.toml` in the current directory |
| `confit resolve <path>` | Print the resolved value at a dotted config path |
| `confit resolve <path> --reveal` | Show secret values instead of masking |
| `confit resolve <path> --no-eval` | Skip provider and shell evaluation |
| `confit show <section>` | Display a config section as KEY=VALUE pairs |
| `confit show <section> --yaml` | Display as YAML |
| `confit show <section> --export --upper` | Output as `export KEY=VALUE` for `eval` |
| `confit show <section> --reveal` | Unmask secrets in output |
| `confit keys <section>` | List key names under a config section |
| `confit run <section> -- <cmd...>` | Inject section as env vars and exec command |
| `confit run <section> --upper -- <cmd...>` | Uppercase env var names |
| `confit validate` | Check that all config values resolve |
| `confit validate <section>` | Validate a specific section |
| `confit ssh --key <path> -- <cmd...>` | Load SSH keys into a temporary agent and exec |
| `confit log <message>` | Print a styled info message to stderr |
| `confit log --ok <message>` | Green success message |
| `confit log --err <message>` | Red error message |
| `confit update` | Update to the latest release |
| `confit version` | Print version |

All commands accept `--set key=value` (repeatable) to override variables at runtime.

## Providers

Providers map URI schemes to shell commands. Define them in `[providers]`:

```toml
[providers.op]
cmd = "op read {uri}"

[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[providers.aws]
cmd = "aws ssm get-parameter --name {path} --with-decryption --query Parameter.Value --output text"
```

Then reference them in config values:

```toml
[credentials]
api_key = "secret://op://vault/api/credential"
db_password = "secret://aws://prod/db/password"

[infra]
server_ip = "tf://server_ip"
```

Built-in providers: `file://` reads from a local file, `secret://` wraps any value for masking.

## Composing Commands

SSH agent + env var injection in one pipeline:

```bash
confit ssh --key credentials.ssh.deploy -- \
  confit run credentials.cloud --upper -- \
    ./deploy.sh
```

Or the explicit way:

```bash
SERVER_IP=$(confit resolve credentials.server.ip --reveal)
TOKEN=$(confit resolve credentials.api_key --reveal)

confit ssh --key credentials.ssh.deploy -- \
  env SERVER_IP="$SERVER_IP" TOKEN="$TOKEN" \
    ./deploy.sh
```

## Development

```bash
cargo build
cargo test    # 85 tests (42 unit + 43 integration)
```

## Updating

```bash
confit update
```

Or reinstall manually:

```bash
curl -fsSL https://raw.githubusercontent.com/krondor-corp/confit/main/install.sh | bash
```

## Uninstall

```bash
rm ~/.local/bin/confit
```

## License

MIT
