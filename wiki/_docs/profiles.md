---
title: Env Profiles
slug: profiles
order: 8
---

A working dev env is rarely a single section. Secrets live in `credentials.app`,
datastore URLs in `accessories.postgres` / `accessories.redis`, and a few values
are plain literals. An **env profile** composes all of those into one named,
exportable env — declared once in config instead of re-derived in each `bin/*.sh`.

## declaring a profile

A profile is an `[env.<name>]` table. Each key is an environment variable name;
each value is a string that goes through the full resolution pipeline —
[interpolation](/docs/interpolation/), [shell eval](/docs/shell-eval/),
[providers](/docs/providers/), and [sources](/docs/sources/) — so it can draw
from any section, provider, source bag, or literal.

```toml
[credentials.app]
service_secret              = "secret://op://Private/app/service_secret"
google_o_auth_client_id     = "op://Private/app/google_id"
google_o_auth_client_secret = "secret://op://Private/app/google_secret"
anthropic_api_key           = "secret://op://Private/app/anthropic"

[accessories.postgres]
url = "secret://op://Private/postgres/url"

[accessories.redis]
url = "redis://localhost:6379"

# A named, exportable env profile
[env.dev]
SERVICE_SECRET              = "{credentials.app.service_secret}"
POSTGRES_URL                = "{accessories.postgres.url}"
REDIS_URL                   = "{accessories.redis.url}"
GOOGLE_O_AUTH_CLIENT_ID     = "{credentials.app.google_o_auth_client_id}"
GOOGLE_O_AUTH_CLIENT_SECRET = "{credentials.app.google_o_auth_client_secret}"
ANTHROPIC_API_KEY           = "{credentials.app.anthropic_api_key}"
HOST_NAME                   = "http://localhost:8000"
```

A `{ref}` to a `secret://` value carries the secret flag through, so
`SERVICE_SECRET` above is still treated as a secret (masked by `show`, and
requiring `--reveal` from `export`).

Profile keys can reference a [`[sources]`](/docs/sources/) bag directly, which is
the efficient way to pull many values from one bulk export — the source's `load`
command runs once for the whole profile, not once per key:

```toml
[sources]
infisical = "infisical export --env={vars.stage} --format=dotenv"

[env.dev]
DATABASE_URL = "infisical://DATABASE_URL"
API_KEY      = "secret://infisical://API_KEY"
```

## pinning vars

A profile can pin its own [variables](/docs/variables/) under a `vars` key. This
is handy when the profile's values depend on a `stage` (or any var) that you'd
otherwise have to pass with `--set` every time. Use TOML dotted keys so the pins
sit right at the top of the profile:

```toml
[providers.op]
cmd = "op read {uri}"

[credentials.app]
service_secret = "secret://op://{stage}/app/service_secret"

[env.dev]
vars.stage = "development"
SERVICE_SECRET = "{credentials.app.service_secret}"
```

```bash
# no --set needed — the profile pins stage=development
confit export --profile dev --reveal --out .env.dev
```

`vars` is a sub-table, so it's never emitted as an env var. Pinned vars slot into
the normal [precedence](/docs/variables/#setting-variables) as a default —
overriding the global `[vars]` section, but still overridden by `CONFIT_VAR_*`
and `--set`:

```
[vars]  <  profile vars.*  <  CONFIT_VAR_*  <  --set
```

```bash
# the profile pins development, but --set still wins
confit export --profile dev --set stage=staging --reveal
```

## using a profile

Export the whole profile with [`confit export`](/docs/commands/#export):

```bash
# materialize once (one 1Password unlock) into a gitignored file
confit --set stage=development export --profile dev --reveal --out web/py/.env.dev

# or stream it for eval
eval "$(confit export --profile dev --reveal --format shell)"
```

A profile is just a section under `[env]`, so the other commands work on it too —
`confit show env.dev` (masked) or `confit keys env.dev` to list its variables.

## composing without a profile

If you don't want to name a profile, `export` also accepts multiple sections
directly, merging them left-to-right (later wins on key conflicts):

```bash
confit export credentials.app accessories.postgres --reveal --out .env.dev
```

Profiles are the better home once a composition stabilizes: the env lives in
config, version-controlled and shared, instead of being reassembled by hand at
every call site.
