---
title: Commands
slug: commands
order: 3
---

All commands accept `--set` to inject runtime variables:

```bash
confit --set region=us-east resolve infra.endpoint
confit --set stage=production resolve credentials.server.ip
```

## resolve

Print the resolved value at a dotted config path.

```
confit resolve <path> [--no-eval] [--reveal]
```

| Option | Description |
|--------|------------|
| `--no-eval` | Skip provider and shell evaluation (`op://`, `$(...)`, etc.) |
| `--reveal` | Show real values for `secret://` wrapped secrets (default: masked as `***`) |

**Examples:**

```bash
# Plain value
$ confit resolve project.name
my-project

# Secret value (masked by default)
$ confit resolve credentials.api_key
***

# Secret value (revealed)
$ confit resolve credentials.api_key --reveal
sk-abc123

# Interpolated + provider-evaluated
$ confit resolve credentials.server.ip
65.108.67.19
```

If the path points to a section rather than a leaf value, confit tells you to use `keys` or `show` instead.

## show

Display a config section as KEY=VALUE pairs or YAML.

```
confit show <section> [--yaml] [--export] [--upper] [--wrap <key>] [--no-eval] [--reveal]
```

| Option | Description |
|--------|------------|
| `--yaml` | Output as YAML instead of KEY=VALUE |
| `--export` | Prefix each line with `export` (env format only) |
| `--upper` | Uppercase key names (env format only) |
| `--wrap <key>` | Wrap output under a top-level key (yaml format only) |
| `--no-eval` | Skip provider and shell evaluation |
| `--reveal` | Show real values for secrets (default: masked as `***`) |

**Examples:**

```bash
# KEY=VALUE format (default)
$ confit show services.web.env
API_KEY=***
BASE_URL=https://example.com
NODE_ENV=production

# Export format for eval
$ eval "$(confit show services.web.env --export --upper --reveal)"

# YAML format
$ confit show services.web --yaml
port: 3000
url: https://example.com
env:
  API_KEY: "***"
  BASE_URL: https://example.com
  NODE_ENV: production

# YAML wrapped under a key (useful for ansible -e @file)
$ confit show credentials.cloud --yaml --wrap credentials_cloud
credentials_cloud:
  access_key: AKIA...
  secret_key: "***"
```

When `--upper` is set, confit detects key collisions (e.g. `api_key` and `API_KEY` both becoming `API_KEY`) and errors instead of silently shadowing.

## keys

List the key names under a config section.

```
confit keys <section>
```

**Examples:**

```bash
$ confit keys services
web
api
worker

$ confit keys credentials
server
cloud
deploy
```

## run

Run a command with a config section injected as environment variables.

```
confit run <section> [--upper] [--no-eval] -- <command...>
```

| Option | Description |
|--------|------------|
| `--upper` | Uppercase key names |
| `--no-eval` | Skip provider and shell evaluation |

The `--` separator is required before the command.

Secret values are always passed as real values to the child process -- `secret://` masking only applies to display commands (`resolve`, `show`).

**Examples:**

```bash
# Run node with service env vars
confit run services.web.env -- node server.js

# Run with uppercased keys
confit run credentials.cloud --upper -- ./deploy.sh
```

confit uses `exec` to replace its own process with the target command. This means the command inherits confit's PID and signal handling works correctly.

## validate

Check that all values in the config can be resolved. Optionally scope to a section.

```
confit validate [section]
```

**Examples:**

```bash
# Validate everything
$ confit validate
✓ project.name
✓ credentials.server.ip
✓ credentials.api_key
✓ credentials.ssh.deploy.private_key
✓ all 4 values ok

# Validate a specific section
$ confit validate credentials
✓ credentials.server.ip
✓ credentials.api_key
✓ credentials.ssh.deploy.private_key
✓ all 3 values ok

# Nested paths work too
$ confit validate credentials.ssh
✓ credentials.ssh.deploy.private_key
✓ all 1 values ok
```

If any value fails to resolve, confit prints the error and exits non-zero.

## ssh

Run a command with SSH keys loaded into a temporary agent.

```
confit ssh --key <config-path> [--key <config-path>...] -- <command...>
```

Resolves private keys from config, starts a temporary ssh-agent, loads the keys, runs the command, and cleans up the agent on exit. Handles 1Password PKCS#8 keys automatically.

**Examples:**

```bash
# Run a command with SSH key available
confit ssh --key credentials.ssh.deploy.private_key -- git pull

# Compose with run for env vars + SSH
confit ssh --key credentials.ssh.admin.private_key -- \
  confit run credentials.cloud --upper -- ./deploy.sh
```

## log

Print a styled message to stderr.

```
confit log <message> [--ok] [--err]
```

| Option | Style |
|--------|-------|
| (none) | Blue info |
| `--ok` | Green with checkmark |
| `--err` | Red with X |

**Examples:**

```bash
confit log "deploying web service"
confit log --ok "deploy complete"
confit log --err "deploy failed"
```

Output goes to stderr so it doesn't interfere with piped stdout.

## update

Update confit to the latest release.

```
confit update [--force]
```

Checks GitHub for the latest release, compares versions, and re-runs the install script to update in place. Detects whether you installed via the install script, cargo, or a source build.

## version

Print the current version.

```
confit version
```
