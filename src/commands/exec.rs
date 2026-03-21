use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::commands::browser_relay;
use crate::config;
use crate::settings::{RelaySettings, Settings};
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

    let settings = Settings::get();
    if settings.docker.path != "docker" {
        cmd_args.push("--docker-path".to_string());
        cmd_args.push(settings.docker.path.clone());
    }
    if settings.docker.compose_path != "docker-compose" {
        cmd_args.push("--docker-compose-path".to_string());
        cmd_args.push(settings.docker.compose_path.clone());
    }

    // Start relay in-process so cmux child processes inherit our process tree
    // (cmux requires callers to be descendants of a cmux terminal).
    // Skip entirely if both relay features are disabled.
    let need_relay = settings.relay.browser.enabled || settings.relay.cmux.enabled;
    let relay = if need_relay {
        match browser_relay::start_relay_thread() {
            Ok((token, port, guard)) => Some((token, port, guard)),
            Err(e) => {
                eprintln!("Warning: failed to start browser relay: {e}");
                None
            }
        }
    } else {
        None
    };

    cmd_args.extend(build_relay_wrapped_cmd(
        &args.cmd,
        relay.as_ref().map(|(token, port, _)| (token.as_str(), *port)),
        &settings.relay,
    ));

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
    if Settings::get().docker.path.contains("podman") {
        return "host.containers.internal";
    }
    "host.docker.internal"
}

/// Collect CMUX_* environment variables from the host for forwarding into the container.
fn collect_cmux_env() -> Vec<(String, String)> {
    const CMUX_VARS: &[&str] = &[
        "CMUX_WORKSPACE_ID",
        "CMUX_SURFACE_ID",
        "CMUX_TAB_ID",
        "CMUX_SOCKET_PATH",
        "CMUX_SOCKET_PASSWORD",
    ];
    CMUX_VARS
        .iter()
        .filter_map(|&name| std::env::var(name).ok().map(|val| (name.to_string(), val)))
        .collect()
}

