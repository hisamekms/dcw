mod commands;
mod docker;
mod forward_ports;
mod workspace;

use anyhow::Result;
use clap::Parser;

use commands::{port, up};

#[derive(Parser)]
#[command(name = "dcw", about = "Devcontainer CLI helper")]
enum Cli {
    /// Start the devcontainer
    Up(up::UpArgs),
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
        Cli::Port { action } => port::run(action),
    }
}
