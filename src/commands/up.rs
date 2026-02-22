use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::docker;
use crate::forward_ports;
use crate::workspace;

#[derive(clap::Args)]
pub struct UpArgs {
    /// Remove existing container and rebuild
    #[arg(long)]
    pub rebuild: bool,

    /// Automatically forward ports from devcontainer.json after start
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    pub auto_forward: bool,

    /// Watch for new listening ports and auto-forward them
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true")]
    pub watch: bool,

    /// Extra arguments passed to `devcontainer up`
    #[arg(last = true)]
    pub extra: Vec<String>,
}

pub fn run(args: &UpArgs) -> Result<()> {
    let workspace_folder = workspace::workspace_folder()?;

    let mut cmd_args = vec![
        "up".to_string(),
        "--workspace-folder".to_string(),
        workspace_folder.clone(),
    ];

    if args.rebuild {
        cmd_args.push("--remove-existing-container".to_string());
    }

    cmd_args.extend(args.extra.clone());

    println!("Starting devcontainer...");
    let status = Command::new("devcontainer")
        .args(&cmd_args)
        .status()
        .context("failed to run devcontainer up — is the devcontainer CLI installed?")?;

    if !status.success() {
        bail!("devcontainer up exited with status {status}");
    }

    println!("Devcontainer is running.");

    if args.auto_forward {
        auto_forward_ports(&workspace_folder)?;
    }

    if args.watch {
        spawn_watcher()?;
    }

    Ok(())
}

/// Spawn `dcw port watch` as a detached background process.
fn spawn_watcher() -> Result<()> {
    let exe = std::env::current_exe().context("failed to get current executable path")?;
    let pid_file = workspace::watcher_pid_file()?;

    // Kill any existing watcher first
    stop_watcher_if_running(&pid_file);

    if let Some(parent) = pid_file.parent() {
        fs::create_dir_all(parent).context("failed to create runtime directory")?;
    }

    let child = Command::new(exe)
        .args(["port", "watch"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn port watcher")?;

    let pid = child.id();
    fs::write(&pid_file, pid.to_string())
        .context("failed to write watcher PID file")?;

    println!("Port watcher started (pid {pid}).");
    Ok(())
}

fn stop_watcher_if_running(pid_file: &PathBuf) {
    if let Ok(contents) = fs::read_to_string(pid_file) {
        if let Ok(pid) = contents.trim().parse::<i32>() {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        let _ = fs::remove_file(pid_file);
    }
}

fn auto_forward_ports(workspace_folder: &str) -> Result<()> {
    let ws_id = workspace::workspace_id()?;
    let root = PathBuf::from(workspace_folder);
    let ports = forward_ports::load_forward_ports(&root)?;

    if ports.is_empty() {
        println!("No forwardPorts configured.");
        return Ok(());
    }

    let container_id = docker::find_devcontainer(workspace_folder)?
        .context("devcontainer not found after start")?;

    let network = docker::get_container_network(&container_id)?;

    println!("Auto-forwarding ports: {:?}", ports);
    for port in &ports {
        if let Err(e) =
            docker::start_port_forward(&ws_id, &container_id, *port, *port, &network, true, None)
        {
            eprintln!("Warning: failed to forward port {port}: {e}");
        } else {
            println!("  Forwarded port {port} -> {port}");
        }
    }

    Ok(())
}