/// Wrap the user's command to inject BROWSER stub, cmux stub, and relay env vars.
/// `relay` provides the token and port from the in-process relay. If `None`, the
/// original command is returned unchanged. Stubs are conditionally included based
/// on relay feature settings.
fn build_relay_wrapped_cmd(
    cmd: &[String],
    relay: Option<(&str, u16)>,
    relay_settings: &RelaySettings,
) -> Vec<String> {
    let (token, port) = match relay {
        Some((t, p)) => (t.to_string(), p),
        None => return cmd.to_vec(),
    };

    let host = relay_host();

    let mut script = String::new();

    // Relay connection env vars (needed by either feature)
    script.push_str(&format!(
        "export DCW_BROWSER_TOKEN='{token}'; \
         export DCW_RELAY_HOST='{host}'; \
         export DCW_RELAY_PORT='{port}'; "
    ));

    // Forward CMUX_* env vars into the container (only if cmux enabled)
    if relay_settings.cmux.enabled {
        for (k, v) in &collect_cmux_env() {
            script.push_str(&format!("export {k}='{v}'; "));
        }
    }

    // BROWSER stub: creates a temp script that POSTs URLs to the relay
    if relay_settings.browser.enabled {
        script.push_str(concat!(
            r#"_dcw_b=$(mktemp); "#,
            r#"printf '%s\n' '#!/bin/sh' 'curl -sf -X POST -H "Authorization: Bearer $DCW_BROWSER_TOKEN" -H "Content-Type: application/json" -d "{\"url\":\"$1\"}" http://$DCW_RELAY_HOST:$DCW_RELAY_PORT/open >/dev/null 2>&1' > "$_dcw_b"; "#,
            r#"chmod +x "$_dcw_b"; "#,
            r#"export BROWSER="$_dcw_b"; "#,
        ));
    }

    // cmux stub: creates a temp directory with a cmux script that proxies
    // through the relay, then prepends it to PATH
    if relay_settings.cmux.enabled {
        let cmux_stub = concat!(
            r#"#!/bin/sh"#, "\n",
            r#"_json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g;s/"/\\"/g;s/\t/\\t/g' | tr '\n' ' '; }"#, "\n",
            r#"_args=''"#, "\n",
            r#"_first=1"#, "\n",
            r#"for _a in "$@"; do"#, "\n",
            r#"  _ea=$(_json_escape "$_a")"#, "\n",
            r#"  if [ "$_first" = 1 ]; then _args="\"$_ea\""; _first=0; else _args="$_args,\"$_ea\""; fi"#, "\n",
            r#"done"#, "\n",
            r#"_env=''"#, "\n",
            r#"_efirst=1"#, "\n",
            r#"for _var in CMUX_WORKSPACE_ID CMUX_SURFACE_ID CMUX_TAB_ID CMUX_SOCKET_PATH CMUX_SOCKET_PASSWORD; do"#, "\n",
            r#"  eval _val=\${$_var:-}"#, "\n",
            r#"  if [ -n "$_val" ]; then"#, "\n",
            r#"    _ev=$(_json_escape "$_val")"#, "\n",
            r#"    if [ "$_efirst" = 1 ]; then _env="\"$_var\":\"$_ev\""; _efirst=0; else _env="$_env,\"$_var\":\"$_ev\""; fi"#, "\n",
            r#"  fi"#, "\n",
            r#"done"#, "\n",
            r#"_body="{\"args\":[$_args],\"env\":{$_env}}""#, "\n",
            r#"_resp=$(curl -sf -X POST \"#, "\n",
            r#"  -H "Authorization: Bearer $DCW_BROWSER_TOKEN" \"#, "\n",
            r#"  -H "Content-Type: application/json" \"#, "\n",
            r#"  -d "$_body" \"#, "\n",
            r#"  "http://$DCW_RELAY_HOST:$DCW_RELAY_PORT/cmux" 2>/dev/null)"#, "\n",
            r#"if [ $? -ne 0 ] || [ -z "$_resp" ]; then"#, "\n",
            r#"  echo "cmux relay: connection failed" >&2; exit 1"#, "\n",
            r#"fi"#, "\n",
            r#"# Extract base64 fields and exit_code from JSON response"#, "\n",
            r#"_stdout_b64=$(printf '%s' "$_resp" | sed -n 's/.*"stdout_b64":"\([^"]*\)".*/\1/p')"#, "\n",
            r#"_stderr_b64=$(printf '%s' "$_resp" | sed -n 's/.*"stderr_b64":"\([^"]*\)".*/\1/p')"#, "\n",
            r#"_exit=$(printf '%s' "$_resp" | sed -n 's/.*"exit_code":\([0-9-]*\).*/\1/p')"#, "\n",
            r#"[ -n "$_stdout_b64" ] && printf '%s' "$_stdout_b64" | base64 -d"#, "\n",
            r#"[ -n "$_stderr_b64" ] && printf '%s' "$_stderr_b64" | base64 -d >&2"#, "\n",
            r#"exit "${_exit:-1}""#, "\n",
        );

        script.push_str(&format!(
            concat!(
                r#"_dcw_bin=$(mktemp -d); "#,
                r#"cat > "$_dcw_bin/cmux" << 'CMUX_STUB_EOF'"#, "\n",
                r#"{cmux_stub}"#,
                r#"CMUX_STUB_EOF"#, "\n",
                r#"chmod +x "$_dcw_bin/cmux"; "#,
                r#"export PATH="$_dcw_bin:$PATH"; "#,
            ),
            cmux_stub = cmux_stub,
        ));
    }

    script.push_str(r#"exec "$@""#);

    let mut wrapped = vec![
        "sh".to_string(),
        "-c".to_string(),
        script,
        "_".to_string(),
    ];
    wrapped.extend_from_slice(cmd);
    wrapped
}
