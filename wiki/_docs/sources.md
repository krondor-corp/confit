---
title: Sources
slug: sources
order: 7
---

## what sources do

Sources load a bag of `KEY=VALUE` pairs **once**, parse them, and serve all field lookups from that cached result. This is the right tool when your secret manager does bulk fetches — running the load command once gets you all the variables, and confit looks up individual fields without any extra subprocesses.

Compare to [providers](providers), which run a subprocess **per key**. If you have 8 Terraform outputs or 8 Infisical secrets in one section, providers spawn 8 subprocesses; a source spawns 1.

## declaring a source

Add a `[sources]` section to `confit.toml`. The string shorthand is the `load` command:

```toml
[sources]
infisical = "infisical export --env={vars.stage} --format=dotenv"
```

The table form adds options:

```toml
[sources.infisical]
load   = "infisical export --env={vars.stage} --format=dotenv"
secret = true
```

## referencing source fields

Use `source-name://FIELD_NAME` as a value:

```toml
[vars]
stage = "dev"

[sources]
infisical = "infisical export --env={vars.stage} --format=dotenv"

[tf]
TF_VAR_neon_org_id = "infisical://NEON_ORGANIZATION_ID"
TF_VAR_db_password = "infisical://DB_PASSWORD"
TF_VAR_api_key     = "infisical://API_KEY"
```

All three values are resolved from a single `infisical export` call. A missing field is a hard error — `infisical://NOPE` where `NOPE` isn't in the output reports `Field 'NOPE' not found in source 'infisical'` rather than returning an empty string.

## secret masking

Set `secret = true` on the source to mark every field from it as sensitive. Display commands (`resolve`, `show`, `yaml`) will mask them as `***` by default; use `--reveal` to see real values. `confit run` always passes real values to the process.

```toml
[sources.vault]
load   = "vault kv get -field=data -format=json secret/myapp | jq -r 'to_entries[] | \"\(.key)=\(.value)\""
secret = true

[app]
db_pass  = "vault://DB_PASSWORD"
api_key  = "vault://API_KEY"
```

You can also use the `secret://` prefix on individual references to mark just that field:

```toml
[sources]
config = "myconfig export --format=dotenv"

[app]
public_name = "config://APP_NAME"
db_password = "secret://config://DB_PASSWORD"
```

Both compose: `secret://source://FIELD` marks the field as secret even when the source itself doesn't have `secret = true`.

## template variables

Source `load` commands support `{vars.*}` interpolation, the same as the rest of confit:

| Variable | Description |
|----------|-------------|
| `{vars.stage}` or `{stage}` | Any variable from `[vars]`, `--set`, or `CONFIT_VAR_*` |

The placeholders `{path}` and `{uri}` are **not** available — sources load a whole bag, so there's no per-key path. If you need per-key substitution, use a [provider](providers) instead.

## output format

The `load` command must emit `KEY=VALUE` pairs, one per line. All of these formats are accepted:

```
FOO=bar
export FOO=bar
FOO="bar baz"
FOO='bar baz'
# comment lines and blank lines are ignored
```

This covers `infisical export --format=dotenv`, `op environment read`, and most other secret manager export commands out of the box.

## the env:// built-in

`env://VARNAME` reads directly from the process environment — no declaration needed, no subprocess:

```toml
[app]
debug   = "env://DEBUG"
db_host = "env://DATABASE_HOST"
```

An unset variable is a hard error: `Environment variable 'DATABASE_HOST' is not set`. This makes missing env vars loud rather than silently empty.

`secret://env://VARNAME` works as expected if you need to mask the value.

## lazy loading

A source's `load` command only runs if at least one key references it. Declaring a source in `[sources]` but not using any `source://FIELD` references in your config means the command never runs.

## provider vs source — quick guide

| | Provider | Source |
|---|---|---|
| Command runs | once per key | once per source |
| `{path}` in template | ✓ | ✗ |
| Good for | per-key lookups (Vault, SSM, 1Password items) | bulk exports (Infisical, op environment, .env exports) |

If your command needs to know which variable it's fetching, use a provider. If it dumps everything and you pick fields, use a source.
