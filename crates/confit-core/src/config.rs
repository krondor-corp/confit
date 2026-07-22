use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use toml::map::Map;
use toml::Value;

use crate::error::{Error, Result};
use crate::yaml;

static REF_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([a-zA-Z0-9_.]+)\}").unwrap());

static SCHEME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-zA-Z][a-zA-Z0-9_-]*)://").unwrap());

static SHELL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\((.+?)\)").unwrap());

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

pub fn get<'a>(config: &'a Value, dotted_path: &str) -> Result<&'a Value> {
    let parts: Vec<&str> = dotted_path.split('.').collect();
    let mut node = config;
    for part in &parts {
        match node.as_table() {
            Some(table) if table.contains_key(*part) => {
                node = &table[*part];
            }
            _ => {
                if parts[0] == "vars" {
                    let var_name = parts[1..].join(".");
                    return Err(Error::Lookup(format!(
                        "Variable '{var_name}' is not set. \
                         Define it in [vars] in confit.toml, \
                         pass --set {var_name}=VALUE, \
                         or set CONFIT_VAR_{}",
                        var_name.to_uppercase()
                    )));
                }
                return Err(Error::Lookup(format!(
                    "Path '{dotted_path}' not found (failed at '{part}')"
                )));
            }
        }
    }
    Ok(node)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Datetime(d) => d.to_string(),
        Value::Array(arr) => arr
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Table(_) => "[table]".into(),
    }
}

pub fn interpolate_value(
    value: &str,
    config: &Value,
    resolving: &HashSet<String>,
) -> Result<String> {
    let mut result = String::new();
    let mut last = 0;
    for cap in REF_RE.captures_iter(value) {
        let m = cap.get(0).unwrap();
        result.push_str(&value[last..m.start()]);
        let ref_path = &cap[1];
        if resolving.contains(ref_path) {
            return Err(Error::Runtime(format!("Circular reference: {ref_path}")));
        }
        let raw = get(config, ref_path)?;
        let replacement = match raw {
            Value::Array(arr) => arr
                .iter()
                .map(value_to_string)
                .collect::<Vec<_>>()
                .join(" "),
            Value::String(s) => {
                let mut new_resolving = resolving.clone();
                new_resolving.insert(ref_path.to_string());
                interpolate_value(s, config, &new_resolving)?
            }
            other => value_to_string(other),
        };
        result.push_str(&replacement);
        last = m.end();
    }
    result.push_str(&value[last..]);
    Ok(result)
}

pub fn interpolate_node(node: &Value, config: &Value) -> Result<Value> {
    let resolving = HashSet::new();
    interpolate_node_inner(node, config, &resolving)
}

fn interpolate_node_inner(
    node: &Value,
    config: &Value,
    resolving: &HashSet<String>,
) -> Result<Value> {
    match node {
        Value::String(s) => Ok(Value::String(interpolate_value(s, config, resolving)?)),
        Value::Table(map) => {
            let mut new_map = Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), interpolate_node_inner(v, config, resolving)?);
            }
            Ok(Value::Table(new_map))
        }
        Value::Array(arr) => {
            let new_arr: Result<Vec<Value>> = arr
                .iter()
                .map(|item| interpolate_node_inner(item, config, resolving))
                .collect();
            Ok(Value::Array(new_arr?))
        }
        other => Ok(other.clone()),
    }
}

pub fn eval_shell(value: &str, cwd: Option<&Path>) -> Result<String> {
    if !value.contains("$(") {
        return Ok(value.to_string());
    }
    let mut result = String::new();
    let mut last = 0;
    for cap in SHELL_RE.captures_iter(value) {
        let m = cap.get(0).unwrap();
        result.push_str(&value[last..m.start()]);
        let cmd = &cap[1];
        let mut command = Command::new("sh");
        command.args(["-c", cmd]);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let output = command
            .output()
            .map_err(|e| Error::Runtime(format!("Shell eval $({cmd}): {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Runtime(format!(
                "Shell eval failed: $({cmd}): {}",
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        result.push_str(stdout.trim());
        last = m.end();
    }
    result.push_str(&value[last..]);
    Ok(result)
}

pub fn eval_shells(node: &Value, cwd: Option<&Path>) -> Result<Value> {
    match node {
        Value::String(s) => Ok(Value::String(eval_shell(s, cwd)?)),
        Value::Table(map) => {
            let mut new_map = Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), eval_shells(v, cwd)?);
            }
            Ok(Value::Table(new_map))
        }
        Value::Array(arr) => {
            let new_arr: Result<Vec<Value>> =
                arr.iter().map(|item| eval_shells(item, cwd)).collect();
            Ok(Value::Array(new_arr?))
        }
        other => Ok(other.clone()),
    }
}

