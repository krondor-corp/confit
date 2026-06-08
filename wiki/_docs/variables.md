---
title: Variables
slug: variables
order: 8
---

## what variables are

Variables are runtime values that can be set from multiple sources and referenced in your config via `{vars.name}`. They're useful for values that change between invocations -- like which stage to deploy to, or which region to target.

## setting variables

Variables come from three sources, in order of precedence (highest wins):

| Source | Example | Precedence |
|--------|---------|-----------|
| `--set` | `confit --set stage=prod resolve ...` | Highest |
| `CONFIT_VAR_*` env | `CONFIT_VAR_STAGE=prod confit resolve ...` | Medium |
| `[vars]` in TOML | `stage = "dev"` in confit.toml | Lowest |

### in confit.toml

Declare defaults in the `[vars]` section:

```toml
[vars]
stage = "dev"
region = "us-east"
```

### from environment

Set `CONFIT_VAR_<NAME>` (uppercased):

```bash
export CONFIT_VAR_STAGE=production
export CONFIT_VAR_REGION=eu-west
```

Environment variables override TOML defaults.

### from the CLI

Use `--set` (repeatable):

```bash
confit --set stage=production --set region=eu-west resolve infra.endpoint
```

CLI flags override both env and TOML.

## using variables in config

Reference variables with `{vars.name}`:

```toml
[vars]
stage = "dev"

[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[infra]
endpoint = "tf://api_endpoint"
db_host = "tf://db_host"
```

Variables are available in two places:
- **Config interpolation:** `{vars.stage}` in any TOML value
- **Provider templates:** `{stage}` in provider `cmd` strings (note: no `vars.` prefix in provider templates)

## error messages

If you reference a variable that isn't set, confit tells you how to fix it:

```bash
$ confit resolve infra.endpoint
Error: Variable 'stage' is not set. Define it in [vars] in confit.toml,
pass --set stage=VALUE, or set CONFIT_VAR_STAGE
```

This error only fires when you actually try to resolve a path that needs the variable. Reading `project.name` won't fail just because `stage` is unset.
