use anyhow::{Context, Result};
use std::process::Command;

use crate::workspace;

#[derive(clap::Args)]
pub struct ExecArgs {
    /// Command and arguments to run inside the devcontainer
    #[arg(trailing_var_arg = true, required = true)]
    pub cmd: Vec<String>,
}

pub fn run(args: &ExecArgs) -> Result<()> {
    let workspace_folder = workspace::workspace_folder()?;

    let mut cmd_args = vec![
        "exec".to_string(),
        "--workspace-folder".to_string(),
        workspace_folder,
    ];
    cmd_args.extend(args.cmd.clone());

    let status = Command::new("devcontainer")
        .args(&cmd_args)
        .status()
        .context("failed to run devcontainer exec — is the devcontainer CLI installed?")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
