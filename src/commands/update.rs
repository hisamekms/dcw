use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use anyhow::{bail, Context, Result};

const REPO: &str = "hisamekms/dcw";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(clap::Args)]
pub struct UpdateArgs {
    /// Install a specific version (e.g. v0.2.0)
    #[arg(long)]
    pub version: Option<String>,

    /// Update even if already on the latest version
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: &UpdateArgs) -> Result<()> {
    let current = CURRENT_VERSION.trim_start_matches('v');

    let tag = match &args.version {
        Some(v) => {
            let v = v.strip_prefix('v').unwrap_or(v);
            format!("v{v}")
        }
        None => fetch_latest_tag()?,
    };

    let latest = tag.trim_start_matches('v');

    if latest == current && !args.force {
        println!("Already up to date (v{current}).");
        return Ok(());
    }

    if latest == current {
        println!("Reinstalling v{current}...");
    } else {
        println!("Updating v{current} → {tag}...");
    }

    let target = detect_target()?;
    let asset = format!("dcw-{tag}-{target}.tar.gz");
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");

    let tmpdir = tempdir()?;
    let tarball = format!("{tmpdir}/{asset}");

    download(&url, &tarball)?;

    let status = Command::new("tar")
        .args(["xzf", &tarball, "-C", &tmpdir])
        .status()
        .context("failed to extract tarball")?;
    if !status.success() {
        bail!("tar extraction failed");
    }

    let new_binary = format!("{tmpdir}/dcw");
    let current_exe =
        env::current_exe().context("failed to determine current executable path")?;

    fs::copy(&new_binary, &current_exe)
        .context("failed to replace binary — try with appropriate permissions")?;
    fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755))?;

    let _ = fs::remove_dir_all(&tmpdir);

    println!("Updated to {tag}.");
    Ok(())
}

fn fetch_latest_tag() -> Result<String> {
    let output = Command::new("curl")
        .args([
            "-fsSL",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .context("failed to run curl — is it installed?")?;

    if !output.status.success() {
        bail!(
            "failed to fetch latest release: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let body = String::from_utf8_lossy(&output.stdout);
    let tag = body
        .lines()
        .find(|l| l.contains("\"tag_name\""))
        .and_then(|l| {
            let after_key = l.find("tag_name")? + "tag_name".len();
            let rest = &l[after_key..];
            // Pattern: `"tag_name": "v0.1.0"` — skip to the value's opening quote
            let q1 = rest.find('"')? + 1;
            let inner = &rest[q1..];
            let q2 = inner.find('"')?;
            Some(inner[..q2].to_string())
        })
        .context("could not parse tag_name from GitHub API response")?;

    Ok(tag)
}

fn detect_target() -> Result<String> {
    let arch = cmd_output("uname", &["-m"])?;
    let target = match arch.as_str() {
        "x86_64" => "x86_64-unknown-linux-gnu",
        "aarch64" => "aarch64-unknown-linux-gnu",
        other => bail!("unsupported architecture: {other}"),
    };
    Ok(target.to_string())
}

fn download(url: &str, dest: &str) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", url, "-o", dest])
        .status()
        .context("failed to run curl")?;

    if !status.success() {
        bail!("download failed: {url}");
    }
    Ok(())
}

fn tempdir() -> Result<String> {
    let output = Command::new("mktemp")
        .args(["-d"])
        .output()
        .context("failed to create temp directory")?;

    if !output.status.success() {
        bail!("mktemp failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn cmd_output(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {cmd}"))?;

    if !output.status.success() {
        bail!("{cmd} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
