//! `Config` is the source of truth: [`Config::build`] parses confit.toml
//! (plus git/env state) once into it, and every other operation (`resolve`,
//! `env`, `validate`, ...) is a method on the result.
//!
//! Internally this splits into the generic, arbitrary-shaped part of
//! confit.toml -- user-defined sections, resolved lazily via
//! [`interpolate`]'s `{ref}` engine, [`shell`]'s `$(...)` engine, and
//! [`providers`]'s `scheme://` dispatch -- and the confit-owned typed parts
//! ([`crate::ports`]) that get parsed once and mirrored into the generic
//! tree so both look the same to `{ref}` interpolation.

mod interpolate;
mod providers;
mod shell;

pub use interpolate::{get, interpolate_node, interpolate_value};
pub use providers::{resolve_provider, resolve_providers, ProviderSpec, SourceCache, SourceSpec};
pub use shell::{eval_shell, eval_shells};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use toml::map::Map;
use toml::Value;

use crate::error::{Error, Result};
use crate::yaml;

use interpolate::value_to_string;

const CONFIG_FILENAME: &str = "confit.toml";
const VAR_ENV_PREFIX: &str = "CONFIT_VAR_";

pub fn find_config() -> Result<PathBuf> {
    let cwd = std::env::current_dir().map_err(|e| Error::Io {
        path: PathBuf::from("."),
        source: e,
    })?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(CONFIG_FILENAME);
        if candidate.exists() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Err(Error::ConfigNotFound),
        }
    }
}

pub fn load_raw(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(content.parse::<Value>()?)
}

pub fn collect_env_vars() -> HashMap<String, String> {
    let mut result = HashMap::new();
    for (k, v) in std::env::vars() {
        if let Some(name) = k.strip_prefix(VAR_ENV_PREFIX) {
            let name = name.to_lowercase();
            if !name.is_empty() {
                result.insert(name, v);
            }
        }
    }
    result
}

pub struct Config {
    /// The generic, arbitrary-shaped part of confit.toml -- user-defined
    /// sections, resolved lazily per path via `{ref}`/`$(...)`/`scheme://`.
    /// `[ports]`'s resolved values are mirrored in here too (as plain
    /// literals) so `{ports.*}` refs work exactly like any other value.
    pub tree: Value,
    pub providers: HashMap<String, ProviderSpec>,
    pub sources: HashMap<String, SourceSpec>,
    pub merged_vars: HashMap<String, String>,
    pub config_dir: PathBuf,
    /// The typed, confit-owned `[ports]` section, if the file has one.
    /// `None` means there's no `[ports]` table at all -- not that it failed
    /// to parse; a malformed `[ports]` fails `Config::build` outright.
    pub ports: Option<crate::ports::ResolvedPorts>,
    /// Sources are loaded (and their `load` command run) at most once per
    /// `Config`, the first time something actually references them --
    /// shared across every `resolve`/`env`/`validate`/... call on `&self`,
    /// not recreated per call.
    source_cache: RefCell<SourceCache>,
}

