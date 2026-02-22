use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::workspace;

/// Read a JSONC file (JSON with `//` line comments) and parse it.
pub fn read_jsonc(path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

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

/// Recursively merge `overlay` into `base`.
///
/// - Objects: keys from overlay are merged recursively; keys only in base are preserved.
/// - Arrays and scalars: overlay replaces base.
pub fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                let entry = base_map.entry(key).or_insert(Value::Null);
                deep_merge(entry, overlay_val);
            }
        }
        (base, overlay) => {
            *base = overlay;
        }
    }
}

/// Resolve the devcontainer config for the workspace.
///
/// If `.devcontainer/devcontainer.local.json` exists, merges it on top of
/// `devcontainer.json` and writes the result to runtime_dir. Returns the
/// path to the merged config file.
///
/// If the local override does not exist, returns `None` (use default config).
pub fn resolve_config(workspace_root: &Path) -> Result<Option<PathBuf>> {
    let dc_dir = workspace_root.join(".devcontainer");
    let local_path = dc_dir.join("devcontainer.local.json");

    if !local_path.exists() {
        return Ok(None);
    }

    let main_path = dc_dir.join("devcontainer.json");
    let mut base = read_jsonc(&main_path).context("failed to read devcontainer.json")?;
    let overlay = read_jsonc(&local_path).context("failed to read devcontainer.local.json")?;

    deep_merge(&mut base, overlay);

    let runtime = workspace::runtime_dir()?;
    fs::create_dir_all(&runtime).context("failed to create runtime directory")?;

    let merged_path = runtime.join("devcontainer.merged.json");
    let json = serde_json::to_string_pretty(&base).context("failed to serialize merged config")?;
    fs::write(&merged_path, json).context("failed to write merged config")?;

    Ok(Some(merged_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deep_merge_objects_recursively() {
        let mut base = json!({
            "customizations": {
                "vscode": {
                    "settings": {"editor.fontSize": 14},
                    "extensions": ["ext1"]
                }
            },
            "name": "base"
        });
        let overlay = json!({
            "customizations": {
                "vscode": {
                    "extensions": ["ext2", "ext3"]
                }
            }
        });
        deep_merge(&mut base, overlay);

        // settings should be preserved (only in base)
        assert_eq!(base["customizations"]["vscode"]["settings"]["editor.fontSize"], 14);
        // extensions should be replaced (array replaces, not merges)
        assert_eq!(base["customizations"]["vscode"]["extensions"], json!(["ext2", "ext3"]));
        // name should be preserved
        assert_eq!(base["name"], "base");
    }

    #[test]
    fn deep_merge_overlay_adds_new_keys() {
        let mut base = json!({"a": 1});
        let overlay = json!({"b": 2});
        deep_merge(&mut base, overlay);

        assert_eq!(base, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn deep_merge_scalar_replaces() {
        let mut base = json!({"a": 1});
        let overlay = json!({"a": 99});
        deep_merge(&mut base, overlay);

        assert_eq!(base["a"], 99);
    }

    #[test]
    fn deep_merge_array_replaces() {
        let mut base = json!({"ports": [3000, 8080]});
        let overlay = json!({"ports": [9090]});
        deep_merge(&mut base, overlay);

        assert_eq!(base["ports"], json!([9090]));
    }

    #[test]
    fn deep_merge_nested_new_key() {
        let mut base = json!({"a": {"b": 1}});
        let overlay = json!({"a": {"c": 2}});
        deep_merge(&mut base, overlay);

        assert_eq!(base, json!({"a": {"b": 1, "c": 2}}));
    }

    #[test]
    fn deep_merge_overlay_replaces_non_object_with_object() {
        let mut base = json!({"a": 1});
        let overlay = json!({"a": {"nested": true}});
        deep_merge(&mut base, overlay);

        assert_eq!(base["a"], json!({"nested": true}));
    }

    #[test]
    fn deep_merge_overlay_replaces_object_with_scalar() {
        let mut base = json!({"a": {"nested": true}});
        let overlay = json!({"a": "flat"});
        deep_merge(&mut base, overlay);

        assert_eq!(base["a"], "flat");
    }

    #[test]
    fn read_jsonc_strips_comments() {
        let dir = std::env::temp_dir().join("dcw-test-config-jsonc");
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
        assert_eq!(val["forwardPorts"], json!([3000]));

        let _ = fs::remove_dir_all(&dir);
    }
}
