use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::workspace;

/// Read a JSONC file (JSON with comments and trailing commas) and parse it.
pub fn read_jsonc(path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = jsonc_parser::parse_to_serde_value(&content, &Default::default())
        .map_err(|e| anyhow::anyhow!("failed to parse JSONC from {}: {}", path.display(), e))?;
    parsed.context(format!("empty JSONC file: {}", path.display()))
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

/// Convert a relative path to absolute by joining it with `base`.
/// If the path is already absolute, return it unchanged.
fn make_absolute(path_str: &str, base: &Path) -> String {
    let p = Path::new(path_str);
    if p.is_absolute() {
        path_str.to_string()
    } else {
        base.join(p).to_string_lossy().to_string()
    }
}

/// Rewrite build-related relative paths in the merged config to absolute paths.
///
/// This is necessary because the merged config is written to a runtime directory
/// (`/tmp/dcw-<uid>/<ws_id>/`), and the devcontainer CLI resolves relative paths
/// from the config file location. Without this, Dockerfile-based builds would
/// fail because the CLI cannot find the Dockerfile.
fn resolve_build_paths(config: &mut Value, config_dir: &Path) {
    let config_dir_str = config_dir.to_string_lossy().to_string();

    // --- nested build.dockerfile / build.context ---
    if let Some(build) = config.get_mut("build").and_then(|v| v.as_object_mut()) {
        let has_dockerfile = build.contains_key("dockerfile");

        if let Some(df) = build.get_mut("dockerfile") {
            if let Some(s) = df.as_str().map(|s| s.to_string()) {
                *df = Value::String(make_absolute(&s, config_dir));
            }
        }

        if let Some(ctx) = build.get_mut("context") {
            if let Some(s) = ctx.as_str().map(|s| s.to_string()) {
                *ctx = Value::String(make_absolute(&s, config_dir));
            }
        } else if has_dockerfile {
            build.insert("context".to_string(), Value::String(config_dir_str.clone()));
        }
    }

    // --- top-level dockerFile / context ---
    {
        let has_docker_file = config
            .as_object()
            .is_some_and(|m| m.contains_key("dockerFile"));

        if let Some(df) = config.get_mut("dockerFile") {
            if let Some(s) = df.as_str().map(|s| s.to_string()) {
                *df = Value::String(make_absolute(&s, config_dir));
            }
        }

        if let Some(ctx) = config.get_mut("context") {
            if let Some(s) = ctx.as_str().map(|s| s.to_string()) {
                *ctx = Value::String(make_absolute(&s, config_dir));
            }
        } else if has_docker_file {
            config
                .as_object_mut()
                .unwrap()
                .insert("context".to_string(), Value::String(config_dir_str.clone()));
        }
    }

    // --- dockerComposeFile (string or array) ---
    if let Some(dcf) = config.get_mut("dockerComposeFile") {
        match dcf {
            Value::String(s) => {
                *s = make_absolute(s, config_dir);
            }
            Value::Array(arr) => {
                for item in arr.iter_mut() {
                    if let Value::String(s) = item {
                        *s = make_absolute(s, config_dir);
                    }
                }
            }
            _ => {}
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
    resolve_build_paths(&mut base, &dc_dir);

    let runtime = workspace::runtime_dir()?;
    fs::create_dir_all(&runtime).context("failed to create runtime directory")?;

    let merged_path = runtime.join("devcontainer.json");
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
    fn read_jsonc_strips_line_comments() {
        let dir = std::env::temp_dir().join("dcw-test-config-jsonc-line");
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

    #[test]
    fn read_jsonc_strips_inline_comments() {
        let dir = std::env::temp_dir().join("dcw-test-config-jsonc-inline");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.jsonc");
        fs::write(
            &path,
            r#"{
    "name": "test", // inline comment
    "forwardPorts": [3000]
}"#,
        )
        .unwrap();

        let val = read_jsonc(&path).unwrap();
        assert_eq!(val["name"], "test");
        assert_eq!(val["forwardPorts"], json!([3000]));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_jsonc_strips_block_comments() {
        let dir = std::env::temp_dir().join("dcw-test-config-jsonc-block");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.jsonc");
        fs::write(
            &path,
            r#"{
    /* block comment */
    "name": "test",
    /*
     * multi-line
     * block comment
     */
    "forwardPorts": [3000]
}"#,
        )
        .unwrap();

        let val = read_jsonc(&path).unwrap();
        assert_eq!(val["name"], "test");
        assert_eq!(val["forwardPorts"], json!([3000]));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_jsonc_allows_trailing_commas() {
        let dir = std::env::temp_dir().join("dcw-test-config-jsonc-trailing");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.jsonc");
        fs::write(
            &path,
            r#"{
    "name": "test",
    "forwardPorts": [3000, 8080,],
}"#,
        )
        .unwrap();

        let val = read_jsonc(&path).unwrap();
        assert_eq!(val["name"], "test");
        assert_eq!(val["forwardPorts"], json!([3000, 8080]));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_jsonc_preserves_urls() {
        let dir = std::env::temp_dir().join("dcw-test-config-jsonc-url");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.jsonc");
        fs::write(
            &path,
            r#"{
    "image": "https://example.com/image:latest"
}"#,
        )
        .unwrap();

        let val = read_jsonc(&path).unwrap();
        assert_eq!(val["image"], "https://example.com/image:latest");

        let _ = fs::remove_dir_all(&dir);
    }

    // ---- resolve_build_paths tests ----

    #[test]
    fn resolve_build_paths_nested_dockerfile_and_context() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "build": {
                "dockerfile": "Dockerfile",
                "context": ".."
            }
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(
            config["build"]["dockerfile"],
            "/workspace/.devcontainer/Dockerfile"
        );
        assert_eq!(config["build"]["context"], "/workspace/.devcontainer/..");
    }

    #[test]
    fn resolve_build_paths_nested_dockerfile_without_context() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "build": {
                "dockerfile": "Dockerfile"
            }
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(
            config["build"]["dockerfile"],
            "/workspace/.devcontainer/Dockerfile"
        );
        assert_eq!(
            config["build"]["context"],
            "/workspace/.devcontainer"
        );
    }

    #[test]
    fn resolve_build_paths_top_level_dockerfile_and_context() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "dockerFile": "Dockerfile.dev",
            "context": ".."
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(
            config["dockerFile"],
            "/workspace/.devcontainer/Dockerfile.dev"
        );
        assert_eq!(config["context"], "/workspace/.devcontainer/..");
    }

    #[test]
    fn resolve_build_paths_top_level_dockerfile_without_context() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "dockerFile": "Dockerfile"
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(
            config["dockerFile"],
            "/workspace/.devcontainer/Dockerfile"
        );
        assert_eq!(config["context"], "/workspace/.devcontainer");
    }

    #[test]
    fn resolve_build_paths_absolute_paths_unchanged() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "build": {
                "dockerfile": "/opt/docker/Dockerfile",
                "context": "/opt/docker"
            }
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(config["build"]["dockerfile"], "/opt/docker/Dockerfile");
        assert_eq!(config["build"]["context"], "/opt/docker");
    }

    #[test]
    fn resolve_build_paths_docker_compose_string() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "dockerComposeFile": "docker-compose.yml"
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(
            config["dockerComposeFile"],
            "/workspace/.devcontainer/docker-compose.yml"
        );
    }

    #[test]
    fn resolve_build_paths_docker_compose_array() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "dockerComposeFile": [
                "docker-compose.yml",
                "/absolute/docker-compose.override.yml",
                "../docker-compose.dev.yml"
            ]
        });
        resolve_build_paths(&mut config, base);

        let arr = config["dockerComposeFile"].as_array().unwrap();
        assert_eq!(arr[0], "/workspace/.devcontainer/docker-compose.yml");
        assert_eq!(arr[1], "/absolute/docker-compose.override.yml");
        assert_eq!(
            arr[2],
            "/workspace/.devcontainer/../docker-compose.dev.yml"
        );
    }

    #[test]
    fn resolve_build_paths_no_build_fields_is_noop() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "image": "mcr.microsoft.com/devcontainers/rust:1",
            "forwardPorts": [3000]
        });
        let original = config.clone();
        resolve_build_paths(&mut config, base);

        assert_eq!(config, original);
    }

    #[test]
    fn resolve_build_paths_build_without_dockerfile() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "build": {
                "args": {"VARIANT": "bullseye"}
            }
        });
        resolve_build_paths(&mut config, base);

        // context should NOT be added when dockerfile is absent
        assert!(config["build"].get("context").is_none());
        assert_eq!(config["build"]["args"]["VARIANT"], "bullseye");
    }

    #[test]
    fn resolve_build_paths_both_nested_and_top_level() {
        let base = Path::new("/workspace/.devcontainer");
        let mut config = json!({
            "build": {
                "dockerfile": "Dockerfile",
                "context": ".."
            },
            "dockerFile": "Dockerfile.alt",
            "context": "../other"
        });
        resolve_build_paths(&mut config, base);

        assert_eq!(
            config["build"]["dockerfile"],
            "/workspace/.devcontainer/Dockerfile"
        );
        assert_eq!(config["build"]["context"], "/workspace/.devcontainer/..");
        assert_eq!(
            config["dockerFile"],
            "/workspace/.devcontainer/Dockerfile.alt"
        );
        assert_eq!(config["context"], "/workspace/.devcontainer/../other");
    }
}
