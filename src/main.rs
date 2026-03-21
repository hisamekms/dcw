mod browser;
mod commands;
mod config;
mod docker;
mod forward_ports;
mod process;
mod workspace;

use anyhow::Result;
use clap::Parser;

use commands::{browser_relay, down, exec, port, up, update};

#[derive(Parser)]
#[command(name = "dcw", about = "Devcontainer CLI helper", version)]
enum Cli {
    /// Start the devcontainer
    Up(up::UpArgs),
    /// Stop the devcontainer
    Down,
    /// Execute a command inside the devcontainer
    Exec(exec::ExecArgs),
    /// Manage port forwards
    Port {
        #[command(subcommand)]
        action: port::PortAction,
    },
    /// Update dcw to the latest version
    Update(update::UpdateArgs),
    /// Internal: browser relay server
    #[command(name = "browser-relay")]
    BrowserRelay {
        #[command(subcommand)]
        action: browser_relay::BrowserRelayAction,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli {
        Cli::Up(args) => up::run(args),
        Cli::Down => down::run(),
        Cli::Exec(args) => exec::run(args),
        Cli::Port { action } => port::run(action),
        Cli::Update(args) => update::run(args),
        Cli::BrowserRelay { action } => browser_relay::run(action),
    }
}
