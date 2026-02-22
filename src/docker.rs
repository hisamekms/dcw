use anyhow::{bail, Context, Result};
use std::process::Command;

/// Execute a command inside a running container and return stdout.
pub fn exec_in_container(container_id: &str, cmd: &[&str]) -> Result<String> {
    let mut args = vec!["exec", container_id];
    args.extend(cmd);

    let output = Command::new("docker")
        .args(&args)
        .output()
        .context("failed to run docker exec")?;

    if !output.status.success() {
        bail!(
            "docker exec failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if a container is still running.
pub fn is_container_running(container_id: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", container_id])
        .output()
        .context("failed to run docker inspect")?;

    Ok(output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == "true")
}

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

/// Get the IP address of a container on a given network.
/// The default `bridge` network doesn't support container name/ID DNS resolution,
/// so we need the actual IP for socat to connect to.
pub fn get_container_ip(container_id: &str, network: &str) -> Result<String> {
    let template = format!(
        "{{{{.NetworkSettings.Networks.{network}.IPAddress}}}}"
    );
    let output = Command::new("docker")
        .args(["inspect", "-f", &template, container_id])
        .output()
        .context("failed to run docker inspect for IP")?;

    if !output.status.success() {
        bail!(
            "docker inspect failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ip.is_empty() {
        bail!("container {container_id} has no IP on network {network}");
    }

    Ok(ip)
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
    source: Option<&str>,
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
        "dcw.role=port-forward".to_string(),
        "--label".to_string(),
        format!("dcw.workspace={ws_id}"),
        "--label".to_string(),
        format!("dcw.port={container_port}"),
        "--label".to_string(),
        format!("dcw.host_port={host_port}"),
    ];

    if let Some(src) = source {
        args.extend([
            "--label".to_string(),
            format!("dcw.source={src}"),
        ]);
    }

    args.extend([
        "-p".to_string(),
        format!("127.0.0.1:{host_port}:{host_port}"),
    ]);

    if detach {
        args.push("-d".to_string());
    }

    let container_ip = get_container_ip(container_id, network)?;

    args.extend([
        "alpine/socat".to_string(),
        format!("TCP-LISTEN:{host_port},fork,reuseaddr"),
        format!("TCP:{container_ip}:{container_port}"),
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
            "label=dcw.role=port-forward",
            "--filter",
            &format!("label=dcw.workspace={ws_id}"),
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

/// Remove all port-forwarding sidecars with a given source label.
pub fn remove_port_forwards_by_source(ws_id: &str, source: &str) -> Result<()> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-q",
            "--filter",
            "label=dcw.role=port-forward",
            "--filter",
            &format!("label=dcw.workspace={ws_id}"),
            "--filter",
            &format!("label=dcw.source={source}"),
        ])
        .output()
        .context("failed to list port-forward sidecars by source")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for id in stdout.trim().lines() {
        if !id.is_empty() {
            let _ = Command::new("docker").args(["rm", "-f", id]).output();
        }
    }

    Ok(())
}

/// Info about an active port forward.
pub struct PortForwardInfo {
    pub name: String,
    pub host_port: String,
    pub container_port: String,
}

/// List active port-forwarding sidecars for a workspace.
pub fn list_port_forwards(ws_id: &str) -> Result<Vec<PortForwardInfo>> {
    let output = Command::new("docker")
        .args([
            "ps",
            "--filter",
            "label=dcw.role=port-forward",
            "--filter",
            &format!("label=dcw.workspace={ws_id}"),
            "--format",
            "{{.Names}}\t{{.Label \"dcw.host_port\"}}\t{{.Label \"dcw.port\"}}",
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
            PortForwardInfo {
                name: parts.first().unwrap_or(&"").to_string(),
                host_port: parts.get(1).unwrap_or(&"").to_string(),
                container_port: parts.get(2).unwrap_or(&"").to_string(),
            }
        })
        .collect();

    Ok(forwards)
}
