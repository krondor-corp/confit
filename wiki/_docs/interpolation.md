---
title: Interpolation
slug: interpolation
order: 4
---

## how it works

Any string value in `confit.toml` can reference other values using `{dotted.path}` syntax:

```toml
[project]
name = "my-project"
domain = "example.com"

[services.web]
url = "https://{project.domain}"
description = "{project.name} web service"
```

When confit resolves `services.web.url`, it:

1. Finds the raw value: `"https://{project.domain}"`
2. Looks up `project.domain` in the config: `"example.com"`
3. Returns: `"https://example.com"`

## recursive references

References can chain through multiple levels:

```toml
[vaults]
prod = "my-vault-production"

[credentials]
api_key = "op://{vaults.prod}/api-key/credential"
```

Resolving `credentials.api_key`:
1. Raw: `"op://{vaults.prod}/api-key/credential"`
2. Looks up `vaults.prod`: `"my-vault-production"`
3. Result: `"op://my-vault-production/api-key/credential"`

The interpolated value then goes through provider evaluation if providers are declared.

## lazy evaluation

confit only interpolates the path you ask for. If you run:

```bash
confit resolve project.name
```

It does not interpolate `credentials.api_key` or any other value in the config. This means you can have values that reference unset variables, and they won't cause errors unless you actually try to resolve them.

## lists

List values stay as lists internally. When referenced in a string context, they're joined with spaces:

```toml
[project]
users = ["admin", "deploy", "dev"]

[ansible]
user_list = "{project.users}"
```

```bash
$ confit resolve ansible.user_list
admin deploy dev

$ confit resolve project.users
admin deploy dev
```

## cycle detection

Circular references are caught and reported:

```toml
[a]
x = "{b.y}"

[b]
y = "{a.x}"
```

```bash
$ confit resolve a.x
Error: Circular reference: a.x
```

## regex

The interpolation token pattern is:

```
\{([a-zA-Z0-9_.]+)\}
```

Tokens must contain only letters, digits, underscores, and dots. This means `{path.to.value}` works but `{path-to-value}` does not match and is left as a literal string.
