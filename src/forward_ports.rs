use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Parse `forwardPorts` from a JSON value, supporting multiple formats:
/// - Numbers: `3000`
/// - Strings: `"3000"`, `"localhost:3000"`
/// - Objects: `{"port": 3000}`
pub fn parse_forward_ports_from_value(value: &Value) -> Vec<u16> {
    let Some(arr) = value.get("forwardPorts").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    arr.iter()
        .filter_map(|entry| match entry {
            Value::Number(n) => n.as_u64().map(|p| p as u16),
            Value::String(s) => {
                // Handle "localhost:3000" or just "3000"
                let port_str = s.rsplit(':').next().unwrap_or(s);
                port_str.parse::<u16>().ok()
            }
            Value::Object(obj) => obj
                .get("port")
                .and_then(|v| v.as_u64())
                .map(|p| p as u16),
            _ => None,
        })
        .collect()
}

/// Read a JSONC file (JSON with `//` line comments) and parse it.
pub fn read_jsonc(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let stripped: String = content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    serde_json::from_str(&stripped)
        .with_context(|| format!("failed to parse JSON from {}", path.display()))
}

/// Load forward ports from devcontainer.json, with optional override
/// from devcontainer.local.json.
pub fn load_forward_ports(workspace_root: &Path) -> Result<Vec<u16>> {
    let dc_dir = workspace_root.join(".devcontainer");

    let local_path = dc_dir.join("devcontainer.local.json");
    if local_path.exists() {
        let value = read_jsonc(&local_path)?;
        let ports = parse_forward_ports_from_value(&value);
        if !ports.is_empty() {
            return Ok(ports);
        }
    }

    let main_path = dc_dir.join("devcontainer.json");
    if main_path.exists() {
        let value = read_jsonc(&main_path)?;
        return Ok(parse_forward_ports_from_value(&value));
    }

    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_number_ports() {
        let val = json!({"forwardPorts": [3000, 8080]});
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000, 8080]);
    }

    #[test]
    fn parse_string_ports() {
        let val = json!({"forwardPorts": ["3000", "localhost:8080"]});
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000, 8080]);
    }

    #[test]
    fn parse_object_ports() {
        let val = json!({"forwardPorts": [{"port": 3000}, {"port": 9090}]});
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000, 9090]);
    }

    #[test]
    fn parse_mixed_ports() {
        let val = json!({"forwardPorts": [3000, "localhost:8080", {"port": 9090}]});
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000, 8080, 9090]);
    }

    #[test]
    fn parse_missing_forward_ports() {
        let val = json!({"name": "test"});
        assert_eq!(parse_forward_ports_from_value(&val), Vec::<u16>::new());
    }

    #[test]
    fn read_jsonc_strips_comments() {
        let dir = std::env::temp_dir().join("dc-test-jsonc");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.jsonc");
        fs::write(
            &path,
            r#"
// This is a comment
{
    // another comment
    "forwardPorts": [3000]
}
"#,
        )
        .unwrap();

        let val = read_jsonc(&path).unwrap();
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000]);

        let _ = fs::remove_dir_all(&dir);
    }
}