impl Config {
    /// Load and resolve confit.toml (or the file at `path`) once.
    ///
    /// `vars` are `--set`-style overrides. Every key in `vars`, and every
    /// `CONFIT_VAR_*` environment variable, must already exist as a key in
    /// `[vars]` -- this catches a typo like `--set stagee=prod` at build
    /// time instead of the value silently going unused.
    ///
    /// `profile`, if given, must name an existing `[env.<profile>]` section
    /// (an unknown profile is an error, not a silent no-op). Its `vars`
    /// sub-table, if any, is layered in between `[vars]` and
    /// `CONFIT_VAR_*`/`vars` -- so a profile can pin values (e.g. `stage`)
    /// without requiring `--set` at the call site. Precedence, lowest to
    /// highest: `[vars]` < profile `vars` < `CONFIT_VAR_*` < `vars`.
    ///
    /// If the file has a `[ports]` section, it's parsed and resolved here
    /// too (see [`ports::resolve`](crate::ports::resolve)): a malformed
    /// `[ports]` table fails construction immediately, the same as any
    /// other structural problem.
    pub fn build(
        path: Option<&Path>,
        vars: &HashMap<String, String>,
        profile: Option<&str>,
    ) -> Result<Config> {
        let path = match path {
            Some(p) => p.to_path_buf(),
            None => find_config()?,
        };
        let mut raw = load_raw(&path)?;

        let (providers, sources) = {
            let table = raw
                .as_table_mut()
                .ok_or_else(|| Error::Runtime("Config root must be a table".into()))?;
            let providers: HashMap<String, ProviderSpec> = match table.remove("providers") {
                Some(v) => v
                    .try_into()
                    .map_err(|e| Error::Runtime(format!("[providers]: {e}")))?,
                None => HashMap::new(),
            };
            let sources: HashMap<String, SourceSpec> = match table.remove("sources") {
                Some(v) => v
                    .try_into()
                    .map_err(|e| Error::Runtime(format!("[sources]: {e}")))?,
                None => HashMap::new(),
            };
            (providers, sources)
        };

        let declared_table = raw
            .as_table()
            .and_then(|t| t.get("vars"))
            .and_then(|v| v.as_table());
        let mut declared_vars: HashSet<String> = declared_table
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();
        let vars_section: HashMap<String, String> = declared_table
            .map(|t| {
                t.iter()
                    .map(|(k, v)| (k.clone(), value_to_string(v)))
                    .collect()
            })
            .unwrap_or_default();

        let profile_vars = match profile {
            None => HashMap::new(),
            Some(name) => {
                let profile_path = format!("env.{name}");
                get(&raw, &profile_path).map_err(|_| {
                    Error::Runtime(format!(
                        "profile '{name}' not found (no [env.{name}] section in confit.toml)"
                    ))
                })?;
                match get(&raw, &format!("{profile_path}.vars")) {
                    Ok(Value::Table(t)) => t
                        .iter()
                        .map(|(k, v)| (k.clone(), value_to_string(v)))
                        .collect(),
                    _ => HashMap::new(),
                }
            }
        };
        // A profile's own `vars.*` pins are a legitimate declaration too --
        // e.g. `[env.dev.vars] stage = "development"` with no top-level
        // [vars] at all, then `--set stage=staging` overriding it.
        declared_vars.extend(profile_vars.keys().cloned());

        let env_vars = collect_env_vars();

        // Every name explicitly supplied by the caller (env + `vars`) must
        // already be declared in [vars] (or pinned by the active profile) --
        // catches typos like `--set stagee=prod` instead of letting the
        // value silently do nothing.
        for name in env_vars.keys().chain(vars.keys()) {
            if !declared_vars.contains(name) {
                return Err(Error::Runtime(format!(
                    "'{name}' is not declared in [vars]; add it to confit.toml's \
                     [vars] section, or check for a typo"
                )));
            }
        }

        let mut merged_vars = vars_section;
        merged_vars.extend(profile_vars);
        merged_vars.extend(env_vars);
        merged_vars.extend(vars.clone());

        let config_dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        // Resolve [ports] (if present) before the vars/ports tables are
        // written back, so this reads the user-declared `infra`/`services`
        // offsets, not anything already-resolved.
        let ports_raw = raw.as_table().and_then(|t| t.get("ports")).cloned();
        let ports = match ports_raw {
            Some(v) => {
                let branch = crate::ports::current_branch(&config_dir)?;
                Some(crate::ports::resolve(&v, &branch, &config_dir)?)
            }
            None => None,
        };

        let table = raw.as_table_mut().unwrap();
        let vars_table: Map<String, Value> = merged_vars
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        table.insert("vars".into(), Value::Table(vars_table));
        if let Some(resolved) = &ports {
            let ports_tree =
                Value::try_from(resolved).map_err(|e| Error::Runtime(format!("[ports]: {e}")))?;
            table.insert("ports".into(), ports_tree);
        }

        Ok(Config {
            tree: raw,
            providers,
            sources,
            merged_vars,
            config_dir,
            ports,
            source_cache: RefCell::new(SourceCache::new()),
        })
    }

    /// Resolve one `scheme://...` (or plain) value against this config's
    /// providers/sources/vars/cwd -- [`resolve_provider`] with the config's
    /// own fields already supplied. Shares this `Config`'s source cache, so
    /// a source referenced across several calls loads at most once.
    pub fn resolve_provider(&self, value: &str) -> Result<(String, bool)> {
        resolve_provider(
            value,
            &self.providers,
            &self.sources,
            &self.merged_vars,
            Some(&self.config_dir),
            &mut self.source_cache.borrow_mut(),
        )
    }

    /// Recursively resolve every string leaf in `node` this way, tracking
    /// which paths turned out secret -- [`resolve_providers`] with the
    /// config's own fields already supplied.
    fn resolve_providers(&self, node: &Value, secrets: &mut HashSet<String>) -> Result<Value> {
        resolve_providers(
            node,
            &self.providers,
            &self.sources,
            &self.merged_vars,
            Some(&self.config_dir),
            secrets,
            &mut self.source_cache.borrow_mut(),
        )
    }

