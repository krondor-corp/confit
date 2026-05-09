---
title: Shell Eval
slug: shell-eval
order: 5
---

## inline shell expressions

Any value can contain `$(...)` to run a shell command inline. The command's stdout becomes the value.

```toml
[project]
container_runtime = "$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)"
hostname = "$(hostname -s)"
git_sha = "$(git rev-parse --short HEAD)"
```

```bash
$ confit resolve project.container_runtime
podman

$ confit resolve project.hostname
devbox

$ confit resolve project.git_sha
a1b2c3d
```

## how it works

Shell eval runs after `{ref}` interpolation but before provider evaluation. This means you can use interpolated values inside shell expressions:

```
raw TOML → {ref} interpolation → $(...) shell eval → scheme:// providers → final value
```

```toml
[project]
data_dir = "data"
row_count = "$(wc -l < {project.data_dir}/input.csv)"
```

The `{project.data_dir}` reference is resolved first, then the shell command runs with the interpolated path.

## working directory

Shell expressions run from the directory containing `confit.toml`, just like provider commands. Relative paths work correctly regardless of where you invoke confit.

## skipping eval

`--no-eval` skips both shell expressions and provider evaluation:

```bash
$ confit resolve project.container_runtime --no-eval
$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)
```

## error handling

If a shell expression exits non-zero, confit reports the error:

```bash
$ confit resolve project.broken
Error: Shell eval failed: $(bad-command): bad-command: command not found
```

## when to use shell eval vs providers

Use **shell eval** for one-off commands where declaring a provider would be overkill:

```toml
container_runtime = "$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)"
```

Use **providers** when multiple values share the same tool and you want a reusable pattern:

```toml
[providers.op]
cmd = "op read {uri}"

[credentials]
api_key = "op://vault/item/field"
db_pass = "op://vault/db/password"
```
