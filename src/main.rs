mod commands;
mod docker;
mod forward_ports;
mod workspace;

use anyhow::Result;
use clap::Parser;

use commands::{exec, port, stop, up};

#[derive(Parser)]
#[command(name = "dcw", about = "Devcontainer CLI helper")]
enum Cli {
    /// Start the devcontainer
    Up(up::UpArgs),
    /// Stop the devcontainer
    Stop,
    /// Execute a command inside the devcontainer
    Exec(exec::ExecArgs),
    /// Manage port forwards
    Port {
        #[command(subcommand)]
        action: port::PortAction,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli {
        Cli::Up(args) => up::run(args),
        Cli::Stop => stop::run(),
        Cli::Exec(args) => exec::run(args),
        Cli::Port { action } => port::run(action),
    }
}
