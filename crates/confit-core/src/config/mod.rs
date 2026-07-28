//! `Config` is the source of truth: [`Config::build`] parses confit.toml
//! (plus git/env state) once into it, and every other operation (`resolve`,
//! `env`, `validate`, ...) is a method on the result.
//!
//! Internally this splits into the generic, arbitrary-shaped part of
//! confit.toml -- user-defined sections, resolved lazily via
//! [`interpolate`]'s `{ref}` engine, [`shell`]'s `$(...)` engine, and
//! [`providers`]'s `scheme://` dispatch -- and the confit-owned typed parts
//! ([`ports`]) that get parsed once and mirrored into the generic tree so
//! both look the same to `{ref}` interpolation.

mod interpolate;
mod ports;
mod providers;
mod shell;
mod slots;

pub use ports::{check_host, HostIssue, ResolvedPorts, Severity};
pub use providers::{ProviderSpec, SourceSpec};

use interpolate::{get, interpolate_node, interpolate_value, value_to_string};
use providers::{resolve_provider, resolve_providers, SourceCache};
use shell::{eval_shell, eval_shells};

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use toml::map::Map;
use toml::Value;

use crate::error::{Error, Result};
use crate::yaml;

const CONFIG_FILENAME: &str = "confit.toml";
const VAR_ENV_PREFIX: &str = "CONFIT_VAR_";

