---
title: Quickstart
slug: quickstart
order: 2
---

## create a config

Create `confit.toml` in your project root:

```toml
[project]
name = "my-project"
domain = "example.com"

[providers.op]
cmd = "op read {uri}"

[credentials]
api_key = "secret://op://my-vault/api-key/credential"

[services.web]
port = 3000
url = "https://{project.domain}"

[services.web.env]
API_KEY = "{credentials.api_key}"
BASE_URL = "{services.web.url}"
NODE_ENV = "production"
```

Values in `{curly braces}` are interpolation references -- they resolve to other values in the same config file.

Values starting with `op://` (or any declared provider scheme) are evaluated at resolve time by calling the provider command.

Values wrapped in `secret://` are marked sensitive and masked in display output.

## read a value

```bash
$ confit resolve project.name
my-project

$ confit resolve services.web.url
https://example.com
```

## secrets are masked by default

```bash
$ confit resolve credentials.api_key
***

$ confit resolve credentials.api_key --reveal
sk-abc123
```

## skip provider evaluation

```bash
$ confit resolve credentials.api_key --no-eval
secret://op://my-vault/api-key/credential
```

The `--no-eval` flag resolves `{ref}` interpolation but skips calling provider commands. Useful for debugging or when you don't have provider access.

## export environment variables

```bash
$ confit show services.web.env --export --upper
export API_KEY=***
export BASE_URL=https://example.com
export NODE_ENV=production
```

## materialize an env file

For a persistent env you can source from any fresh shell — handy for coding
agents that run each command in a new process — use [`confit export`](/docs/commands/#export):

```bash
# write a gitignored dotenv file (one unlock, real values, mode 0600)
$ confit export services.web.env --reveal --out .env.local
✓ wrote 3 vars to .env.local (API_KEY, BASE_URL, NODE_ENV)

# thereafter, in any shell — no re-auth
$ set -a; . .env.local; set +a
```

Compose several sections into one file by listing them (later wins), or declare
the composition once as an [env profile](/docs/profiles/) and export `--profile`.

## run a command with injected env

```bash
confit run --section services.web.env -- node server.js
```

This resolves the section, injects all leaf values as environment variables (secrets are passed as real values), and replaces the confit process with `node server.js` via `execvpe`.

## list keys

```bash
$ confit keys services
web
```

Useful for scripting over services:

```bash
for svc in $(confit keys services); do
  echo "deploying $svc..."
done
```
