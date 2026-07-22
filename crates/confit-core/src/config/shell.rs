//! The `$(...)` engine: find shell-command substrings in a string and
//! replace each with its stdout.

use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use toml::map::Map;
use toml::Value;

use crate::error::{Error, Result};

static SHELL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\((.+?)\)").unwrap());

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
