---
title: 1Password Environments
slug: 1password-environments
order: 9
---

## what environments are

[1Password Environments](https://www.1password.dev/environments) let you store environment variables in 1Password and access them via the CLI:

```bash
op environment read <environment-id>
```

This outputs `KEY=VALUE` pairs. Each environment has a unique ID, and your team can manage variables through the 1Password desktop app without passing `.env` files around.

## provider setup

Declare an `openv` provider that reads a single variable from an environment. The URI format is `openv://ENVIRONMENT_ID/VAR_NAME`:

```toml
[providers.openv]
cmd = "op environment read $(echo {path} | cut -d/ -f1) | sed -n \"s/^$(echo {path} | cut -d/ -f2-)=//p\""
```

This splits `{path}` into the environment ID and variable name, calls `op environment read`, and extracts the matching value.

## per-stage environment IDs

Map each stage to its 1Password Environment ID, then interpolate:

```toml
[vars]
stage = "dev"

[providers.openv]
cmd = "op environment read $(echo {path} | cut -d/ -f1) | sed -n \"s/^$(echo {path} | cut -d/ -f2-)=//p\""

[op.environments]
dev = "env_abc123"
staging = "env_def456"
prod = "env_ghi789"

[db]
password = "secret://openv://{op.environments.{vars.stage}}/DB_PASSWORD"
host = "secret://openv://{op.environments.{vars.stage}}/DB_HOST"
```

```bash
# dev (default)
$ confit resolve db.password
***

# production
$ confit --set stage=prod resolve db.password --reveal
s3cret-prod-pw
```

## mixing with other providers

The point of using confit with environments is composing secrets alongside other sources:

```toml
[providers.openv]
cmd = "op environment read $(echo {path} | cut -d/ -f1) | sed -n \"s/^$(echo {path} | cut -d/ -f2-)=//p\""

[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[vars]
stage = "dev"

[op.environments]
dev = "env_abc123"
prod = "env_ghi789"

[db]
password = "secret://openv://{op.environments.{vars.stage}}/DB_PASSWORD"
host = "tf://db_endpoint"
url = "postgres://app:{db.password}@{db.host}/mydb"

[app]
api_key = "secret://openv://{op.environments.{vars.stage}}/API_KEY"
cdn = "tf://cdn_domain"
base_url = "https://{app.cdn}/v1"
```

```bash
$ confit --set stage=prod run app --upper -- node server.js
# DB_PASSWORD, DB_HOST, DB_URL, API_KEY, CDN, BASE_URL all injected
```

## shell eval alternative

If you only need a few values and don't want the provider, use shell evaluation directly:

```toml
[db]
password = "secret://$(op environment read {op.environments.{vars.stage}} | sed -n 's/^DB_PASSWORD=//p')"
```

This works because confit resolves interpolation first, then evaluates `$(...)` commands. The provider approach is cleaner when you're pulling many variables from the same environment.
