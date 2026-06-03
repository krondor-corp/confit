use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
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

// --- Sources ---

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

fn source_is_secret(source_name: &str, sources: &Value) -> bool {
    if let Some(table) = sources.as_table() {
        if let Some(Value::Table(t)) = table.get(source_name) {
            if let Some(Value::Boolean(b)) = t.get("secret") {
                return *b;
            }
        }
    }
    false
}

/// Load a source's data (running the load command if needed).
/// Returns the parsed key→value map.
fn load_source_data(
    source_name: &str,
    sources: &Value,
    runtime_vars: &HashMap<String, String>,
    cwd: Option<&Path>,
) -> Result<HashMap<String, String>> {
    if source_name == "env" {
        return Ok(std::env::vars().collect());
    }

    let sources_table = sources
        .as_table()
        .ok_or_else(|| Error::Runtime(format!("Source '{source_name}' not found")))?;
    let source = sources_table
        .get(source_name)
        .ok_or_else(|| Error::Runtime(format!("Source '{source_name}' not found")))?;

    let load_template = match source {
        Value::String(s) => s.as_str(),
        Value::Table(t) => match t.get("load").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return Err(Error::Runtime(format!(
                    "Source '{source_name}' table must have a 'load' field"
                )))
            }
        },
        _ => {
            return Err(Error::Runtime(format!(
                "Source '{source_name}' must be a string or table"
            )))
        }
    };

    // Reject {path} and {uri} — sources load a bag, they have no per-key path
    for cap in REF_RE.captures_iter(load_template) {
        let key = &cap[1];
        if key == "path" || key == "uri" {
            return Err(Error::Runtime(format!(
                "Source '{source_name}' load template references '{{{key}}}' which is not \
                 allowed (sources don't have per-key paths). \
                 Use a provider for per-key substitution."
            )));
        }
    }

    // Support both {stage} and {vars.stage} in source load templates
    let template_vars: HashMap<String, String> = runtime_vars
        .iter()
        .flat_map(|(k, v)| [(k.clone(), v.clone()), (format!("vars.{k}"), v.clone())])
        .collect();

    let cmd = expand_template(load_template, &template_vars, source_name, source_name)?;

    let mut command = Command::new("sh");
    command.args(["-c", &cmd]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command
        .output()
        .map_err(|e| Error::Runtime(format!("Source '{source_name}' load failed: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Runtime(format!(
            "Source '{source_name}' load command failed: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_dotenv(&stdout))
}

/// Resolve a field from a source, lazily loading and caching the source.
fn resolve_from_source(
    source_name: &str,
    field: &str,
    sources: &Value,
    runtime_vars: &HashMap<String, String>,
    cwd: Option<&Path>,
    cache: &mut SourceCache,
) -> Result<String> {
    if !cache.loaded.contains_key(source_name) {
        let data = load_source_data(source_name, sources, runtime_vars, cwd)?;
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

fn is_source(scheme: &str, sources: &Value) -> bool {
    if scheme == "env" {
        return true;
    }
    sources.as_table().is_some_and(|t| t.contains_key(scheme))
}

/// Resolve a single provider or source URI. Returns `(resolved_value, is_secret)`.
pub fn resolve_provider(
    value: &str,
    providers: &Value,
    sources: &Value,
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

    let providers_table = match providers.as_table() {
        Some(t) => t,
        None => return Ok((value.to_string(), secret)),
    };
    let provider = match providers_table.get(scheme) {
        Some(p) => p,
        None => return Ok((value.to_string(), secret)),
    };

    let cmd_template = match provider {
        Value::Table(t) => match t.get("cmd").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Ok((value.to_string(), secret)),
        },
        Value::String(s) => s.as_str(),
        _ => return Ok((value.to_string(), secret)),
    };

    let path = &value[scheme.len() + 3..];
    let mut template_vars = runtime_vars.clone();
    template_vars.insert("uri".into(), value.into());
    template_vars.insert("path".into(), path.into());

    let cmd = expand_template(cmd_template, &template_vars, scheme, value)?;

    let mut command = Command::new("sh");
    command.args(["-c", &cmd]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command
        .output()
        .map_err(|e| Error::Runtime(format!("Provider {scheme}: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Runtime(format!(
            "Failed to eval '{value}' via {scheme}: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok((stdout.trim().to_string(), secret))
}

pub fn resolve_providers(
    node: &Value,
    providers: &Value,
    sources: &Value,
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
    providers: &Value,
    sources: &Value,
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

pub struct BuiltConfig {
    pub config: Value,
    pub providers: Value,
    pub sources: Value,
    pub merged_vars: HashMap<String, String>,
    pub config_dir: PathBuf,
}

pub fn build_config(
    path: Option<&Path>,
    runtime_vars: &HashMap<String, String>,
) -> Result<BuiltConfig> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => find_config()?,
    };
    let mut raw = load_raw(&path)?;

    let table = raw
        .as_table_mut()
        .ok_or_else(|| Error::Runtime("Config root must be a table".into()))?;

    let providers = table
        .remove("providers")
        .unwrap_or(Value::Table(Map::new()));

    let sources = table.remove("sources").unwrap_or(Value::Table(Map::new()));

    let vars_section: HashMap<String, String> = table
        .get("vars")
        .and_then(|v| v.as_table())
        .map(|t| {
            t.iter()
                .map(|(k, v)| (k.clone(), value_to_string(v)))
                .collect()
        })
        .unwrap_or_default();

    let env_vars = collect_env_vars();
    let mut merged_vars = vars_section;
    merged_vars.extend(env_vars);
    merged_vars.extend(runtime_vars.clone());

    // Update vars section in config with merged values
    let vars_table: Map<String, Value> = merged_vars
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    table.insert("vars".into(), Value::Table(vars_table));

    let config_dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    Ok(BuiltConfig {
        config: raw,
        providers,
        sources,
        merged_vars,
        config_dir,
    })
}

// --- High-level API ---

/// Resolved value with secret metadata.
pub struct Resolved {
    pub value: String,
    pub secret: bool,
}

pub fn resolve(
    dotted_path: &str,
    eval_providers: bool,
    runtime_vars: &HashMap<String, String>,
) -> Result<Resolved> {
    let bc = build_config(None, runtime_vars)?;
    let node = get(&bc.config, dotted_path)?;
    let value = interpolate_node(node, &bc.config)?;
    let mut secrets = HashSet::new();
    let mut source_cache = SourceCache::new();
    let (value, is_leaf_secret) = if eval_providers {
        let value = eval_shells(&value, Some(&bc.config_dir))?;
        let leaf_secret = match &value {
            Value::String(s) => {
                resolve_provider(
                    s,
                    &bc.providers,
                    &bc.sources,
                    &bc.merged_vars,
                    Some(&bc.config_dir),
                    &mut source_cache,
                )?
                .1
            }
            _ => false,
        };
        let resolved = resolve_providers(
            &value,
            &bc.providers,
            &bc.sources,
            &bc.merged_vars,
            Some(&bc.config_dir),
            &mut secrets,
            &mut source_cache,
        )?;
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

pub fn keys(dotted_path: &str, runtime_vars: &HashMap<String, String>) -> Result<Vec<String>> {
    let bc = build_config(None, runtime_vars)?;
    let node = get(&bc.config, dotted_path)?;
    match node.as_table() {
        Some(table) => Ok(table.keys().cloned().collect()),
        None => Err(Error::Lookup(format!("'{dotted_path}' is not a section"))),
    }
}

/// Env pair with secret metadata.
pub struct EnvPair {
    pub key: String,
    pub value: String,
    pub secret: bool,
}

pub fn env(
    dotted_path: &str,
    eval_providers: bool,
    runtime_vars: &HashMap<String, String>,
) -> Result<Vec<EnvPair>> {
    let bc = build_config(None, runtime_vars)?;
    let node = get(&bc.config, dotted_path)?;
    let interpolated = interpolate_node(node, &bc.config)?;
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
    let mut source_cache = SourceCache::new();
    let resolved = if eval_providers {
        let leaves = eval_shells(&leaves, Some(&bc.config_dir))?;
        resolve_providers(
            &leaves,
            &bc.providers,
            &bc.sources,
            &bc.merged_vars,
            Some(&bc.config_dir),
            &mut secrets,
            &mut source_cache,
        )?
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

pub fn validate(runtime_vars: &HashMap<String, String>) -> Result<Vec<(String, bool, String)>> {
    let bc = build_config(None, runtime_vars)?;
    let mut results = Vec::new();
    let mut source_cache = SourceCache::new();

    #[allow(clippy::too_many_arguments)]
    fn walk(
        node: &Value,
        prefix: &str,
        config: &Value,
        providers: &Value,
        sources: &Value,
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
        &bc.config,
        "",
        &bc.config,
        &bc.providers,
        &bc.sources,
        &bc.merged_vars,
        &bc.config_dir,
        &mut results,
        &mut source_cache,
    );
    Ok(results)
}

pub fn yaml_section(
    dotted_path: &str,
    eval_providers: bool,
    runtime_vars: &HashMap<String, String>,
    wrap: Option<&str>,
    reveal: bool,
) -> Result<String> {
    let bc = build_config(None, runtime_vars)?;
    let node = get(&bc.config, dotted_path)?;
    let mut resolved = interpolate_node(node, &bc.config)?;
    let mut secrets = HashSet::new();
    let mut source_cache = SourceCache::new();
    if eval_providers {
        resolved = eval_shells(&resolved, Some(&bc.config_dir))?;
        resolved = resolve_providers(
            &resolved,
            &bc.providers,
            &bc.sources,
            &bc.merged_vars,
            Some(&bc.config_dir),
            &mut secrets,
            &mut source_cache,
        )?;
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

pub fn load(eval_providers: bool, runtime_vars: &HashMap<String, String>) -> Result<Value> {
    let bc = build_config(None, runtime_vars)?;
    let interpolated = interpolate_node(&bc.config, &bc.config)?;
    if eval_providers {
        let evaled = eval_shells(&interpolated, Some(&bc.config_dir))?;
        let mut secrets = HashSet::new();
        let mut source_cache = SourceCache::new();
        resolve_providers(
            &evaled,
            &bc.providers,
            &bc.sources,
            &bc.merged_vars,
            Some(&bc.config_dir),
            &mut secrets,
            &mut source_cache,
        )
    } else {
        Ok(interpolated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_config(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("confit.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    fn empty_sources() -> Value {
        Value::Table(Map::new())
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

        let providers = Value::Table(Map::new());
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

        let providers = Value::Table(Map::new());
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
        let providers = Value::Table(Map::new());
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
        let providers: Value = r#"
            [echo]
            cmd = "echo resolved-{path}"
        "#
        .parse()
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
        let providers: Value = r#"
            [vault]
            cmd = "echo {stage}-{path}"
        "#
        .parse()
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

        let providers = Value::Table(Map::new());
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
        let providers = Value::Table(Map::new());
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
        let providers: Value = r#"
            [echo]
            cmd = "echo secret-{path}"
        "#
        .parse()
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
        let providers: Value = r#"
            [echo]
            cmd = "echo val"
        "#
        .parse()
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
        let providers = Value::Table(Map::new());
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
        let bc = build_config(Some(&path), &runtime).unwrap();
        assert_eq!(bc.merged_vars["env"], "prod");
    }

    #[test]
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        assert_eq!(bc.merged_vars["region"], "us-west-2");
        std::env::remove_var("CONFIT_VAR_REGION");
    }

    #[test]
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
        let bc = build_config(Some(&path), &runtime).unwrap();
        assert_eq!(bc.merged_vars["x"], "from-cli");
        std::env::remove_var("CONFIT_VAR_X");
    }

    #[test]
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        let node = get(&bc.config, "app.name").unwrap();
        let result = interpolate_node(node, &bc.config).unwrap();
        assert_eq!(result.as_str().unwrap(), "svc-test");
    }

    #[test]
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        let node = get(&bc.config, "db.password").unwrap();
        let interpolated = interpolate_node(node, &bc.config).unwrap();
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
    fn test_end_to_end_with_shell() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(
            dir.path(),
            r#"
            [build]
            hash = "$(echo abc123)"
            "#,
        );
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        let node = get(&bc.config, "build.hash").unwrap();
        let interpolated = interpolate_node(node, &bc.config).unwrap();
        let evaled = eval_shell(interpolated.as_str().unwrap(), Some(&bc.config_dir)).unwrap();
        assert_eq!(evaled, "abc123");
    }

    #[test]
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        let node = get(&bc.config, "db").unwrap();
        let interpolated = interpolate_node(node, &bc.config).unwrap();
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        assert!(bc.config.as_table().unwrap().get("providers").is_none());
        assert!(bc.providers.as_table().unwrap().contains_key("op"));
    }

    #[test]
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        assert!(bc.config.as_table().unwrap().get("sources").is_none());
        assert!(bc.sources.as_table().unwrap().contains_key("mysrc"));
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
        let mut sources_map = Map::new();
        sources_map.insert("mysrc".into(), Value::String("echo FOO=hello".into()));
        let sources = Value::Table(sources_map);

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
        let mut sources_map = Map::new();
        let mut src_table = Map::new();
        src_table.insert("load".into(), Value::String("echo BAR=world".into()));
        sources_map.insert("mysrc".into(), Value::Table(src_table));
        let sources = Value::Table(sources_map);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result =
            resolve_from_source("mysrc", "BAR", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn test_source_missing_field_errors() {
        let mut sources_map = Map::new();
        sources_map.insert("mysrc".into(), Value::String("echo FOO=hello".into()));
        let sources = Value::Table(sources_map);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result = resolve_from_source("mysrc", "NOPE", &sources, &vars, None, &mut cache);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NOPE"));
    }

    #[test]
    fn test_source_cached_single_load() {
        // The source outputs a random suffix each call; caching means we get the same value twice
        let mut sources_map = Map::new();
        sources_map.insert("mysrc".into(), Value::String("echo FOO=$(date +%N)".into()));
        let sources = Value::Table(sources_map);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let first = resolve_from_source("mysrc", "FOO", &sources, &vars, None, &mut cache).unwrap();
        let second =
            resolve_from_source("mysrc", "FOO", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(first, second, "second call should return cached value");
    }

    #[test]
    fn test_source_via_resolve_provider() {
        let mut sources_map = Map::new();
        sources_map.insert("myenv".into(), Value::String("echo KEY=resolved".into()));
        let sources = Value::Table(sources_map);

        let providers = Value::Table(Map::new());
        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let (val, secret) =
            resolve_provider("myenv://KEY", &providers, &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(val, "resolved");
        assert!(!secret);
    }

    #[test]
    fn test_source_secret_flag() {
        let mut sources_map = Map::new();
        let mut src = Map::new();
        src.insert("load".into(), Value::String("echo PASS=hunter2".into()));
        src.insert("secret".into(), Value::Boolean(true));
        sources_map.insert("vault".into(), Value::Table(src));
        let sources = Value::Table(sources_map);

        let providers = Value::Table(Map::new());
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
        let mut sources_map = Map::new();
        sources_map.insert("plain".into(), Value::String("echo TOKEN=abc123".into()));
        let sources = Value::Table(sources_map);

        let providers = Value::Table(Map::new());
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
        let providers = Value::Table(Map::new());
        let sources = Value::Table(Map::new());
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
        let providers = Value::Table(Map::new());
        let sources = Value::Table(Map::new());
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
        let mut sources_map = Map::new();
        sources_map.insert(
            "mysrc".into(),
            Value::String("echo STAGE={vars.stage}".into()),
        );
        let sources = Value::Table(sources_map);

        let mut vars = HashMap::new();
        vars.insert("stage".into(), "prod".into());
        let mut cache = SourceCache::new();
        let result =
            resolve_from_source("mysrc", "STAGE", &sources, &vars, None, &mut cache).unwrap();
        assert_eq!(result, "prod");
    }

    #[test]
    fn test_source_rejects_path_in_template() {
        let mut sources_map = Map::new();
        sources_map.insert("bad".into(), Value::String("echo {path}=value".into()));
        let sources = Value::Table(sources_map);

        let vars = HashMap::new();
        let mut cache = SourceCache::new();
        let result = resolve_from_source("bad", "FOO", &sources, &vars, None, &mut cache);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path"));
    }

    #[test]
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
        let bc = build_config(Some(&path), &HashMap::new()).unwrap();
        let node = get(&bc.config, "app.val").unwrap();
        let interpolated = interpolate_node(node, &bc.config).unwrap();
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
