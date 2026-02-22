use anyhow::{Context, Result};
use std::fs;

use crate::docker;
use crate::workspace;

pub fn run() -> Result<()> {
    let workspace_folder = workspace::workspace_folder()?;
    let ws_id = workspace::workspace_id()?;

    // Always stop the watcher regardless of container state
    stop_watcher();

    // Always remove port-forwarding sidecars
    println!("Removing port forwards...");
    docker::remove_all_port_forwards(&ws_id)?;

    // Stop the container if it is still running
    match docker::find_devcontainer(&workspace_folder)? {
        Some(container_id) => {
            println!("Stopping container {container_id}...");
            let output = std::process::Command::new("docker")
                .args(["stop", &container_id])
                .status()
                .context("failed to run docker stop")?;
            if !output.success() {
                anyhow::bail!("docker stop exited with status {output}");
            }
            println!("Devcontainer stopped.");
        }
        None => {
            println!("No running devcontainer found (already stopped).");
        }
    }
    Ok(())
}

fn stop_watcher() {
    let pid_file = match workspace::watcher_pid_file() {
        Ok(p) => p,
        Err(_) => return,
    };
    if let Ok(contents) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = contents.trim().parse::<i32>() {
            println!("Stopping port watcher (pid {pid})...");
            if !crate::process::kill_dcw_process(pid) {
                println!("  PID {pid} is stale or not a dcw process, skipping kill.");
            }
        }
        let _ = fs::remove_file(&pid_file);
    }
}
