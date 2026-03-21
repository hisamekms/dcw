use anyhow::{Context, Result};
use std::process::Command;

/// Open a URL in the host's default browser.
/// Uses `open` on macOS and `xdg-open` on Linux.
/// The URL is passed as an argument (not via shell) to prevent injection.
pub fn open_url(url: &str) -> Result<()> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    Command::new(cmd)
        .arg(url)
        .spawn()
        .with_context(|| format!("failed to open URL with {cmd}"))?;

    Ok(())
}
