use anyhow::{bail, Context, Result};
use std::process::Command;

/// Find a running devcontainer for the given workspace folder.
/// Returns the container ID if found.
pub fn find_devcontainer(workspace_folder: &str) -> Result<Option<String>> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-q",
            "--filter",
            &format!("label=devcontainer.local_folder={workspace_folder}"),
        ])
        .output()
        .context("failed to run docker ps")?;

    if !output.status.success() {
        bail!(
            "docker ps failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let id = stdout.trim();
    if id.is_empty() {
        Ok(None)
    } else {
        // Take the first container if multiple are returned
        Ok(Some(id.lines().next().unwrap().to_string()))
    }
}

/// Get the network name for a container.
pub fn get_container_network(container_id: &str) -> Result<String> {
    let output = Command::new("docker")
        .args([
            "inspect",
            "-f",
            "{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}",
            container_id,
        ])
        .output()
        .context("failed to run docker inspect")?;

    if !output.status.success() {
        bail!(
            "docker inspect failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let network = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if network.is_empty() {
        bail!("container {container_id} has no networks");
    }

    // If multiple networks, take the first one
    Ok(network.split('\n').next().unwrap().to_string())
}

/// Start a socat port-forwarding sidecar container.
///
/// Sidecar naming: `pf-<ws_id>-c<container_port>`
/// Idempotent: removes existing sidecar first.
pub fn start_port_forward(
    ws_id: &str,
    container_id: &str,
    host_port: u16,
    container_port: u16,
    network: &str,
    detach: bool,
) -> Result<()> {
    let sidecar_name = format!("pf-{ws_id}-c{container_port}");

    // Remove existing sidecar if present (ignore errors)
    let _ = Command::new("docker")
        .args(["rm", "-f", &sidecar_name])
        .output();

    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        sidecar_name.clone(),
        "--network".to_string(),
        network.to_string(),
        "--label".to_string(),
        "dc.role=port-forward".to_string(),
        "--label".to_string(),
        format!("dc.workspace={ws_id}"),
        "--label".to_string(),
        format!("dc.port={container_port}"),
        "-p".to_string(),
        format!("127.0.0.1:{host_port}:{host_port}"),
    ];

    if detach {
        args.push("-d".to_string());
    }

    args.extend([
        "alpine/socat".to_string(),
        format!("TCP-LISTEN:{host_port},fork,reuseaddr"),
        format!("TCP:{container_id}:{container_port}"),
    ]);

    let output = Command::new("docker")
        .args(&args)
        .output()
        .context("failed to run docker run for port forward")?;

    if !output.status.success() {
        bail!(
            "failed to start port forward sidecar {sidecar_name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Remove a specific port-forwarding sidecar.
pub fn remove_port_forward(ws_id: &str, port: u16) -> Result<()> {
    let sidecar_name = format!("pf-{ws_id}-c{port}");
    let output = Command::new("docker")
        .args(["rm", "-f", &sidecar_name])
        .output()
        .context("failed to run docker rm")?;

    if !output.status.success() {
        bail!(
            "failed to remove sidecar {sidecar_name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Remove all port-forwarding sidecars for a workspace.
pub fn remove_all_port_forwards(ws_id: &str) -> Result<()> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-q",
            "--filter",
            "label=dc.role=port-forward",
            "--filter",
            &format!("label=dc.workspace={ws_id}"),
        ])
        .output()
        .context("failed to list port-forward sidecars")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for id in stdout.trim().lines() {
        if !id.is_empty() {
            let _ = Command::new("docker").args(["rm", "-f", id]).output();
        }
    }

    Ok(())
}

/// List active port-forwarding sidecars for a workspace.
/// Returns a list of (sidecar_name, port) tuples.
pub fn list_port_forwards(ws_id: &str) -> Result<Vec<(String, String)>> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "label=dc.role=port-forward",
            "--filter",
            &format!("label=dc.workspace={ws_id}"),
            "--format",
            "{{.Names}}\t{{.Label \"dc.port\"}}",
        ])
        .output()
        .context("failed to list port-forward sidecars")?;

    if !output.status.success() {
        bail!(
            "docker ps failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let forwards = stdout
        .trim()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            let name = parts.first().unwrap_or(&"").to_string();
            let port = parts.get(1).unwrap_or(&"").to_string();
            (name, port)
        })
        .collect();

    Ok(forwards)
}
