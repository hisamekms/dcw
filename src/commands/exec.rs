use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::config;
use crate::workspace;

#[derive(clap::Args)]
pub struct ExecArgs {
    /// Command and arguments to run inside the devcontainer
    #[arg(trailing_var_arg = true, required = true)]
    pub cmd: Vec<String>,
}

pub fn run(args: &ExecArgs) -> Result<()> {
    let workspace_folder = workspace::workspace_folder()?;
    let workspace_root = PathBuf::from(&workspace_folder);
    let merged_config = config::resolve_config(&workspace_root)?;

    let mut cmd_args = vec![
        "exec".to_string(),
        "--workspace-folder".to_string(),
        workspace_folder,
    ];

    if let Some(config_path) = &merged_config {
        cmd_args.push("--config".to_string());
        cmd_args.push(config_path.to_string_lossy().to_string());
    }

    if std::env::var("DCW_DOCKER_PATH").is_ok() {
        cmd_args.push("--docker-path".to_string());
        cmd_args.push(crate::docker::docker_path());
    }
    if std::env::var("DCW_DOCKER_COMPOSE_PATH").is_ok() {
        cmd_args.push("--docker-compose-path".to_string());
        cmd_args.push(crate::docker::docker_compose_path());
    }

    cmd_args.extend(build_browser_wrapped_cmd(&args.cmd));

    let status = Command::new("devcontainer")
        .args(&cmd_args)
        .status()
        .context("failed to run devcontainer exec — is the devcontainer CLI installed?")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Determine the relay hostname based on the Docker runtime in use.
/// Podman uses `host.containers.internal`, Docker uses `host.docker.internal`.
fn relay_host() -> &'static str {
    if let Ok(path) = std::env::var("DCW_DOCKER_PATH") {
        if path.contains("podman") {
            return "host.containers.internal";
        }
    }
    "host.docker.internal"
}

/// Wrap the user's command to inject BROWSER and DCW_BROWSER_TOKEN env vars.
/// If the relay token file does not exist, returns the original command unchanged.
fn build_browser_wrapped_cmd(cmd: &[String]) -> Vec<String> {
    let token = match fs::read_to_string(workspace::relay_token_file()) {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => return cmd.to_vec(),
    };

    let host = relay_host();

    let wrapper_script = format!(
        concat!(
            r#"export DCW_BROWSER_TOKEN='{token}'; "#,
            r#"export DCW_RELAY_HOST='{host}'; "#,
            r#"_dcw_b=$(mktemp); "#,
            r#"printf '%s\n' '#!/bin/sh' 'curl -sf -X POST -H "Authorization: Bearer $DCW_BROWSER_TOKEN" -H "Content-Type: application/json" -d "{{\"url\":\"$1\"}}" http://$DCW_RELAY_HOST:19280/open >/dev/null 2>&1' > "$_dcw_b"; "#,
            r#"chmod +x "$_dcw_b"; "#,
            r#"export BROWSER="$_dcw_b"; "#,
            r#"exec "$@""#,
        ),
        token = token,
        host = host,
    );

    let mut wrapped = vec![
        "sh".to_string(),
        "-c".to_string(),
        wrapper_script,
        "_".to_string(),
    ];
    wrapped.extend_from_slice(cmd);
    wrapped
}
