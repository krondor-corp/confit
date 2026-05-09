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
