#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;

/// Check whether a PID belongs to a dcw process.
///
/// On Linux, inspects `/proc/<pid>/cmdline`.
/// On macOS, uses `ps -p <pid> -o comm=`.
#[cfg(target_os = "linux")]
pub fn is_dcw_process(pid: i32) -> bool {
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).contains("dcw"),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
pub fn is_dcw_process(pid: i32) -> bool {
    match Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).contains("dcw")
        }
        _ => false,
    }
}

/// Send SIGTERM to a process only if it is a dcw process.
/// Returns `true` if the signal was sent, `false` if skipped or failed.
pub fn kill_dcw_process(pid: i32) -> bool {
    if !is_dcw_process(pid) {
        return false;
    }
    unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
}
