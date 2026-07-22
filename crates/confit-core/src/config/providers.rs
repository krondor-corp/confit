//! `scheme://path` resolution: `[providers.<scheme>]` and `[sources.<name>]`
//! each accept a bare-string shorthand or a table in TOML; `ProviderSpec`
//! and `SourceSpec` parse either directly via serde instead of every call
//! site doing its own `.as_table()`/`.get(...)`/`.as_str()` walk. Each is a
//! small "runnable" type -- `ProviderSpec::resolve` and `SourceSpec::load`
//! are the only places that actually shell out.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use toml::map::Map;
use toml::Value;

use crate::error::{Error, Result};

use super::interpolate::REF_RE;

static SCHEME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-zA-Z][a-zA-Z0-9_-]*)://").unwrap());

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