fn find_config() -> Result<PathBuf> {
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

fn load_raw(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(content.parse::<Value>()?)
}

fn collect_env_vars() -> HashMap<String, String> {
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
    /// The raw `[ports]` section is removed from here at build; its
    /// resolved mirror lives in `ports_tree` and is materialized on demand.
    tree: Value,
    pub providers: HashMap<String, ProviderSpec>,
    pub sources: HashMap<String, SourceSpec>,
    pub merged_vars: HashMap<String, String>,
    pub config_dir: PathBuf,
    /// The shape-validated `[ports]` declaration, if the file has one.
    /// Parsing is eager (a malformed `[ports]` fails `Config::build`), but
    /// the effectful part -- reading the git branch and assigning a slot in
    /// the ledger -- is deferred to [`Config::ports`] so commands that
    /// never touch ports never spawn git or write the ledger.
    ports_spec: Option<ports::PortsSpec>,
    /// Lazily-resolved `[ports]` values; populated at most once.
    ports: OnceCell<ports::ResolvedPorts>,
    /// `tree` plus the resolved ports mirror, so `{ports.*}` refs resolve
    /// like any other value. Materialized on first use that needs it.
    ports_tree: OnceCell<Value>,
    /// Whether any string in the file mentions `{ports.` -- when false,
    /// interpolation can never reach ports, so operations on other sections
    /// skip ports resolution entirely.
    refs_ports: bool,
    /// Sources are loaded (and their `load` command run) at most once per
    /// `Config`, the first time something actually references them --
    /// shared across every `resolve`/`env`/`validate`/... call on `&self`,
    /// not recreated per call.
    source_cache: RefCell<SourceCache>,
}

/// Whether any string anywhere in `node` contains the literal `{ports.`.
fn mentions_ports(node: &Value) -> bool {
    match node {
        Value::String(s) => s.contains("{ports."),
        Value::Table(map) => map.values().any(mentions_ports),
        Value::Array(arr) => arr.iter().any(mentions_ports),
        _ => false,
    }
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
    /// If the file has a `[ports]` section, its shape is validated here (a
    /// malformed `[ports]` table fails construction immediately, the same
    /// as any other structural problem), but the git/ledger work of
    /// resolving it is deferred until something actually uses ports.
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
            // `env` is the built-in process-environment source and always
            // wins the scheme lookup; a user-defined one would silently
            // never run, so reject it outright.
            if sources.contains_key("env") {
                return Err(Error::Runtime(
                    "[sources.env] collides with the built-in env:// source \
                     (which reads the process environment); pick another name"
                        .into(),
                ));
            }
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

        // Shape-validate [ports] now (pure); defer the git/ledger work.
        // The raw table is removed from the tree -- its offsets/lanes must
        // never be readable where resolved ports are expected.
        let table = raw.as_table_mut().unwrap();
        let ports_spec = match table.remove("ports") {
            Some(v) => Some(ports::PortsSpec::parse(v)?),
            None => None,
        };

        let vars_table: Map<String, Value> = merged_vars
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        table.insert("vars".into(), Value::Table(vars_table));

        let refs_ports = mentions_ports(&raw);

        Ok(Config {
            tree: raw,
            providers,
            sources,
            merged_vars,
            config_dir,
            ports_spec,
            ports: OnceCell::new(),
            ports_tree: OnceCell::new(),
            refs_ports,
            source_cache: RefCell::new(SourceCache::new()),
        })
    }

    /// The resolved `[ports]` section, or `None` if the file has none.
    ///
    /// The first call reads the current git branch and (on a non-primary
    /// branch) assigns a slot in the ledger; later calls return the cached
    /// result. This is where "must be inside a git working tree" applies --
    /// not `Config::build`.
    pub fn ports(&self) -> Result<Option<&ports::ResolvedPorts>> {
        let Some(spec) = &self.ports_spec else {
            return Ok(None);
        };
        if let Some(resolved) = self.ports.get() {
            return Ok(Some(resolved));
        }
        let branch = ports::current_branch(&self.config_dir)?;
        let resolved = spec.resolve(&branch, &self.config_dir)?;
        Ok(Some(self.ports.get_or_init(|| resolved)))
    }

    /// The tree to resolve `target` against. Ports are materialized into it
    /// only when they can actually matter: the target is under `ports`, or
    /// some string in the file references `{ports.`. Everything else reads
    /// the plain tree and never triggers git/ledger work.
    fn tree_for(&self, target: &str) -> Result<&Value> {
        let needs_ports = self.ports_spec.is_some()
            && (self.refs_ports || target == "ports" || target.starts_with("ports."));
        if !needs_ports {
            return Ok(&self.tree);
        }
        if let Some(tree) = self.ports_tree.get() {
            return Ok(tree);
        }
        let resolved = self.ports()?.expect("ports_spec is present");
        let mirror =
            Value::try_from(resolved).map_err(|e| Error::Runtime(format!("[ports]: {e}")))?;
        let mut tree = self.tree.clone();
        tree.as_table_mut().unwrap().insert("ports".into(), mirror);
        Ok(self.ports_tree.get_or_init(|| tree))
    }

    /// Like [`Config::tree_for`] for whole-tree operations (`load`,
    /// `validate`): if the file declares `[ports]` at all, the mirror is
    /// included.
    fn full_tree(&self) -> Result<&Value> {
        self.tree_for("ports")
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
        let tree = self.tree_for(dotted_path)?;
        let node = get(tree, dotted_path)?;
        let value = interpolate_node(node, tree)?;
        if value.is_table() {
            return Err(Error::Lookup(format!(
                "'{dotted_path}' is a section, not a value. \
                 Use 'confit keys {dotted_path}' to list keys or \
                 'confit show {dotted_path}' for KEY=VALUE output."
            )));
        }
        if !eval_providers {
            return Ok(Resolved {
                value: value_to_string(&value),
                secret: false,
            });
        }

        let value = eval_shells(&value, Some(&self.config_dir))?;
        match &value {
            // A bare top-level string is exactly what resolve_provider
            // handles -- calling resolve_providers here too would run the
            // same provider/source a second time for one value.
            Value::String(s) => {
                let (resolved, secret) = self.resolve_provider(s)?;
                Ok(Resolved {
                    value: resolved,
                    secret,
                })
            }
            Value::Array(_) => {
                let mut secrets = HashSet::new();
                let resolved = self.resolve_providers(&value, &mut secrets)?;
                Ok(Resolved {
                    value: value_to_string(&resolved),
                    secret: !secrets.is_empty(),
                })
            }
            other => Ok(Resolved {
                value: value_to_string(other),
                secret: false,
            }),
        }
    }

    pub fn keys(&self, dotted_path: &str) -> Result<Vec<String>> {
        let node = get(self.tree_for(dotted_path)?, dotted_path)?;
        match node.as_table() {
            Some(table) => Ok(table.keys().cloned().collect()),
            None => Err(Error::Lookup(format!("'{dotted_path}' is not a section"))),
        }
    }

    pub fn env(&self, dotted_path: &str, eval_providers: bool) -> Result<Vec<EnvPair>> {
        let tree = self.tree_for(dotted_path)?;
        let node = get(tree, dotted_path)?;
        let interpolated = interpolate_node(node, tree)?;
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
    /// success/failure instead of stopping at the first error. If the file
    /// has a `[ports]` section that fails to resolve (not in a git repo,
    /// slots exhausted, ...), that failure becomes a `ports` row instead of
    /// aborting validation of everything else.
    pub fn validate(&self) -> Vec<(String, bool, String)> {
        fn walk(
            cfg: &Config,
            tree: &Value,
            node: &Value,
            prefix: &str,
            results: &mut Vec<(String, bool, String)>,
        ) {
            match node {
                Value::Table(map) => {
                    for (k, v) in map {
                        let path = if prefix.is_empty() {
                            k.clone()
                        } else {
                            format!("{prefix}.{k}")
                        };
                        walk(cfg, tree, v, &path, results);
                    }
                }
                Value::Array(arr) => {
                    for (i, item) in arr.iter().enumerate() {
                        let path = format!("{prefix}[{i}]");
                        walk(cfg, tree, item, &path, results);
                    }
                }
                Value::String(s) => {
                    let resolving = HashSet::new();
                    match interpolate_value(s, tree, &resolving)
                        .and_then(|v| eval_shell(&v, Some(&cfg.config_dir)))
                        .and_then(|v| cfg.resolve_provider(&v).map(|(val, _)| val))
                    {
                        Ok(_) => results.push((prefix.to_string(), true, String::new())),
                        Err(e) => results.push((prefix.to_string(), false, e.to_string())),
                    }
                }
                _ => {
                    results.push((prefix.to_string(), true, String::new()));
                }
            }
        }

        let mut results = Vec::new();
        let tree = match self.full_tree() {
            Ok(tree) => tree,
            Err(e) => {
                results.push(("ports".to_string(), false, e.to_string()));
                &self.tree
            }
        };
        walk(self, tree, tree, "", &mut results);
        results
    }

    pub fn yaml_section(
        &self,
        dotted_path: &str,
        eval_providers: bool,
        wrap: Option<&str>,
        reveal: bool,
    ) -> Result<String> {
        let tree = self.tree_for(dotted_path)?;
        let node = get(tree, dotted_path)?;
        let mut resolved = interpolate_node(node, tree)?;
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
        let tree = self.full_tree()?;
        let interpolated = interpolate_node(tree, tree)?;
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
        let resolved = bc.ports().unwrap().unwrap();
        assert_eq!(resolved.infra["postgres"], 4300);
        assert_eq!(resolved.services["app"], 4300 + 50 + resolved.slot as i64);
        let branch = ports::current_branch(dir.path()).unwrap();
        assert_eq!(resolved.slug, ports::slugify(&branch));

        // ports.* values are ordinary refs, resolvable from elsewhere in the file.
        let resolved = bc.resolve("db.url", false).unwrap();
        assert_eq!(resolved.value, "postgres://localhost:4300/mydb");
    }

    #[test]
    #[serial]
    fn test_ports_resolution_is_lazy() {
        // NOT a git repo: with [ports] present, build must still succeed
        // (shape validation is pure) and operations on sections that never
        // reference ports must work -- the git/ledger work only happens
        // when ports are actually used.
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [ports]
            band = 20000
            [ports.services]
            app = 50
            [db]
            host = "localhost"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        assert_eq!(bc.resolve("db.host", true).unwrap().value, "localhost");
        assert_eq!(bc.keys("db").unwrap(), vec!["host"]);

        // Actually touching ports is where git is required.
        let err = bc.resolve("ports.services.app", false).err().unwrap();
        assert!(err.to_string().contains("git repo"), "got: {err}");
        let err = bc.ports().unwrap_err();
        assert!(err.to_string().contains("git repo"), "got: {err}");

        // A malformed [ports] still fails at build, not first use.
        let path = write_config(dir.path(), "[ports]\nband = \"nope\"");
        let err = Config::build(Some(&path), &HashMap::new(), None)
            .err()
            .unwrap();
        assert!(err.to_string().contains("[ports]"), "got: {err}");
    }

    #[test]
    #[serial]
    fn test_transitive_ports_ref_resolves_from_unrelated_section() {
        // db.url doesn't mention ports, but it references services.web.url
        // which does -- the file-level "{ports." scan must catch this.
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
            band = 20000
            [ports.services]
            web = 50
            [services]
            port = "{ports.services.web}"
            [db]
            url = "host:{services.port}"
            "#,
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        assert_eq!(bc.resolve("db.url", false).unwrap().value, "host:20050");
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
    fn test_resolve_invokes_provider_exactly_once() {
        // resolve() used to call resolve_provider() once (to get the
        // top-level secret flag) and then resolve_providers() again on the
        // same string (to get the value), running the provider's command
        // twice for one resolve(). The provider here appends to a file each
        // time it runs, so a second invocation would show up as a second
        // line.
        let dir = tempfile::tempdir().unwrap();
        let counter = dir.path().join("calls.txt");
        let path = write_config(
            dir.path(),
            &format!(
                r#"
                [providers.count]
                cmd = "echo x >> {} && echo ran"
                [app]
                x = "count://thing"
                "#,
                counter.display()
            ),
        );
        let bc = Config::build(Some(&path), &HashMap::new(), None).unwrap();
        let resolved = bc.resolve("app.x", true).unwrap();
        assert_eq!(resolved.value, "ran");
        let calls = std::fs::read_to_string(&counter).unwrap();
        assert_eq!(
            calls.lines().count(),
            1,
            "provider should run exactly once per resolve(), ran: {calls:?}"
        );
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
