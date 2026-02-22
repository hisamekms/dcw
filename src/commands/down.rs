use anyhow::{Context, Result};
use std::fs;

use crate::docker;
use crate::workspace;

pub fn run() -> Result<()> {
    let workspace_folder = workspace::workspace_folder()?;
    let ws_id = workspace::workspace_id()?;

    let container_id = docker::find_devcontainer(&workspace_folder)?
        .context("no running devcontainer found")?;

    // Stop port watcher if running
    stop_watcher();

    // Remove port-forwarding sidecars
    println!("Removing port forwards...");
    docker::remove_all_port_forwards(&ws_id)?;

    // Stop the container
    println!("Stopping container {container_id}...");
    let output = std::process::Command::new("docker")
        .args(["stop", &container_id])
        .status()
        .context("failed to run docker stop")?;

    if !output.success() {
        anyhow::bail!("docker stop exited with status {output}");
    }

    println!("Devcontainer stopped.");
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
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        let _ = fs::remove_file(&pid_file);
    }
}