fn resolve_file(path_str: &str, cwd: Option<&Path>) -> Result<String> {
    let p = PathBuf::from(path_str);
    let p = if !p.is_absolute() {
        match cwd {
            Some(dir) => dir.join(p),
            None => p,
        }
    } else {
        p
    };
    if !p.exists() {
        return Err(Error::Runtime(format!("file://{path_str}: not found")));
    }
    let content = std::fs::read_to_string(&p).map_err(|e| Error::Io { path: p, source: e })?;
    Ok(content.trim().to_string())
}

fn expand_template(
    template: &str,
    vars: &HashMap<String, String>,
    scheme: &str,
    uri: &str,
) -> Result<String> {
    let mut result = String::new();
    let mut last = 0;
    for cap in REF_RE.captures_iter(template) {
        let m = cap.get(0).unwrap();
        result.push_str(&template[last..m.start()]);
        let key = &cap[1];
        match vars.get(key) {
            Some(val) => result.push_str(val),
            None => {
                return Err(Error::Runtime(format!(
                    "Provider '{scheme}' requires '{{{key}}}' but it was not set \
                     (resolving '{uri}'). Pass --set {key}=VALUE or set CONFIT_VAR_{}",
                    key.to_uppercase()
                )));
            }
        }
        last = m.end();
    }
    result.push_str(&template[last..]);
    Ok(result)
}

fn run_shell(cmd: &str, cwd: Option<&Path>) -> std::io::Result<std::process::Output> {
    let mut command = Command::new("sh");
    command.args(["-c", cmd]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.output()
}

// --- Providers and sources ---
//
// Both accept a bare-string shorthand or a table in TOML; ProviderSpec and
// SourceSpec parse either directly via serde instead of every call site
// doing its own `.as_table()`/`.get(...)`/`.as_str()` walk. Each is a small
// "runnable" type -- `ProviderSpec::resolve` and `SourceSpec::load` are the
// only places that actually shell out.

/// `[providers.<scheme>]`: resolves `scheme://path` by running `cmd` with
/// `{path}`, `{uri}`, and any `--set`/`[vars]` name substituted in.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ProviderSpec {
    Shorthand(String),
    Full { cmd: String },
}

impl ProviderSpec {
    fn cmd_template(&self) -> &str {
        match self {
            ProviderSpec::Shorthand(s) => s,
            ProviderSpec::Full { cmd } => cmd,
        }
    }

