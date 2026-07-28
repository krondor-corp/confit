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

### declaration is required

Every name passed via `--set` or `CONFIT_VAR_*` must already exist as a key
in `[vars]` (or be pinned by the active [profile](/docs/profiles/)'s own
`vars` table) -- checked immediately, before anything is resolved:

```bash
$ confit --set stagee=production resolve infra.endpoint
Error: 'stagee' is not declared in [vars]; add it to confit.toml's
[vars] section, or check for a typo
```

This exists to catch a mistyped flag or a stray `CONFIT_VAR_*` left over
from another project -- both would otherwise silently do nothing. Declare a
default even if every real invocation overrides it:

```toml
[vars]
stage = ""
```

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

If you reference a variable that isn't declared, confit tells you how to
fix it:

```bash
$ confit resolve infra.endpoint
Error: Variable 'stage' is not declared. Add it to [vars] in confit.toml
(a default or ""), then override with --set stage=VALUE or CONFIT_VAR_STAGE
```

This particular error only fires when you actually try to resolve a path
that needs the variable -- reading `project.name` won't fail just because
`stage` is undeclared. That's different from an *undeclared*
`--set`/`CONFIT_VAR_*` name, which fails immediately (see above) regardless
of which path you resolve.