    pub fn resolve(&self, dotted_path: &str, eval_providers: bool) -> Result<Resolved> {
        let node = get(&self.tree, dotted_path)?;
        let value = interpolate_node(node, &self.tree)?;
        let mut secrets = HashSet::new();
        let (value, is_leaf_secret) = if eval_providers {
            let value = eval_shells(&value, Some(&self.config_dir))?;
            let leaf_secret = match &value {
                Value::String(s) => self.resolve_provider(s)?.1,
                _ => false,
            };
            let resolved = self.resolve_providers(&value, &mut secrets)?;
            (resolved, leaf_secret)
        } else {
            (value, false)
        };
        match &value {
            Value::Table(_) => Err(Error::Lookup(format!(
                "'{dotted_path}' is a section, not a value. \
                 Use 'confit keys {dotted_path}' to list keys or \
                 'confit show {dotted_path}' for KEY=VALUE output."
            ))),
            Value::Array(arr) => Ok(Resolved {
                value: arr
                    .iter()
                    .map(value_to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
                secret: is_leaf_secret || !secrets.is_empty(),
            }),
            other => Ok(Resolved {
                value: value_to_string(other),
                secret: is_leaf_secret || !secrets.is_empty(),
            }),
        }
    }

    pub fn keys(&self, dotted_path: &str) -> Result<Vec<String>> {
        let node = get(&self.tree, dotted_path)?;
        match node.as_table() {
            Some(table) => Ok(table.keys().cloned().collect()),
            None => Err(Error::Lookup(format!("'{dotted_path}' is not a section"))),
        }
    }

    pub fn env(&self, dotted_path: &str, eval_providers: bool) -> Result<Vec<EnvPair>> {
        let node = get(&self.tree, dotted_path)?;
        let interpolated = interpolate_node(node, &self.tree)?;
        let table = match interpolated.as_table() {
            Some(t) => t,
            None => return Err(Error::Lookup(format!("'{dotted_path}' is not a section"))),
        };
        let mut leaves = Map::new();
        for (k, v) in table {
            if !v.is_table() {
                leaves.insert(k.clone(), v.clone());
            }
        }
        let leaves = Value::Table(leaves);
        let mut secrets = HashSet::new();
        let resolved = if eval_providers {
            let leaves = eval_shells(&leaves, Some(&self.config_dir))?;
            self.resolve_providers(&leaves, &mut secrets)?
        } else {
            leaves
        };
        let table = resolved.as_table().unwrap();
        Ok(table
            .iter()
            .map(|(k, v)| EnvPair {
                key: k.clone(),
                value: value_to_string(v),
                secret: secrets.contains(k.as_str()),
            })
            .collect())
    }

    /// Resolve one or more sections into a single ordered set of env pairs.
    ///
    /// Sections are composed left-to-right; on a key conflict the later
    /// section wins (its value replaces the earlier one, keeping the
    /// original position).
    pub fn env_multi(&self, dotted_paths: &[String], eval_providers: bool) -> Result<Vec<EnvPair>> {
        let mut order: Vec<String> = Vec::new();
        let mut by_key: HashMap<String, EnvPair> = HashMap::new();
        for path in dotted_paths {
            for pair in self.env(path, eval_providers)? {
                if !by_key.contains_key(&pair.key) {
                    order.push(pair.key.clone());
                }
                by_key.insert(pair.key.clone(), pair);
            }
        }
        Ok(order
            .into_iter()
            .map(|k| by_key.remove(&k).unwrap())
            .collect())
    }

    /// Try to resolve every leaf value in the config; returns per-path
    /// success/failure instead of stopping at the first error.
    pub fn validate(&self) -> Vec<(String, bool, String)> {
        let mut results = Vec::new();

        #[allow(clippy::too_many_arguments)]
        fn walk(
            node: &Value,
            prefix: &str,
            config: &Value,
            providers: &HashMap<String, ProviderSpec>,
            sources: &HashMap<String, SourceSpec>,
            merged_vars: &HashMap<String, String>,
            config_dir: &Path,
            results: &mut Vec<(String, bool, String)>,
            source_cache: &mut SourceCache,
        ) {
            match node {
                Value::Table(map) => {
                    for (k, v) in map {
                        let path = if prefix.is_empty() {
                            k.clone()
                        } else {
                            format!("{prefix}.{k}")
                        };
                        walk(
                            v,
                            &path,
                            config,
                            providers,
                            sources,
                            merged_vars,
                            config_dir,
                            results,
                            source_cache,
                        );
                    }
                }
                Value::Array(arr) => {
                    for (i, item) in arr.iter().enumerate() {
                        let path = format!("{prefix}[{i}]");
                        walk(
                            item,
                            &path,
                            config,
                            providers,
                            sources,
                            merged_vars,
                            config_dir,
                            results,
                            source_cache,
                        );
                    }
                }
                Value::String(s) => {
                    let resolving = HashSet::new();
                    match interpolate_value(s, config, &resolving)
                        .and_then(|v| eval_shell(&v, Some(config_dir)))
                        .and_then(|v| {
                            resolve_provider(
                                &v,
                                providers,
                                sources,
                                merged_vars,
                                Some(config_dir),
                                source_cache,
                            )
                            .map(|(val, _)| val)
                        }) {
                        Ok(_) => results.push((prefix.to_string(), true, String::new())),
                        Err(e) => results.push((prefix.to_string(), false, e.to_string())),
                    }
                }
                _ => {
                    results.push((prefix.to_string(), true, String::new()));
                }
            }
        }

        walk(
            &self.tree,
            "",
            &self.tree,
            &self.providers,
            &self.sources,
            &self.merged_vars,
            &self.config_dir,
            &mut results,
            &mut self.source_cache.borrow_mut(),
        );
        results
    }

    pub fn yaml_section(
        &self,
        dotted_path: &str,
        eval_providers: bool,
        wrap: Option<&str>,
        reveal: bool,
    ) -> Result<String> {
        let node = get(&self.tree, dotted_path)?;
        let mut resolved = interpolate_node(node, &self.tree)?;
        let mut secrets = HashSet::new();
        if eval_providers {
            resolved = eval_shells(&resolved, Some(&self.config_dir))?;
            resolved = self.resolve_providers(&resolved, &mut secrets)?;
        }
        if !reveal {
            mask_secrets(&mut resolved, &secrets, "");
        }
        if let Some(key) = wrap {
            let mut wrapper = Map::new();
            wrapper.insert(key.to_string(), resolved);
            resolved = Value::Table(wrapper);
        }
        Ok(yaml::to_yaml(&resolved))
    }

    pub fn load(&self, eval_providers: bool) -> Result<Value> {
        let interpolated = interpolate_node(&self.tree, &self.tree)?;
        if eval_providers {
            let evaled = eval_shells(&interpolated, Some(&self.config_dir))?;
            let mut secrets = HashSet::new();
            self.resolve_providers(&evaled, &mut secrets)
        } else {
            Ok(interpolated)
        }
    }
}

/// Resolved value with secret metadata.
pub struct Resolved {
    pub value: String,
    pub secret: bool,
}

/// Env pair with secret metadata.
pub struct EnvPair {
    pub key: String,
    pub value: String,
    pub secret: bool,
}

fn mask_secrets(node: &mut Value, secrets: &HashSet<String>, prefix: &str) {
    match node {
        Value::Table(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if let Some(v) = map.get_mut(&k) {
                    mask_secrets(v, secrets, &path);
                }
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter_mut().enumerate() {
                let path = format!("{prefix}[{i}]");
                mask_secrets(item, secrets, &path);
            }
        }
        Value::String(s) if secrets.contains(prefix) => {
            *s = "***".to_string();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use std::process::Command;

    fn write_config(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("confit.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_mask_secrets() {
        let mut node: Value = r#"
            token = "hunter2"
            host = "localhost"
        "#
        .parse()
        .unwrap();
        let mut secrets = HashSet::new();
        secrets.insert("token".to_string());
        mask_secrets(&mut node, &secrets, "");
        let table = node.as_table().unwrap();
        assert_eq!(table["token"].as_str().unwrap(), "***");
        assert_eq!(table["host"].as_str().unwrap(), "localhost");
    }

    #[test]
    #[serial]
    fn test_build_config_with_vars() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [vars]
            env = "dev"
            [app]
            name = "myapp-{vars.env}"
            "#,
        );
        let mut runtime = HashMap::new();
        runtime.insert("env".into(), "prod".into());
        let bc = Config::build(Some(&path), &runtime, None).unwrap();
        assert_eq!(bc.merged_vars["env"], "prod");
    }

    #[test]
    #[serial]
    fn test_build_config_env_vars() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [vars]
            region = "default"
            "#,
        );
        std::env::set_var("CONFIT_VAR_REGION", "us-west-2");
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        assert_eq!(bc.merged_vars["region"], "us-west-2");
        std::env::remove_var("CONFIT_VAR_REGION");
    }

