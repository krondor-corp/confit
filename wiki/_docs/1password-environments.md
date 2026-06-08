---
title: 1Password Environments
slug: 1password-environments
order: 10
---

## what environments are

[1Password Environments](https://www.1password.dev/environments) let you store environment variables in 1Password and access them via the CLI:

```bash
op environment read <environment-id>
```

This outputs `KEY=VALUE` pairs. Each environment has a unique ID, and your team can manage variables through the 1Password desktop app without passing `.env` files around.

## source setup (recommended)

`op environment read` dumps the whole environment in one call. Use a [source](sources) to run it once and look up individual fields — no matter how many keys you reference, the command only runs once:

```toml
[vars]
stage = "dev"

[op.environments]
dev  = "env_abc123"
prod = "env_ghi789"

[sources.openv]
load   = "op environment read {op.environments.{vars.stage}}"
secret = true

[db]
password = "openv://DB_PASSWORD"
host     = "openv://DB_HOST"

[app]
api_key  = "openv://API_KEY"
cdn      = "openv://CDN_DOMAIN"
```

```bash
# dev (default) — one op environment read call for all four keys
$ confit show db

# production
$ confit --set stage=prod show db
```

The `secret = true` on the source masks every field from that environment as `***` by default. Use `--reveal` to see real values, or `confit run` to inject them into a process.

## per-stage environments

Map stage names to environment IDs in a `[vars]`-style table, then interpolate into the source `load`:

```toml
[vars]
stage = "dev"

[op.environments]
dev     = "env_abc123"
staging = "env_def456"
prod    = "env_ghi789"

[sources.openv]
load   = "op environment read {op.environments.{vars.stage}}"
secret = true

[db]
password = "openv://DB_PASSWORD"
host     = "openv://DB_HOST"
url      = "postgres://app:{db.password}@{db.host}/mydb"
```

```bash
$ confit --set stage=prod run db -- node server.js
# DB_PASSWORD, DB_HOST, DB_URL injected; op reads prod environment once
```

## mixing with other providers

Sources compose naturally alongside providers for non-environment values:

```toml
[vars]
stage = "dev"

[op.environments]
dev  = "env_abc123"
prod = "env_ghi789"

[sources.openv]
load   = "op environment read {op.environments.{vars.stage}}"
secret = true

[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[db]
password = "openv://DB_PASSWORD"
host     = "tf://db_endpoint"
url      = "postgres://app:{db.password}@{db.host}/mydb"

[app]
api_key  = "openv://API_KEY"
cdn      = "tf://cdn_domain"
base_url = "https://{app.cdn}/v1"
```

```bash
$ confit --set stage=prod run app --upper -- node server.js
# DB_PASSWORD, DB_HOST, DB_URL, API_KEY, CDN, BASE_URL all injected
# op environment read runs once; terraform output runs per-key as needed
```

## provider approach (alternative)

If you need per-key dispatch or want to avoid the source abstraction, the provider approach still works. It runs `op environment read` once per referenced variable:

```toml
[providers.openv]
cmd = "op environment read $(echo {path} | cut -d/ -f1) | sed -n \"s/^$(echo {path} | cut -d/ -f2-)=//p\""
```

The URI format is `openv://ENVIRONMENT_ID/VAR_NAME`:

```toml
[db]
password = "secret://openv://{op.environments.{vars.stage}}/DB_PASSWORD"
host     = "secret://openv://{op.environments.{vars.stage}}/DB_HOST"
```

This runs `op environment read` twice (once per key). For a section with many variables, the [source approach](#source-setup-recommended) is significantly faster.

## shell eval alternative

For one-offs, shell evaluation works without declaring anything:

```toml
[db]
password = "secret://$(op environment read {op.environments.{vars.stage}} | sed -n 's/^DB_PASSWORD=//p')"
```

This works because confit resolves interpolation first, then evaluates `$(...)` commands. Like the provider approach, it runs a separate subprocess per key.