    /// Run this provider for `uri` (`path` is the part after `scheme://`).
    fn resolve(
        &self,
        scheme: &str,
        uri: &str,
        path: &str,
        vars: &HashMap<String, String>,
        cwd: Option<&Path>,
    ) -> Result<String> {
        let mut template_vars = vars.clone();
        template_vars.insert("uri".into(), uri.into());
        template_vars.insert("path".into(), path.into());
        let cmd = expand_template(self.cmd_template(), &template_vars, scheme, uri)?;

        let output =
            run_shell(&cmd, cwd).map_err(|e| Error::Runtime(format!("Provider {scheme}: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Runtime(format!(
                "Failed to eval '{uri}' via {scheme}: {}",
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// `[sources.<name>]`: resolves `name://field` by running `load` once
/// (bulk, dotenv-format output), caching the result, then looking up
/// `field` in it. `secret = true` marks every field from this source as
/// secret even without an explicit `secret://` prefix.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SourceSpec {
    Shorthand(String),
    Full {
        load: String,
        #[serde(default)]
        secret: bool,
    },
}

impl SourceSpec {
    fn load_template(&self) -> &str {
        match self {
            SourceSpec::Shorthand(s) => s,
            SourceSpec::Full { load, .. } => load,
        }
    }

    fn is_secret(&self) -> bool {
        matches!(self, SourceSpec::Full { secret: true, .. })
    }

    /// Run this source's load command once, parsing dotenv-format output
    /// into a key -> value bag.
    fn load(
        &self,
        name: &str,
        vars: &HashMap<String, String>,
        cwd: Option<&Path>,
    ) -> Result<HashMap<String, String>> {
        let template = self.load_template();

        // Reject {path} and {uri} — sources load a bag, they have no per-key path.
        for cap in REF_RE.captures_iter(template) {
            let key = &cap[1];
            if key == "path" || key == "uri" {
                return Err(Error::Runtime(format!(
                    "Source '{name}' load template references '{{{key}}}' which is not \
                     allowed (sources don't have per-key paths). \
                     Use a provider for per-key substitution."
                )));
            }
        }

        // Support both {stage} and {vars.stage} in source load templates.
        let template_vars: HashMap<String, String> = vars
            .iter()
            .flat_map(|(k, v)| [(k.clone(), v.clone()), (format!("vars.{k}"), v.clone())])
            .collect();

        let cmd = expand_template(template, &template_vars, name, name)?;

        let output = run_shell(&cmd, cwd)
            .map_err(|e| Error::Runtime(format!("Source '{name}' load failed: {e}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Runtime(format!(
                "Source '{name}' load command failed: {}",
                stderr.trim()
            )));
        }
        Ok(parse_dotenv(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// Lazy cache for source loads within one resolution pass.
pub struct SourceCache {
    loaded: HashMap<String, HashMap<String, String>>,
}

impl SourceCache {
    pub fn new() -> Self {
        SourceCache {
            loaded: HashMap::new(),
        }
    }
}

impl Default for SourceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse dotenv-format output (KEY=VALUE, export KEY=VALUE, quoted values).
fn parse_dotenv(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            if key.is_empty() {
                continue;
            }
            let val = line[eq + 1..].trim();
            let val = if val.len() >= 2
                && ((val.starts_with('"') && val.ends_with('"'))
                    || (val.starts_with('\'') && val.ends_with('\'')))
            {
                val[1..val.len() - 1].to_string()
            } else {
                val.to_string()
            };
            map.insert(key, val);
        }
    }
    map
}

fn is_source(scheme: &str, sources: &HashMap<String, SourceSpec>) -> bool {
    scheme == "env" || sources.contains_key(scheme)
}

fn source_is_secret(source_name: &str, sources: &HashMap<String, SourceSpec>) -> bool {
    sources.get(source_name).is_some_and(|s| s.is_secret())
}

/// Resolve a field from a source, lazily loading and caching the source.
fn resolve_from_source(
    source_name: &str,
    field: &str,
    sources: &HashMap<String, SourceSpec>,
    runtime_vars: &HashMap<String, String>,
    cwd: Option<&Path>,
    cache: &mut SourceCache,
) -> Result<String> {
    if !cache.loaded.contains_key(source_name) {
        let data = if source_name == "env" {
            std::env::vars().collect()
        } else {
            let source = sources
                .get(source_name)
                .ok_or_else(|| Error::Runtime(format!("Source '{source_name}' not found")))?;
            source.load(source_name, runtime_vars, cwd)?
        };
        cache.loaded.insert(source_name.to_string(), data);
    }
    let data = cache.loaded.get(source_name).unwrap();
    data.get(field).cloned().ok_or_else(|| {
        if source_name == "env" {
            Error::Runtime(format!("Environment variable '{field}' is not set"))
        } else {
            Error::Runtime(format!(
                "Field '{field}' not found in source '{source_name}'"
            ))
        }
    })
}

/// Resolve a single provider or source URI. Returns `(resolved_value, is_secret)`.
pub fn resolve_provider(
    value: &str,
    providers: &HashMap<String, ProviderSpec>,
    sources: &HashMap<String, SourceSpec>,
    runtime_vars: &HashMap<String, String>,
    cwd: Option<&Path>,
    source_cache: &mut SourceCache,
) -> Result<(String, bool)> {
    let (value, secret) = if let Some(inner) = value.strip_prefix("secret://") {
        (inner, true)
    } else {
        (value, false)
    };

    let m = match SCHEME_RE.captures(value) {
        Some(m) => m,
        None => return Ok((value.to_string(), secret)),
    };
    let scheme = &m[1];

    if scheme == "file" {
        return resolve_file(&value[7..], cwd).map(|v| (v, secret));
    }

    // Sources take priority over providers
    if is_source(scheme, sources) {
        let field = &value[scheme.len() + 3..];
        let resolved =
            resolve_from_source(scheme, field, sources, runtime_vars, cwd, source_cache)?;
        let is_secret = secret || source_is_secret(scheme, sources);
        return Ok((resolved, is_secret));
    }

    let provider = match providers.get(scheme) {
        Some(p) => p,
        None => return Ok((value.to_string(), secret)),
    };

    let path = &value[scheme.len() + 3..];
    let resolved = provider.resolve(scheme, value, path, runtime_vars, cwd)?;
    Ok((resolved, secret))
}

pub fn resolve_providers(
    node: &Value,
    providers: &HashMap<String, ProviderSpec>,
    sources: &HashMap<String, SourceSpec>,
    runtime_vars: &HashMap<String, String>,
    cwd: Option<&Path>,
    secrets: &mut HashSet<String>,
    source_cache: &mut SourceCache,
) -> Result<Value> {
    resolve_providers_inner(
        node,
        providers,
        sources,
        runtime_vars,
        cwd,
        "",
        secrets,
        source_cache,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_providers_inner(
    node: &Value,
    providers: &HashMap<String, ProviderSpec>,
    sources: &HashMap<String, SourceSpec>,
    runtime_vars: &HashMap<String, String>,
    cwd: Option<&Path>,
    prefix: &str,
    secrets: &mut HashSet<String>,
    source_cache: &mut SourceCache,
) -> Result<Value> {
    match node {
        Value::String(s) => {
            let (resolved, is_secret) =
                resolve_provider(s, providers, sources, runtime_vars, cwd, source_cache)?;
            if is_secret && !prefix.is_empty() {
                secrets.insert(prefix.to_string());
            }
            Ok(Value::String(resolved))
        }
        Value::Table(map) => {
            let mut new_map = Map::new();
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                new_map.insert(
                    k.clone(),
                    resolve_providers_inner(
                        v,
                        providers,
                        sources,
                        runtime_vars,
                        cwd,
                        &path,
                        secrets,
                        source_cache,
                    )?,
                );
            }
            Ok(Value::Table(new_map))
        }
        Value::Array(arr) => {
            let mut new_arr = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                let path = format!("{prefix}[{i}]");
                new_arr.push(resolve_providers_inner(
                    item,
                    providers,
                    sources,
                    runtime_vars,
                    cwd,
                    &path,
                    secrets,
                    source_cache,
                )?);
            }
            Ok(Value::Array(new_arr))
        }
        other => Ok(other.clone()),
    }
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
        })
    }

    /// Resolve one `scheme://...` (or plain) value against this config's
    /// providers/sources/vars/cwd -- [`resolve_provider`] with the config's
    /// own fields already supplied.
    pub fn resolve_provider(&self, value: &str, cache: &mut SourceCache) -> Result<(String, bool)> {
        resolve_provider(
            value,
            &self.providers,
            &self.sources,
            &self.merged_vars,
            Some(&self.config_dir),
            cache,
        )
    }

    /// Recursively resolve every string leaf in `node` this way, tracking
    /// which paths turned out secret -- [`resolve_providers`] with the
    /// config's own fields already supplied.
    fn resolve_providers(
        &self,
        node: &Value,
        secrets: &mut HashSet<String>,
        cache: &mut SourceCache,
    ) -> Result<Value> {
        resolve_providers(
            node,
            &self.providers,
            &self.sources,
            &self.merged_vars,
            Some(&self.config_dir),
            secrets,
            cache,
        )
    }

    pub fn resolve(&self, dotted_path: &str, eval_providers: bool) -> Result<Resolved> {
        let node = get(&self.tree, dotted_path)?;
        let value = interpolate_node(node, &self.tree)?;
        let mut secrets = HashSet::new();
        let mut source_cache = SourceCache::new();
        let (value, is_leaf_secret) = if eval_providers {
            let value = eval_shells(&value, Some(&self.config_dir))?;
            let leaf_secret = match &value {
                Value::String(s) => self.resolve_provider(s, &mut source_cache)?.1,
                _ => false,
            };
            let resolved = self.resolve_providers(&value, &mut secrets, &mut source_cache)?;
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

    /// Resolve a single section's leaf keys into env pairs, sharing a
    /// [`SourceCache`] across calls so a bulk source loads at most once.
    fn env_with_cache(
        &self,
        dotted_path: &str,
        eval_providers: bool,
        source_cache: &mut SourceCache,
    ) -> Result<Vec<EnvPair>> {
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
            self.resolve_providers(&leaves, &mut secrets, source_cache)?
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

    pub fn env(&self, dotted_path: &str, eval_providers: bool) -> Result<Vec<EnvPair>> {
        let mut source_cache = SourceCache::new();
        self.env_with_cache(dotted_path, eval_providers, &mut source_cache)
    }

    /// Resolve one or more sections into a single ordered set of env pairs.
    ///
    /// Sections are composed left-to-right; on a key conflict the later
    /// section wins (its value replaces the earlier one, keeping the
    /// original position).
    pub fn env_multi(&self, dotted_paths: &[String], eval_providers: bool) -> Result<Vec<EnvPair>> {
        let mut source_cache = SourceCache::new();
        let mut order: Vec<String> = Vec::new();
        let mut by_key: HashMap<String, EnvPair> = HashMap::new();
        for path in dotted_paths {
            for pair in self.env_with_cache(path, eval_providers, &mut source_cache)? {
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
        let mut source_cache = SourceCache::new();

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
            &mut source_cache,
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
        let mut source_cache = SourceCache::new();
        if eval_providers {
            resolved = eval_shells(&resolved, Some(&self.config_dir))?;
            resolved = self.resolve_providers(&resolved, &mut secrets, &mut source_cache)?;
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
            let mut source_cache = SourceCache::new();
            self.resolve_providers(&evaled, &mut secrets, &mut source_cache)
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

    fn write_config(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("confit.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    fn empty_sources() -> HashMap<String, SourceSpec> {
        HashMap::new()
    }

    fn empty_providers() -> HashMap<String, ProviderSpec> {
        HashMap::new()
    }

    fn new_cache() -> SourceCache {
        SourceCache::new()
    }

    #[test]
    fn test_get_simple() {
        let config: Value = r#"
            [app]
            name = "myapp"
            port = 8080
        "#
        .parse()
        .unwrap();
        assert_eq!(get(&config, "app.name").unwrap().as_str().unwrap(), "myapp");
        assert_eq!(
            get(&config, "app.port").unwrap().as_integer().unwrap(),
            8080
        );
    }

    #[test]
    fn test_get_nested() {
        let config: Value = r#"
            [a.b.c]
            d = "deep"
        "#
        .parse()
        .unwrap();
        assert_eq!(get(&config, "a.b.c.d").unwrap().as_str().unwrap(), "deep");
    }

    #[test]
    fn test_get_missing_path() {
        let config: Value = "[app]\nname = \"x\"".parse().unwrap();
        assert!(get(&config, "app.missing").is_err());
        assert!(get(&config, "nope").is_err());
    }

    #[test]
    fn test_interpolation_basic() {
        let config: Value = r#"
            [vars]
            env = "prod"
            [app]
            name = "myapp-{vars.env}"
        "#
        .parse()
        .unwrap();
        let result = interpolate_value("myapp-{vars.env}", &config, &HashSet::new()).unwrap();
        assert_eq!(result, "myapp-prod");
    }

    #[test]
    fn test_interpolation_multiple_refs() {
        let config: Value = r#"
            [vars]
            a = "hello"
            b = "world"
        "#
        .parse()
        .unwrap();
        let result = interpolate_value("{vars.a} {vars.b}", &config, &HashSet::new()).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_interpolation_cycle_detection() {
        let config: Value = r#"
            [app]
            a = "{app.b}"
            b = "{app.a}"
        "#
        .parse()
        .unwrap();
        let mut resolving = HashSet::new();
        resolving.insert("app.b".to_string());
        let result = interpolate_value("{app.a}", &config, &resolving);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circular"));
    }

    #[test]
    fn test_interpolation_no_refs() {
        let config: Value = "x = 1".parse().unwrap();
        let result = interpolate_value("plain text", &config, &HashSet::new()).unwrap();
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_interpolate_node_table() {
        let config: Value = r#"
            [vars]
            region = "us-east-1"
            [deploy]
            target = "deploy-{vars.region}"
            count = 3
        "#
        .parse()
        .unwrap();
        let node = get(&config, "deploy").unwrap();
        let result = interpolate_node(node, &config).unwrap();
        let table = result.as_table().unwrap();
        assert_eq!(table["target"].as_str().unwrap(), "deploy-us-east-1");
        assert_eq!(table["count"].as_integer().unwrap(), 3);
    }

    #[test]
    fn test_eval_shell() {
        let result = eval_shell("hello $(echo world)", None).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_eval_shell_no_expression() {
        let result = eval_shell("plain text", None).unwrap();
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_eval_shell_multiple() {
        let result = eval_shell("$(echo a)-$(echo b)", None).unwrap();
        assert_eq!(result, "a-b");
    }

    #[test]
    fn test_file_provider() {
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("secret.txt");
        std::fs::write(&secret_path, "s3cret_value\n").unwrap();

        let providers = empty_providers();
        let vars = HashMap::new();
        let uri = format!("file://{}", secret_path.display());
        let (result, secret) = resolve_provider(
            &uri,
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "s3cret_value");
        assert!(!secret);
    }

    #[test]
    fn test_file_provider_relative() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("token.txt"), "abc123\n").unwrap();

        let providers = empty_providers();
        let vars = HashMap::new();
        let (result, _) = resolve_provider(
            "file://token.txt",
            &providers,
            &empty_sources(),
            &vars,
            Some(dir.path()),
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "abc123");
    }

    #[test]
    fn test_file_provider_missing() {
        let providers = empty_providers();
        let vars = HashMap::new();
        let result = resolve_provider(
            "file:///nonexistent/file.txt",
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_custom_provider() {
        let providers: HashMap<String, ProviderSpec> = r#"
            [echo]
            cmd = "echo resolved-{path}"
        "#
        .parse::<Value>()
        .unwrap()
        .try_into()
        .unwrap();
        let vars = HashMap::new();
        let (result, _) = resolve_provider(
            "echo://some/path",
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "resolved-some/path");
    }

    #[test]
    fn test_custom_provider_with_vars() {
        let providers: HashMap<String, ProviderSpec> = r#"
            [vault]
            cmd = "echo {stage}-{path}"
        "#
        .parse::<Value>()
        .unwrap()
        .try_into()
        .unwrap();
        let mut vars = HashMap::new();
        vars.insert("stage".into(), "prod".into());
        let (result, _) = resolve_provider(
            "vault://db/password",
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "prod-db/password");
    }

    #[test]
    fn test_secret_scheme_strips_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("key.txt"), "my-secret\n").unwrap();

        let providers = empty_providers();
        let vars = HashMap::new();
        let (result, secret) = resolve_provider(
            "secret://file://key.txt",
            &providers,
            &empty_sources(),
            &vars,
            Some(dir.path()),
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "my-secret");
        assert!(secret);
    }

    #[test]
    fn test_secret_scheme_with_literal() {
        let providers = empty_providers();
        let vars = HashMap::new();
        let (result, secret) = resolve_provider(
            "secret://literal-value",
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "literal-value");
        assert!(secret);
    }

    #[test]
    fn test_secret_scheme_with_custom_provider() {
        let providers: HashMap<String, ProviderSpec> = r#"
            [echo]
            cmd = "echo secret-{path}"
        "#
        .parse::<Value>()
        .unwrap()
        .try_into()
        .unwrap();
        let vars = HashMap::new();
        let (result, secret) = resolve_provider(
            "secret://echo://data",
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(result, "secret-data");
        assert!(secret);
    }

    #[test]
    fn test_non_secret_not_flagged() {
        let providers: HashMap<String, ProviderSpec> = r#"
            [echo]
            cmd = "echo val"
        "#
        .parse::<Value>()
        .unwrap()
        .try_into()
        .unwrap();
        let vars = HashMap::new();
        let (_, secret) = resolve_provider(
            "echo://whatever",
            &providers,
            &empty_sources(),
            &vars,
            None,
            &mut new_cache(),
        )
        .unwrap();
        assert!(!secret);
    }

    #[test]
    fn test_secret_tracking_in_resolve_providers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pass.txt"), "hunter2\n").unwrap();

        let node: Value = r#"
            token = "secret://file://pass.txt"
            host = "localhost"
        "#
        .parse()
        .unwrap();
        let providers = empty_providers();
        let vars = HashMap::new();
        let mut secrets = HashSet::new();
        let mut cache = new_cache();
        let result = resolve_providers(
            &node,
            &providers,
            &empty_sources(),
            &vars,
            Some(dir.path()),
            &mut secrets,
            &mut cache,
        )
        .unwrap();
        let table = result.as_table().unwrap();
        assert_eq!(table["token"].as_str().unwrap(), "hunter2");
        assert_eq!(table["host"].as_str().unwrap(), "localhost");
        assert!(secrets.contains("token"));
        assert!(!secrets.contains("host"));
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
        let node = get(&bc.tree, "app.name").unwrap();
        let result = interpolate_node(node, &bc.tree).unwrap();
        assert_eq!(result.as_str().unwrap(), "svc-test");
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
        let node = get(&bc.tree, "db.password").unwrap();
        let interpolated = interpolate_node(node, &bc.tree).unwrap();
        let (resolved, _) = resolve_provider(
            interpolated.as_str().unwrap(),
            &bc.providers,
            &bc.sources,
            &bc.merged_vars,
            Some(&bc.config_dir),
            &mut new_cache(),
        )
        .unwrap();
        assert_eq!(resolved, "hunter2");
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
        let node = get(&bc.tree, "db.url").unwrap();
        let interpolated = interpolate_node(node, &bc.tree).unwrap();
        assert_eq!(
            interpolated.as_str().unwrap(),
            "postgres://localhost:4300/mydb"
        );
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
        let node = get(&bc.tree, "build.hash").unwrap();
        let interpolated = interpolate_node(node, &bc.tree).unwrap();
        let evaled = eval_shell(interpolated.as_str().unwrap(), Some(&bc.config_dir)).unwrap();
        assert_eq!(evaled, "abc123");
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
        let node = get(&bc.tree, "db").unwrap();
        let interpolated = interpolate_node(node, &bc.tree).unwrap();
        let table = interpolated.as_table().unwrap();

        let mut pairs: Vec<(String, String)> = table
            .iter()
            .filter(|(_, v)| !v.is_table())
            .map(|(k, v)| (k.clone(), value_to_string(v)))
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(pairs.len(), 3);
        assert!(pairs.iter().any(|(k, v)| k == "host" && v == "localhost"));
        assert!(pairs.iter().any(|(k, v)| k == "port" && v == "5432"));
        assert!(pairs.iter().any(|(k, v)| k == "name" && v == "mydb-dev"));
    }

    #[test]
    fn test_expand_template() {
        let mut vars = HashMap::new();
        vars.insert("path".into(), "secret/key".into());
        vars.insert("stage".into(), "prod".into());
        let result =
            expand_template("fetch {stage} {path}", &vars, "vault", "vault://secret/key").unwrap();
        assert_eq!(result, "fetch prod secret/key");
    }

    #[test]
    fn test_expand_template_missing_var() {
        let vars = HashMap::new();
        let result = expand_template("fetch {missing}", &vars, "vault", "vault://x");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing"));
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
    fn test_array_interpolation() {
        let config: Value = r#"
            [vars]
            tag = "v1"
            [deploy]
            tags = ["latest", "{vars.tag}"]
        "#
        .parse()
        .unwrap();
        let node = get(&config, "deploy.tags").unwrap();
        let result = interpolate_node(node, &config).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), "latest");
        assert_eq!(arr[1].as_str().unwrap(), "v1");
    }

    // --- Source tests ---

    #[test]
    fn test_parse_dotenv_basic() {
        let output = "FOO=bar\nBAZ=qux\n";
        let map = parse_dotenv(output);
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["BAZ"], "qux");
    }

    #[test]
    fn test_parse_dotenv_export_prefix() {
        let output = "export FOO=bar\nexport BAZ=qux\n";
        let map = parse_dotenv(output);
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["BAZ"], "qux");
    }

    #[test]
    fn test_parse_dotenv_quoted() {
        let output = "FOO=\"bar baz\"\nBAZ='qux quux'\n";
        let map = parse_dotenv(output);
        assert_eq!(map["FOO"], "bar baz");
        assert_eq!(map["BAZ"], "qux quux");
    }

    #[test]
    fn test_parse_dotenv_comments_and_blanks() {
        let output = "# comment\n\nFOO=bar\n# another\nBAZ=qux\n";
        let map = parse_dotenv(output);
        assert_eq!(map.len(), 2);
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["BAZ"], "qux");
    }

    #[test]
    fn test_source_string_shorthand() {
        let sources = HashMap::from([(
            "mysrc".to_string(),
            SourceSpec::Shorthand("echo FOO=hello".into()),
        )]);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result =
            resolve_from_source("mysrc", "FOO", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(result, "hello");
        // Verify caching: loaded should contain mysrc
        assert!(cache.loaded.contains_key("mysrc"));
    }

    #[test]
    fn test_source_table_form() {
        let sources = HashMap::from([(
            "mysrc".to_string(),
            SourceSpec::Full {
                load: "echo BAR=world".into(),
                secret: false,
            },
        )]);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result =
            resolve_from_source("mysrc", "BAR", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn test_source_missing_field_errors() {
        let sources = HashMap::from([(
            "mysrc".to_string(),
            SourceSpec::Shorthand("echo FOO=hello".into()),
        )]);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result = resolve_from_source("mysrc", "NOPE", &sources, &vars, None, &mut cache);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NOPE"));
    }

    #[test]
    fn test_source_cached_single_load() {
        // The source outputs a random suffix each call; caching means we get the same value twice
        let sources = HashMap::from([(
            "mysrc".to_string(),
            SourceSpec::Shorthand("echo FOO=$(date +%N)".into()),
        )]);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let first = resolve_from_source("mysrc", "FOO", &sources, &vars, None, &mut cache).unwrap();
        let second =
            resolve_from_source("mysrc", "FOO", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(first, second, "second call should return cached value");
    }

    #[test]
    fn test_source_via_resolve_provider() {
        let sources = HashMap::from([(
            "myenv".to_string(),
            SourceSpec::Shorthand("echo KEY=resolved".into()),
        )]);

        let providers = empty_providers();
        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let (val, secret) =
            resolve_provider("myenv://KEY", &providers, &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(val, "resolved");
        assert!(!secret);
    }

    #[test]
    fn test_source_secret_flag() {
        let sources = HashMap::from([(
            "vault".to_string(),
            SourceSpec::Full {
                load: "echo PASS=hunter2".into(),
                secret: true,
            },
        )]);

        let providers = empty_providers();
        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let (val, secret) = resolve_provider(
            "vault://PASS",
            &providers,
            &sources,
            &vars,
            None,
            &mut cache,
        )
        .unwrap();
        assert_eq!(val, "hunter2");
        assert!(
            secret,
            "source with secret=true should mark field as secret"
        );
    }

    #[test]
    fn test_source_secret_prefix_composes() {
        let sources = HashMap::from([(
            "plain".to_string(),
            SourceSpec::Shorthand("echo TOKEN=abc123".into()),
        )]);

        let providers = empty_providers();
        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let (val, secret) = resolve_provider(
            "secret://plain://TOKEN",
            &providers,
            &sources,
            &vars,
            None,
            &mut cache,
        )
        .unwrap();
        assert_eq!(val, "abc123");
        assert!(secret, "secret:// prefix should mark as secret");
    }

    #[test]
    fn test_env_source_builtin() {
        unsafe { std::env::set_var("TEST_CONFIT_BUILTIN", "hello_builtin") };
        let providers = empty_providers();
        let sources = empty_sources();
        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let (val, _) = resolve_provider(
            "env://TEST_CONFIT_BUILTIN",
            &providers,
            &sources,
            &vars,
            None,
            &mut cache,
        )
        .unwrap();
        assert_eq!(val, "hello_builtin");
        unsafe { std::env::remove_var("TEST_CONFIT_BUILTIN") };
    }

    #[test]
    fn test_env_source_missing_errors() {
        std::env::remove_var("TEST_CONFIT_DEFINITELY_NOT_SET_XYZ");
        let providers = empty_providers();
        let sources = empty_sources();
        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result = resolve_provider(
            "env://TEST_CONFIT_DEFINITELY_NOT_SET_XYZ",
            &providers,
            &sources,
            &vars,
            None,
            &mut cache,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("TEST_CONFIT_DEFINITELY_NOT_SET_XYZ"));
    }

    #[test]
    fn test_source_vars_interpolation() {
        let sources = HashMap::from([(
            "mysrc".to_string(),
            SourceSpec::Shorthand("echo STAGE={vars.stage}".into()),
        )]);

        let mut vars = HashMap::new();
        vars.insert("stage".into(), "prod".into());
        let mut cache = SourceCache::new();
        let result =
            resolve_from_source("mysrc", "STAGE", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(result, "prod");
    }

    #[test]
    fn test_source_rejects_path_in_template() {
        let sources = HashMap::from([(
            "bad".to_string(),
            SourceSpec::Shorthand("echo {path}=value".into()),
        )]);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result = resolve_from_source("bad", "FOO", &sources, &vars, None, &mut cache);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path"));
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
        let node = get(&bc.tree, "app.val").unwrap();
        let interpolated = interpolate_node(node, &bc.tree).unwrap();
        let mut cache = SourceCache::new();
        let (resolved, _) = resolve_provider(
            interpolated.as_str().unwrap(),
            &bc.providers,
            &bc.sources,
            &bc.merged_vars,
            Some(&bc.config_dir),
            &mut cache,
        )
        .unwrap();
        assert_eq!(resolved, "from_source");
    }
}
