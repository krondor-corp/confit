---
title: Dev Ports
slug: dev-ports
order: 9
---

A `[ports]` section gives a project a fixed 100-port band, fixed ports for
shared infra within it, and per-branch ports for HTTP services -- so
multiple git worktrees of the same project can run concurrently against one
shared infra stack without colliding.

```toml
[ports]
band = 4300

[ports.infra]
postgres = 0
redis = 1

[ports.services]
app = 50
site = 70
```

confit expands this at load time, before any of the values are used
elsewhere, into plain values alongside `band`:

- `ports.slug` -- the current branch, lowercased, cleaned to `[a-z0-9-]`,
  and length-capped, safe to use in a database name, bucket name, or
  container name
- `ports.slot` -- `0` on a primary branch (`main`/`master` by default),
  else the lowest integer in `1..9` not already claimed by another branch
  checked out in this repo right now (see below)
- `ports.infra.<name>` -- `band + offset`, fixed regardless of branch
- `ports.services.<name>` -- `band + lane + slot`

### How the slot is assigned

Each branch's slot lives in a small ledger confit keeps at
`<git-common-dir>/confit/ports.toml` (inside `.git`, so it's per-clone,
never committed, and shared by every `git worktree` of the repo). Each time
confit resolves `[ports]`:

1. It lists every worktree currently checked out (`git worktree list`) and
   drops any ledger entry whose branch isn't one of them -- a removed or
   merged worktree's slot is freed immediately, not left stale.
2. If the current branch already has an entry, it's reused (stable across
   runs while the worktree exists).
3. Otherwise it takes the lowest slot not currently held by another live
   branch, records it, and returns it.

This packs slots tightly: three concurrent worktrees get `1, 2, 3`; remove
the middle one and the next new worktree reclaims `2`, not `4`.

> There are only 9 non-primary slots per repo. Check out a 10th branch at
> the same time and confit tells you plainly there's no slot left, rather
> than quietly handing out a port someone else is already using.

Because these are ordinary resolved values, reference them from anywhere in
confit.toml the same way you'd reference `{vars.*}`:

```toml
[db]
url = "postgres://localhost:{ports.infra.postgres}/myapp_{ports.slug}"

[services.app.env]
PORT = "{ports.services.app}"
DATABASE_URL = "{db.url}"
```

```bash
$ confit resolve ports.services.app
4350                          # on main: band(4300) + lane(50) + slot(0)

$ git worktree add ../widgets -b feature/widgets
$ cd ../widgets && confit resolve ports.services.app
4351                          # first concurrent worktree: slot 1 (lowest free)

$ confit resolve ports.infra.postgres
4300                          # infra is fixed -- same on every branch

$ confit show ports --yaml
band: 4300
slug: feature-widgets
slot: 1
infra:
  postgres: 4300
  redis: 4301
services:
  app: 4351
  site: 4371
```

## Choosing a band

Pick a base per project, a multiple of 100, in a high quiet range below the
ephemeral port floor (below ~48000). A port then tells you what it is
at a glance -- keep a short registry of "which project claimed which base"
somewhere shared (a wiki page, a top comment in each project's
`confit.toml`).

## Spacing lanes

A service's port sweeps `lane` through `lane + 9` as branches come and go,
so lanes must sit at least 10 apart from each other (and from any fixed
infra offset) or two branches can land on the same port. `confit validate`
errors when a `[ports]` layout breaks this.

## Primary branches

By default `main` and `master` get slot `0`. Override with
`ports.primary_branches` if your trunk branch is named something else:

```toml
[ports]
band = 4300
primary_branches = ["trunk"]
```

## Requirements

`[ports]` needs an integer `band`; `ports.infra.*` values must be integer
offsets and `ports.services.*` values integer lanes -- all checked when the
config loads. Actually resolving ports (a `{ports.*}` ref, `confit resolve
ports...`, `confit validate`) additionally requires a git working tree with
a branch checked out: confit shells out to `git` to read the current
branch, list worktrees, and read/write the slot ledger. Commands that never
touch ports work fine without git.

## Checking a band against this host

Whenever a `[ports]` section is in scope, `confit validate` (and
`confit validate ports`) also checks the resolved values against the
machine you're running on -- no flag needed: two names resolving to the
same port, lanes closer than 10 apart, privileged (`<1024`) ports, ports
inside this host's OS-assigned ephemeral range (which risks the OS handing
one out for an unrelated outbound connection), service ports another
process already has bound, and a corrupted or hand-edited slot ledger.
Scoping validation elsewhere (`confit validate db`) skips the ports check:

```bash
$ confit validate
✓ ports.band
✓ ports.slug
...
✓ ports: no issues found on this host
✓ all 8 values ok
```
