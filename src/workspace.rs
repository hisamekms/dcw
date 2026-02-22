use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;

/// Returns a workspace identifier derived from the current directory basename.
/// Format: `dev-<basename>`
pub fn workspace_id() -> Result<String> {
    let folder = workspace_folder()?;
    let basename = PathBuf::from(&folder)
        .file_name()
        .context("workspace folder has no basename")?
        .to_string_lossy()
        .to_string();
    Ok(format!("dev-{basename}"))
}

/// Returns the absolute path of the current working directory.
pub fn workspace_folder() -> Result<String> {
    let cwd = env::current_dir().context("failed to get current directory")?;
    Ok(cwd.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_contains_basename() {
        let id = workspace_id().unwrap();
        assert!(id.starts_with("dev-"), "expected dev- prefix, got: {id}");
        assert!(id.len() > 4, "expected non-empty basename");
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
