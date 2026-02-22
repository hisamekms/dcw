use std::fs;

/// Check whether a PID belongs to a dcw process by inspecting `/proc/<pid>/cmdline`.
pub fn is_dcw_process(pid: i32) -> bool {
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).contains("dcw"),
        Err(_) => false,
    }
}

/// Send SIGTERM to a process only if it is a dcw process.
/// Returns `true` if the signal was sent, `false` if skipped.
pub fn kill_dcw_process(pid: i32) -> bool {
    if !is_dcw_process(pid) {
        return false;
    }
    unsafe { libc::kill(pid, libc::SIGTERM) };
    true
}
