---
title: Providers
slug: providers
order: 6
---

## what providers do

Providers evaluate `scheme://` URIs into real values by running shell commands. After `{ref}` interpolation, confit scans the result for URIs that match a declared provider and calls the provider's command to get the final value.

## declaring providers

Add a `[providers]` section to `confit.toml`:

```toml
[providers.op]
cmd = "op read {uri}"

[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[providers.aws]
cmd = "aws ssm get-parameter --name {path} --with-decryption --query Parameter.Value --output text"
```

The key after `providers.` is the scheme name. A value starting with `op://` dispatches to `providers.op`, `tf://` dispatches to `providers.tf`, and so on.

## template variables

Provider `cmd` templates have access to:

| Variable | Description |
|----------|-------------|
| `{uri}` | The full original value (e.g. `op://vault/item/field`) |
| `{path}` | Everything after `scheme://` (e.g. `vault/item/field`) |
| Any runtime var | Set via `--set`, `CONFIT_VAR_*`, or `[vars]` in TOML |

## how evaluation works

Given this config:

```toml
[providers.op]
cmd = "op read {uri}"

[vaults]
prod = "my-vault"

[credentials]
api_key = "op://{vaults.prod}/api-key/credential"
```

When you run `confit resolve credentials.api_key`:

1. **Raw:** `"op://{vaults.prod}/api-key/credential"`
2. **Interpolate:** `"op://my-vault/api-key/credential"`
3. **Provider match:** `op://` matches `providers.op`
4. **Template:** `cmd = "op read {uri}"` becomes `op read op://my-vault/api-key/credential`
5. **Execute:** runs the shell command, captures stdout
6. **Result:** the secret value

## runtime vars in providers

Provider templates can reference runtime variables. This is how you parameterize providers for different environments:

```toml
[providers.tf]
cmd = "terraform -chdir=iac/stages/{stage} output -raw {path}"

[infra]
server_ip = "tf://server_ip"
```

```bash
$ confit --set stage=production resolve infra.server_ip
# runs: terraform -chdir=iac/stages/production output -raw server_ip
65.108.67.19
```

If a provider template uses a variable that isn't set, confit errors with a clear message:

```
Error: Provider 'tf' requires '{stage}' but it was not set
(resolving 'tf://server_ip'). Pass --set stage=VALUE or set CONFIT_VAR_STAGE
```

## working directory

Provider commands always run from the directory containing `confit.toml`, not from your shell's current directory. This means relative paths in provider `cmd` templates work correctly no matter where you invoke confit from:

```toml
[providers.tf]
cmd = "terraform -chdir=stages/{stage} output -raw {path}"
```

```bash
# Both of these resolve identically — terraform sees stages/ relative to confit.toml
$ cd ~/myproject && confit --set stage=prod resolve infra.ip
$ cd ~/myproject/src/deep/nested && confit --set stage=prod resolve infra.ip
```

confit finds `confit.toml` by walking up from your current directory through every parent. So you can run it from any subdirectory of your project.

## no providers = no evaluation

If there's no `[providers]` section in your TOML, values with `scheme://` prefixes are left as literal strings. You opt in to provider evaluation by declaring them.

If a value has a `scheme://` prefix but no matching provider is declared, it's also left as-is. This is not an error.

## built-in schemes

### file://

The `file://` scheme reads a file's contents as the value. No provider declaration needed:

```toml
[credentials.ssh]
private_key = "file://keys/deploy_ed25519"
```

The path is relative to the directory containing `confit.toml`.

### secret://

The `secret://` scheme is a wrapper that marks a value as sensitive. It doesn't change how the value is resolved -- it just flags it so display commands (`resolve`, `env`, `yaml`) mask it as `***` by default.

```toml
[credentials]
api_key = "secret://op://{vaults.prod}/api-key/credential"
db_password = "secret://vault://secret/myapp/db_pass"
plain_secret = "secret://my-literal-secret"
```

The `secret://` prefix is stripped, and the inner value is resolved normally (interpolation, shell eval, provider eval). The value is then tracked as sensitive.

Use `--reveal` on display commands to see real values:

```bash
$ confit resolve credentials.api_key
***

$ confit resolve credentials.api_key --reveal
sk-abc123
```

Commands that pass values to processes (`run`, `ssh`) always use real values -- masking only applies to display output.

## adding a new provider

To add a new secret backend or data source:

1. Add the provider declaration to `confit.toml`:
   ```toml
   [providers.vault]
   cmd = "vault kv get -field=value {path}"
   ```

2. Use the scheme in your config values:
   ```toml
   [credentials]
   db_password = "vault://secret/data/myapp/db_password"
   ```

3. That's it. No code changes needed.
