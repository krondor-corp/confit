---
title: Install
slug: install
order: 1
---

## install from GitHub releases

The fastest way to install confit:

```bash
curl -fsSL https://raw.githubusercontent.com/krondor-corp/confit/main/install.sh | bash
```

This detects your OS and architecture, downloads the latest release binary, and installs it to `~/.local/bin`.

Make sure `~/.local/bin` is on your `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## verify

```bash
confit --help
```

You should see the top-level help with `resolve`, `show`, `keys`, `run`, `validate`, `ssh`, `log`, `update`, and `version` commands.

## confit.toml

confit looks for `confit.toml` by walking up from the current directory. As long as you're somewhere inside your project tree, it will find the config file automatically.

```
my-project/
  confit.toml    # <-- found automatically
  src/
  deploy/
```

No environment variables or flags needed to point at the config file. Provider commands always run from the config's directory, so relative paths in provider templates work correctly regardless of where you invoke confit.
