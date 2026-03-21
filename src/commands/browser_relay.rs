use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};

use crate::process;
use crate::workspace;

const RELAY_PORT: u16 = 19280;

#[derive(clap::Subcommand)]
pub enum BrowserRelayAction {
    /// Run the browser relay server (internal, not for direct use)
    Serve,
}

pub fn run(action: &BrowserRelayAction) -> Result<()> {
    match action {
        BrowserRelayAction::Serve => run_serve(),
    }
}

/// Run the HTTP relay server. Blocks until the process is killed.
fn run_serve() -> Result<()> {
    let token = fs::read_to_string(workspace::relay_token_file())
        .context("failed to read relay token file")?
        .trim()
        .to_string();

    if token.is_empty() {
        bail!("relay token file is empty");
    }

    let addr = format!("127.0.0.1:{RELAY_PORT}");
    let server = tiny_http::Server::http(&addr)
        .map_err(|e| anyhow::anyhow!("failed to bind {addr}: {e}"))?;

    eprintln!("Browser relay listening on {addr}");

    for request in server.incoming_requests() {
        handle_request(request, &token);
    }

    Ok(())
}

fn handle_request(mut request: tiny_http::Request, expected_token: &str) {
    // Check authorization
    let expected_value = format!("Bearer {expected_token}");
    let auth_ok = request
        .headers()
        .iter()
        .any(|h| {
            h.field.as_str().as_str().eq_ignore_ascii_case("authorization")
                && h.value.as_str() == expected_value
        });

    if !auth_ok {
        let _ = request.respond(tiny_http::Response::from_string("Unauthorized").with_status_code(401));
        return;
    }

    // Check method and path
    if request.method() != &tiny_http::Method::Post || request.url() != "/open" {
        let _ = request.respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
        return;
    }

    // Read body
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        let _ = request.respond(tiny_http::Response::from_string("Bad Request").with_status_code(400));
        return;
    }

    // Parse JSON
    let url = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(val) => match val.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                let _ = request.respond(
                    tiny_http::Response::from_string("Missing 'url' field").with_status_code(400),
                );
                return;
            }
        },
        Err(_) => {
            let _ = request.respond(
                tiny_http::Response::from_string("Invalid JSON").with_status_code(400),
            );
            return;
        }
    };

    // Validate URL scheme
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let _ = request.respond(
            tiny_http::Response::from_string("Only http:// and https:// URLs are allowed")
                .with_status_code(400),
        );
        return;
    }

    // Open browser
    match crate::browser::open_url(&url) {
        Ok(_) => {
            eprintln!("Opened: {url}");
            let _ = request.respond(tiny_http::Response::from_string("OK").with_status_code(200));
        }
        Err(e) => {
            eprintln!("Failed to open URL: {e}");
            let _ = request.respond(
                tiny_http::Response::from_string("Failed to open browser").with_status_code(500),
            );
        }
    }
}

/// Ensure the browser relay is running. If already running, returns the existing token.
/// Otherwise, generates a new token, spawns the relay process, and returns the token.
pub fn ensure_relay_running() -> Result<String> {
    let pid_file = workspace::relay_pid_file();
    let token_file = workspace::relay_token_file();

    // Check if relay is already running
    if let Ok(contents) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = contents.trim().parse::<i32>() {
            if process::is_dcw_process(pid) {
                // Already running, return existing token
                let token = fs::read_to_string(&token_file)
                    .context("relay PID alive but token file missing")?;
                return Ok(token.trim().to_string());
            }
        }
        // Stale PID file, clean up
        let _ = fs::remove_file(&pid_file);
    }

    // Generate new token
    let token = generate_token()?;

    // Ensure directory exists
    let dir = workspace::shared_runtime_dir();
    fs::create_dir_all(&dir).context("failed to create shared runtime directory")?;

    // Write token before spawning so the server can read it
    fs::write(&token_file, &token).context("failed to write relay token file")?;

    // Spawn relay server
    let exe = std::env::current_exe().context("failed to get current executable path")?;
    let child = Command::new(&exe)
        .args(["browser-relay", "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn browser relay")?;

    let pid = child.id();
    fs::write(&pid_file, pid.to_string()).context("failed to write relay PID file")?;

    // Brief pause to check if the process crashed immediately (e.g. port conflict)
    std::thread::sleep(std::time::Duration::from_millis(100));
    if !process::is_dcw_process(pid as i32) {
        let _ = fs::remove_file(&pid_file);
        let _ = fs::remove_file(&token_file);
        bail!("browser relay exited immediately — port {RELAY_PORT} may be in use");
    }

    println!("Browser relay started (pid {pid}, port {RELAY_PORT}).");
    Ok(token)
}

/// Stop the browser relay process if it is running.
pub fn stop_relay() {
    let pid_file = workspace::relay_pid_file();
    let token_file = workspace::relay_token_file();

    if let Ok(contents) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = contents.trim().parse::<i32>() {
            println!("Stopping browser relay (pid {pid})...");
            if !process::kill_dcw_process(pid) {
                println!("  PID {pid} is stale or not a dcw process, skipping kill.");
            }
        }
        let _ = fs::remove_file(&pid_file);
    }
    let _ = fs::remove_file(&token_file);
}

/// Returns true if any devcontainers (from any workspace) are currently running.
pub fn any_devcontainers_running() -> Result<bool> {
    let output = Command::new(crate::docker::docker_path())
        .args(["ps", "-q", "--filter", "label=devcontainer.local_folder"])
        .output()
        .context("failed to query running devcontainers")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(!stdout.trim().is_empty())
}

/// Generate a random hex token using /dev/urandom.
fn generate_token() -> Result<String> {
    let mut buf = [0u8; 16];
    let mut f = fs::File::open("/dev/urandom").context("failed to open /dev/urandom")?;
    f.read_exact(&mut buf).context("failed to read from /dev/urandom")?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_produces_32_char_hex() {
        let token = generate_token().unwrap();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_is_random() {
        let t1 = generate_token().unwrap();
        let t2 = generate_token().unwrap();
        assert_ne!(t1, t2);
    }
}
