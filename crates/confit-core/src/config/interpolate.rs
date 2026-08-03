//! The `{ref}` engine: dotted-path lookups into a `toml::Value` tree, and
//! recursive `{a.b.c}` string interpolation against it.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use toml::map::Map;
use toml::Value;

use crate::error::{Error, Result};

pub(crate) static REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([a-zA-Z0-9_.-]+)\}").unwrap());

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
                        "Variable '{var_name}' is not declared. \
                         Add it to [vars] in confit.toml (a default or \"\"), \
                         then override with --set {var_name}=VALUE or CONFIT_VAR_{}",
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

pub(crate) fn value_to_string(v: &Value) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