    #[test]
    #[serial]
    fn test_build_config_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [vars]
            x = "from-file"
            "#,
        );
        std::env::set_var("CONFIT_VAR_X", "from-env");
        let mut runtime = HashMap::new();
        runtime.insert("x".into(), "from-cli".into());
        let bc = Config::build(Some(&path), &runtime, None).unwrap();
        assert_eq!(bc.merged_vars["x"], "from-cli");
        std::env::remove_var("CONFIT_VAR_X");
    }

    #[test]
    #[serial]
    fn test_end_to_end_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [vars]
            stage = "test"
            [app]
            name = "svc-{vars.stage}"
            port = 3000
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let result = bc.resolve("app.name", false).unwrap();
        assert_eq!(result.value, "svc-test");
    }

    #[test]
    #[serial]
    fn test_end_to_end_with_file_provider() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("db_pass.txt"), "hunter2\n").unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [db]
            password = "file://db_pass.txt"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let resolved = bc.resolve("db.password", true).unwrap();
        assert_eq!(resolved.value, "hunter2");
    }

    #[test]
    #[serial]
    fn test_end_to_end_with_ports() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        let path = write_config(
            dir.path(),
            r#"
            [ports]
            band = 4300

            [ports.infra]
            postgres = 0

            [ports.services]
            app = 50

            [db]
            url = "postgres://localhost:{ports.infra.postgres}/mydb"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();

        // main/master is unborn HEAD's symbolic ref by default in a fresh
        // `git init`, so slot should be 0 and app == band + lane.
        let resolved = bc.ports.as_ref().unwrap();
        assert_eq!(resolved.infra["postgres"], 4300);
        assert_eq!(resolved.services["app"], 4300 + 50 + resolved.slot as i64);
        assert_eq!(
            resolved.branch_slug,
            crate::ports::slugify(&resolved.branch)
        );

        // ports.* values are ordinary refs, resolvable from elsewhere in the file.
        let resolved = bc.resolve("db.url", false).unwrap();
        assert_eq!(resolved.value, "postgres://localhost:4300/mydb");
    }

    #[test]
    #[serial]
    fn test_end_to_end_with_shell() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [build]
            hash = "$(echo abc123)"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let resolved = bc.resolve("build.hash", true).unwrap();
        assert_eq!(resolved.value, "abc123");
    }

    #[test]
    #[serial]
    fn test_env_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [vars]
            stage = "dev"
            [db]
            host = "localhost"
            port = 5432
            name = "mydb-{vars.stage}"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let mut pairs = bc.env("db", false).unwrap();
        pairs.sort_by(|a, b| a.key.cmp(&b.key));

        assert_eq!(pairs.len(), 3);
        assert!(pairs
            .iter()
            .any(|p| p.key == "host" && p.value == "localhost"));
        assert!(pairs.iter().any(|p| p.key == "port" && p.value == "5432"));
        assert!(pairs
            .iter()
            .any(|p| p.key == "name" && p.value == "mydb-dev"));
    }

    #[test]
    #[serial]
    fn test_providers_section_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [providers.op]
            cmd = "op read {path}"
            [app]
            secret = "op://vault/item/field"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        assert!(bc.tree.as_table().unwrap().get("providers").is_none());
        assert!(bc.providers.contains_key("op"));
    }

    #[test]
    #[serial]
    fn test_sources_section_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [sources.mysrc]
            load = "echo FOO=bar"
            [app]
            val = "mysrc://FOO"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        assert!(bc.tree.as_table().unwrap().get("sources").is_none());
        assert!(bc.sources.contains_key("mysrc"));
    }

    #[test]
    #[serial]
    fn test_source_end_to_end_via_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [sources.bag]
            load = "echo FOO=from_source"
            [app]
            val = "bag://FOO"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let resolved = bc.resolve("app.val", true).unwrap();
        assert_eq!(resolved.value, "from_source");
    }

    #[test]
    #[serial]
    fn test_source_cache_shared_across_config_method_calls() {
        // The source's load command produces a fresh random value each time
        // it actually runs. If two separate top-level calls on the same
        // Config (resolve, then env) each got their own SourceCache, the
        // second call would see a different value.
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [sources.bag]
            load = "echo FOO=$(date +%N)"
            [app]
            val = "bag://FOO"
            other = "bag://FOO"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let first = bc.resolve("app.val", true).unwrap().value;
        let second = bc.resolve("app.other", true).unwrap().value;
        assert_eq!(
            first, second,
            "two separate .resolve() calls on the same Config should share \
             one source load, not re-run the load command each time"
        );

        let pairs = bc.env("app", true).unwrap();
        let via_env = pairs.iter().find(|p| p.key == "val").unwrap();
        assert_eq!(
            via_env.value, first,
            ".env() should reuse the same cached source load too"
        );
    }
}
