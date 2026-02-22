use anyhow::{Context, Result};
use std::env;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Returns a workspace identifier derived from the current directory.
/// Format: `dev-<basename>-<hash8>` where hash is based on the full path
/// to avoid collisions between directories with the same basename.
pub fn workspace_id() -> Result<String> {
    let folder = workspace_folder()?;
    let basename = PathBuf::from(&folder)
        .file_name()
        .context("workspace folder has no basename")?
        .to_string_lossy()
        .to_string();
    let hash = path_hash(&folder);
    Ok(format!("dev-{basename}-{hash}"))
}

fn path_hash(path: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xFFFF_FFFF)
}

/// Returns the absolute path of the current working directory.
pub fn workspace_folder() -> Result<String> {
    let cwd = env::current_dir().context("failed to get current directory")?;
    Ok(cwd.to_string_lossy().to_string())
}

/// Returns the XDG runtime directory for this workspace.
/// Uses `$XDG_RUNTIME_DIR/dcw/<ws_id>/`, falling back to `/tmp/dcw-<uid>/<ws_id>/`.
pub fn runtime_dir() -> Result<PathBuf> {
    let ws_id = workspace_id()?;
    let base = match env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(format!("/tmp/dcw-{}", unsafe { libc::getuid() })),
    };
    Ok(base.join("dcw").join(ws_id))
}

/// Returns the path of the PID file for the port watcher process.
pub fn watcher_pid_file() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("watch.pid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_contains_basename_and_hash() {
        let id = workspace_id().unwrap();
        assert!(id.starts_with("dev-"), "expected dev- prefix, got: {id}");
        // Format: dev-<basename>-<8 hex chars>
        let parts: Vec<&str> = id.rsplitn(2, '-').collect();
        assert_eq!(parts[0].len(), 8, "expected 8-char hash suffix, got: {id}");
        assert!(
            parts[0].chars().all(|c| c.is_ascii_hexdigit()),
            "expected hex hash suffix, got: {id}"
        );
    }

    #[test]
    fn path_hash_is_deterministic() {
        let h1 = path_hash("/foo/bar");
        let h2 = path_hash("/foo/bar");
        assert_eq!(h1, h2);
    }

    #[test]
    fn path_hash_differs_for_different_paths() {
        let h1 = path_hash("/home/user/foo/app");
        let h2 = path_hash("/home/user/bar/app");
        assert_ne!(h1, h2);
    }

    #[test]
    fn workspace_folder_is_absolute() {
        let folder = workspace_folder().unwrap();
        assert!(
            PathBuf::from(&folder).is_absolute(),
            "expected absolute path, got: {folder}"
        );
    }
}
