use toml::Value;

pub fn to_yaml(data: &Value) -> String {
    let lines = to_yaml_lines(data, 0);
    lines.join("\n")
}

fn yaml_scalar(value: &Value) -> String {
    match value {
        Value::Boolean(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => yaml_quote_string(s),
        Value::Datetime(d) => d.to_string(),
        _ => String::new(),
    }
}

fn yaml_quote_string(s: &str) -> String {
    if s.contains('\n') {
        return s.to_string();
    }
    if s.is_empty()
        || s.contains(|c: char| ":{}&*?|>',[]%@`\"\\#".contains(c))
        || matches!(
            s.to_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no"
        )
    {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("\"{escaped}\"");
    }
    s.to_string()
}

fn to_yaml_lines(data: &Value, indent: usize) -> Vec<String> {
    let prefix = "  ".repeat(indent);
    let mut lines = Vec::new();

    match data {
        Value::Table(map) => {
            for (k, v) in map {
                match v {
                    Value::Table(_) => {
                        lines.push(format!("{prefix}{k}:"));
                        lines.extend(to_yaml_lines(v, indent + 1));
                    }
                    Value::Array(arr) => {
                        lines.push(format!("{prefix}{k}:"));
                        for item in arr {
                            if item.is_table() {
                                lines.push(format!("{prefix}-"));
                                lines.extend(to_yaml_lines(item, indent + 1));
                            } else {
                                lines.push(format!("{prefix}- {}", yaml_scalar(item)));
                            }
                        }
                    }
                    Value::String(s) if s.contains('\n') => {
                        lines.push(format!("{prefix}{k}: |"));
                        for text_line in s.split('\n') {
                            lines.push(format!("{prefix}  {text_line}"));
                        }
                    }
                    _ => {
                        lines.push(format!("{prefix}{k}: {}", yaml_scalar(v)));
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                if item.is_table() {
                    lines.push(format!("{prefix}-"));
                    lines.extend(to_yaml_lines(item, indent + 1));
                } else {
                    lines.push(format!("{prefix}- {}", yaml_scalar(item)));
                }
            }
        }
        _ => {
            lines.push(format!("{prefix}{}", yaml_scalar(data)));
        }
    }

    lines
}
