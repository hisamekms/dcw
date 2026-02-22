use anyhow::Result;
use serde_json::Value;
use std::path::Path;

use crate::config;

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
            Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
            Value::String(s) => {
                // Handle "localhost:3000" or just "3000"
                let port_str = s.rsplit(':').next().unwrap_or(s);
                port_str.parse::<u16>().ok()
            }
            Value::Object(obj) => obj
                .get("port")
                .and_then(|v| v.as_u64())
                .and_then(|p| u16::try_from(p).ok()),
            _ => None,
        })
        .collect()
}

/// Load forward ports from the resolved devcontainer config.
///
/// If a local override exists, uses the merged config; otherwise reads
/// devcontainer.json directly.
pub fn load_forward_ports(workspace_root: &Path) -> Result<Vec<u16>> {
    let config_path = match config::resolve_config(workspace_root)? {
        Some(merged) => merged,
        None => {
            let main_path = workspace_root.join(".devcontainer/devcontainer.json");
            if !main_path.exists() {
                return Ok(Vec::new());
            }
            main_path
        }
    };

    let value = config::read_jsonc(&config_path)?;
    Ok(parse_forward_ports_from_value(&value))
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
    fn parse_out_of_range_number_port_skipped() {
        let val = json!({"forwardPorts": [3000, 70000, 8080]});
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000, 8080]);
    }

    #[test]
    fn parse_out_of_range_object_port_skipped() {
        let val = json!({"forwardPorts": [{"port": 3000}, {"port": 100000}]});
        assert_eq!(parse_forward_ports_from_value(&val), vec![3000]);
    }
}
