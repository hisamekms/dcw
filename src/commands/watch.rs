use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::docker;
use crate::workspace;

pub struct WatchConfig {
    pub interval: u64,
    pub min_port: u16,
    pub exclude_ports: HashSet<u16>,
}

/// Parse `/proc/net/tcp` (or `/proc/net/tcp6`) content and return
/// the set of ports in LISTEN state (state == 0A).
///
/// Format (each line after header):
///   sl  local_address rem_address   st ...
/// where local_address is `ADDR:PORT` (hex).
pub fn parse_proc_net_tcp(content: &str) -> HashSet<u16> {
    let mut ports = HashSet::new();
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        // fields[3] is the state
        if fields[3] != "0A" {
            continue;
        }
        // fields[1] is local_address in format ADDR:PORT (hex)
        if let Some(port_hex) = fields[1].split(':').last() {
            if let Ok(port) = u16::from_str_radix(port_hex, 16) {
                ports.insert(port);
            }
        }
    }
    ports
}

/// Detect listening ports inside a container by reading /proc/net/tcp{,6}.
fn detect_listening_ports(container_id: &str) -> Result<HashSet<u16>> {
    let tcp = docker::exec_in_container(container_id, &["cat", "/proc/net/tcp"])
        .context("failed to read /proc/net/tcp")?;
    let mut ports = parse_proc_net_tcp(&tcp);

    // tcp6 may not exist; ignore errors
    if let Ok(tcp6) = docker::exec_in_container(container_id, &["cat", "/proc/net/tcp6"]) {
        ports.extend(parse_proc_net_tcp(&tcp6));
    }

    Ok(ports)
}

pub fn run_watch(config: &WatchConfig) -> Result<()> {
    let ws_id = workspace::workspace_id()?;
    let workspace_folder = workspace::workspace_folder()?;

    let container_id = docker::find_devcontainer(&workspace_folder)?
        .context("no running devcontainer found")?;
    let network = docker::get_container_network(&container_id)?;

    println!(
        "Watching for listening ports (interval: {}s)...",
        config.interval
    );
    println!("Press Ctrl+C to stop and clean up.");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    let mut managed: HashSet<u16> = HashSet::new();
    let interval = Duration::from_secs(config.interval);

    while running.load(Ordering::SeqCst) {
        // Check container is still running
        if !docker::is_container_running(&container_id)? {
            println!("Container stopped, exiting watch.");
            break;
        }

        let listening = match detect_listening_ports(&container_id) {
            Ok(ports) => ports,
            Err(e) => {
                eprintln!("Warning: failed to detect ports: {e}");
                thread::sleep(interval);
                continue;
            }
        };

        // Apply filters
        let eligible: HashSet<u16> = listening
            .into_iter()
            .filter(|p| *p >= config.min_port && !config.exclude_ports.contains(p))
            .collect();

        // New ports to forward
        let new_ports: Vec<u16> = eligible.difference(&managed).copied().collect();
        for port in new_ports {
            println!("Detected port {port}, creating forward...");
            match docker::start_port_forward(
                &ws_id,
                &container_id,
                port,
                port,
                &network,
                true,
                Some("watch"),
            ) {
                Ok(()) => {
                    println!("  Forwarded 127.0.0.1:{port} -> {port}");
                    managed.insert(port);
                }
                Err(e) => {
                    eprintln!("  Warning: failed to forward port {port}: {e}");
                }
            }
        }

        // Ports that disappeared
        let disappeared: Vec<u16> = managed.difference(&eligible).copied().collect();
        for port in disappeared {
            println!("Port {port} no longer listening, removing forward...");
            if let Err(e) = docker::remove_port_forward(&ws_id, port) {
                eprintln!("  Warning: failed to remove forward for port {port}: {e}");
            }
            managed.remove(&port);
        }

        thread::sleep(interval);
    }

    println!("Cleaning up watcher-managed port forwards...");
    docker::remove_port_forwards_by_source(&ws_id, "watch")?;
    println!("Done.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tcp_listen_ports() {
        let content = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12346 1 0000000000000000 100 0 0 10 0
   2: 0100007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12347 1 0000000000000000 100 0 0 10 0
   3: 0100007F:C350 0100007F:0BB8 01 00000000:00000000 00:00000000 00000000     0        0 12348 1 0000000000000000 100 0 0 10 0";

        let ports = parse_proc_net_tcp(content);
        // 0x0BB8 = 3000, 0x1F90 = 8080, 0x0035 = 53
        // Line 3 is state 01 (ESTABLISHED), should be excluded
        assert!(ports.contains(&3000));
        assert!(ports.contains(&8080));
        assert!(ports.contains(&53));
        assert!(!ports.contains(&50000)); // 0xC350 = 50000 but state is 01
        assert_eq!(ports.len(), 3);
    }

    #[test]
    fn parse_tcp_empty() {
        let content = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";
        let ports = parse_proc_net_tcp(content);
        assert!(ports.is_empty());
    }

    #[test]
    fn parse_tcp6_listen_ports() {
        let content = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0";

        let ports = parse_proc_net_tcp(content);
        assert!(ports.contains(&8080));
        assert_eq!(ports.len(), 1);
    }
}
