use anyhow::{bail, Context, Result};
use std::collections::HashSet;

use crate::commands::watch;
use crate::docker;
use crate::workspace;

#[derive(clap::Subcommand)]
pub enum PortAction {
    /// Add a port forward
    Add {
        /// Host port
        host_port: u16,
        /// Container port
        container_port: u16,
        /// Run in background (detached)
        #[arg(short, long)]
        detach: bool,
    },
    /// Remove a port forward
    #[command(alias = "rm")]
    Remove {
        /// Container port to stop forwarding (omit if using --all)
        port: Option<u16>,
        /// Remove all port forwards
        #[arg(long)]
        all: bool,
    },
    /// List active port forwards
    #[command(alias = "ls")]
    List,
    /// Watch for new listening ports and auto-forward them
    Watch {
        /// Polling interval in seconds
        #[arg(short, long, default_value = "2")]
        interval: u64,
        /// Minimum port number to forward
        #[arg(long, default_value = "1024")]
        min_port: u16,
        /// Ports to exclude from auto-forwarding
        #[arg(short, long)]
        exclude: Vec<u16>,
    },
}

pub fn run(action: &PortAction) -> Result<()> {
    let ws_id = workspace::workspace_id()?;
    let workspace_folder = workspace::workspace_folder()?;

    match action {
        PortAction::Add {
            host_port,
            container_port,
            detach,
        } => {
            let container_id = docker::find_devcontainer(&workspace_folder)?
                .context("no running devcontainer found")?;
            let network = docker::get_container_network(&container_id)?;

            println!("Forwarding port {host_port} -> {container_port}...");
            docker::start_port_forward(
                &ws_id,
                &container_id,
                *host_port,
                *container_port,
                &network,
                *detach,
                None,
            )?;
            println!("Port forward active.");
        }
        PortAction::Remove { port, all } => {
            if *all {
                println!("Removing all port forwards...");
                docker::remove_all_port_forwards(&ws_id)?;
                println!("All port forwards removed.");
            } else if let Some(p) = port {
                println!("Removing port forward for {p}...");
                docker::remove_port_forward(&ws_id, *p)?;
                println!("Port forward removed.");
            } else {
                bail!("specify a port or --all");
            }
        }
        PortAction::List => {
            let forwards = docker::list_port_forwards(&ws_id)?;
            if forwards.is_empty() {
                println!("No active port forwards.");
            } else {
                println!("{:<30} {:>6}   {:>6}", "SIDECAR", "HOST", "CONTAINER");
                for fwd in &forwards {
                    println!("{:<30} {:>6}   {:>6}", fwd.name, fwd.host_port, fwd.container_port);
                }
            }
        }
        PortAction::Watch {
            interval,
            min_port,
            exclude,
        } => {
            let config = watch::WatchConfig {
                interval: *interval,
                min_port: *min_port,
                exclude_ports: exclude.iter().copied().collect::<HashSet<u16>>(),
            };
            watch::run_watch(&config)?;
        }
    }

    Ok(())
}
