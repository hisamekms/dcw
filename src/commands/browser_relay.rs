use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;

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
    // Health check endpoint (no auth required)
    if request.url() == "/health" && request.method() == &tiny_http::Method::Get {
        let _ = request.respond(tiny_http::Response::from_string("OK").with_status_code(200));
        return;
    }

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

    // Check method
    if request.method() != &tiny_http::Method::Post {
        let _ = request.respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
        return;
    }

    // Read body
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        let _ = request.respond(tiny_http::Response::from_string("Bad Request").with_status_code(400));
        return;
    }

    match request.url() {
        "/open" => handle_open(request, &body),
        "/cmux" => handle_cmux(request, &body),
        _ => {
            let _ = request.respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
        }
    }
}

fn handle_open(request: tiny_http::Request, body: &str) {
    // Parse JSON
    let url = match serde_json::from_str::<serde_json::Value>(body) {
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

fn handle_cmux(request: tiny_http::Request, body: &str) {
    // Parse JSON: { "args": [...], "env": { "KEY": "val", ... } }
    let val: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            let _ = request.respond(
                tiny_http::Response::from_string("Invalid JSON").with_status_code(400),
            );
            return;
        }
    };

    let args: Vec<String> = match val.get("args").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        None => {
            let _ = request.respond(
                tiny_http::Response::from_string("Missing 'args' array").with_status_code(400),
            );
            return;
        }
    };

    let env: std::collections::HashMap<String, String> =
        match val.get("env").and_then(|v| v.as_object()) {
            Some(obj) => obj
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect(),
            None => std::collections::HashMap::new(),
        };

    // Execute cmux on the host.
    // Remove all inherited CMUX_* env vars so stale values from the relay's
    // launch context don't leak into the child process.
    let inherited_cmux_keys: Vec<String> = std::env::vars()
        .filter(|(k, _)| k.starts_with("CMUX_"))
        .map(|(k, _)| k)
        .collect();

    let mut cmd = Command::new("cmux");
    cmd.args(&args);
    for key in &inherited_cmux_keys {
        cmd.env_remove(key);
    }
    cmd.envs(&env);
    cmd.stdin(Stdio::null());

    let result = cmd.output();

    let (stdout_b64, stderr_b64, exit_code) = match result {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1);
            eprintln!("cmux {:?} -> exit {exit_code}", args);
            (
                base64_encode(&output.stdout),
                base64_encode(&output.stderr),
                exit_code,
            )
        }
        Err(e) => {
            eprintln!("Failed to execute cmux: {e}");
            let msg = format!("failed to execute cmux: {e}");
            ("".to_string(), base64_encode(msg.as_bytes()), -1)
        }
    };

    let resp = serde_json::json!({
        "stdout_b64": stdout_b64,
        "stderr_b64": stderr_b64,
        "exit_code": exit_code,
    });
    let resp_str = resp.to_string();
    let response = tiny_http::Response::from_string(&resp_str)
        .with_header(
            "Content-Type: application/json"
                .parse::<tiny_http::Header>()
                .unwrap(),
        )
        .with_status_code(200);
    let _ = request.respond(response);
}

/// Start the relay server in a background thread within the current process.
/// Binds to an OS-assigned random port so multiple `dcw exec` sessions can
/// each run their own independent relay. Returns the token, the port, and a
/// guard that stops the server when dropped.
///
/// Running the relay in-process keeps cmux child processes in the caller's
/// process tree, which is required by cmux's process-origin authentication.
pub fn start_relay_thread() -> Result<(String, u16, RelayGuard)> {
    let token = generate_token()?;

    // Bind to port 0 to let the OS pick an available port.
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|e| anyhow::anyhow!("failed to bind relay: {e}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .context("relay server has no IP address")?
        .port();
    let server = Arc::new(server);

    let token_clone = token.clone();
    let server_clone = Arc::clone(&server);
    std::thread::spawn(move || {
        for request in server_clone.incoming_requests() {
            handle_request(request, &token_clone);
        }
    });

    Ok((token, port, RelayGuard { server }))
}

/// Guard that stops the in-process relay server when dropped.
pub struct RelayGuard {
    server: Arc<tiny_http::Server>,
}

impl Drop for RelayGuard {
    fn drop(&mut self) {
        self.server.unblock();
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
    let log_path = dir.join("browser-relay.log");
    let log_file = fs::File::create(&log_path)
        .context("failed to create relay log file")?;
    let child = Command::new(&exe)
        .args(["browser-relay", "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
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

/// Encode bytes as base64 (standard alphabet with padding).
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        result.push(if chunk.len() > 1 { CHARS[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
        result.push(if chunk.len() > 2 { CHARS[(triple & 0x3F) as usize] as char } else { '=' });
    }
    result
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

    #[test]
    fn base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_encode_hello() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
    }

    #[test]
    fn base64_encode_multiline() {
        assert_eq!(base64_encode(b"line1\nline2\n"), "bGluZTEKbGluZTIK");
    }

    #[test]
    fn base64_encode_padding() {
        // 1 byte -> 4 chars with ==
        assert_eq!(base64_encode(b"A"), "QQ==");
        // 2 bytes -> 4 chars with =
        assert_eq!(base64_encode(b"AB"), "QUI=");
        // 3 bytes -> 4 chars no padding
        assert_eq!(base64_encode(b"ABC"), "QUJD");
    }
}
