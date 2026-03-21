use std::sync::OnceLock;

use serde::Deserialize;

static SETTINGS: OnceLock<Settings> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub docker: DockerSettings,
    pub relay: RelaySettings,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct DockerSettings {
    pub path: String,
    pub compose_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RelaySettings {
    pub browser: RelayFeature,
    pub cmux: RelayFeature,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RelayFeature {
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            docker: DockerSettings::default(),
            relay: RelaySettings::default(),
        }
    }
}

impl Default for DockerSettings {
    fn default() -> Self {
        Self {
            path: "docker".to_string(),
            compose_path: "docker-compose".to_string(),
        }
    }
}

impl Default for RelaySettings {
    fn default() -> Self {
        Self {
            browser: RelayFeature { enabled: true },
            cmux: RelayFeature { enabled: true },
        }
    }
}

impl Default for RelayFeature {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Settings {
    /// Get the global settings instance. Loads from config file on first access.
    pub fn get() -> &'static Settings {
        SETTINGS.get_or_init(|| Self::load())
    }

    /// Load settings from config.toml with environment variable overrides.
    fn load() -> Settings {
        let mut settings = Self::load_from_file();
        Self::apply_env_overrides(&mut settings);
        settings
    }

    /// Load settings from the config file, falling back to defaults if not found.
    fn load_from_file() -> Settings {
        let Some(config_dir) = dirs::config_dir() else {
            return Settings::default();
        };
        let config_path = config_dir.join("dcw").join("config.toml");

        let Ok(contents) = std::fs::read_to_string(&config_path) else {
            return Settings::default();
        };

        match toml::from_str(&contents) {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!("Warning: failed to parse {}: {e}", config_path.display());
                Settings::default()
            }
        }
    }

    /// Apply environment variable overrides (highest priority).
    fn apply_env_overrides(settings: &mut Settings) {
        if let Ok(val) = std::env::var("DCW_DOCKER_PATH") {
            settings.docker.path = val;
        }
        if let Ok(val) = std::env::var("DCW_DOCKER_COMPOSE_PATH") {
            settings.docker.compose_path = val;
        }
    }

    /// Parse settings from a TOML string. For testing.
    #[cfg(test)]
    fn from_toml(toml_str: &str) -> Result<Settings, toml::de::Error> {
        toml::from_str(toml_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_docker_path() {
        let s = Settings::default();
        assert_eq!(s.docker.path, "docker");
        assert_eq!(s.docker.compose_path, "docker-compose");
    }

    #[test]
    fn default_relay_enabled() {
        let s = Settings::default();
        assert!(s.relay.browser.enabled);
        assert!(s.relay.cmux.enabled);
    }

    #[test]
    fn parse_empty_toml() {
        let s = Settings::from_toml("").unwrap();
        assert_eq!(s.docker.path, "docker");
        assert_eq!(s.docker.compose_path, "docker-compose");
        assert!(s.relay.browser.enabled);
        assert!(s.relay.cmux.enabled);
    }

    #[test]
    fn parse_docker_settings() {
        let toml = r#"
[docker]
path = "/usr/local/bin/podman"
compose_path = "podman-compose"
"#;
        let s = Settings::from_toml(toml).unwrap();
        assert_eq!(s.docker.path, "/usr/local/bin/podman");
        assert_eq!(s.docker.compose_path, "podman-compose");
    }

    #[test]
    fn parse_relay_settings() {
        let toml = r#"
[relay.browser]
enabled = false

[relay.cmux]
enabled = false
"#;
        let s = Settings::from_toml(toml).unwrap();
        assert!(!s.relay.browser.enabled);
        assert!(!s.relay.cmux.enabled);
    }

    #[test]
    fn parse_partial_settings() {
        let toml = r#"
[docker]
path = "podman"
"#;
        let s = Settings::from_toml(toml).unwrap();
        assert_eq!(s.docker.path, "podman");
        // compose_path should fall back to default
        assert_eq!(s.docker.compose_path, "docker-compose");
        // relay should fall back to defaults
        assert!(s.relay.browser.enabled);
    }

    #[test]
    fn env_override_docker_path() {
        let mut s = Settings::default();
        std::env::set_var("DCW_DOCKER_PATH", "/custom/docker");
        Settings::apply_env_overrides(&mut s);
        assert_eq!(s.docker.path, "/custom/docker");
        std::env::remove_var("DCW_DOCKER_PATH");
    }

    #[test]
    fn env_override_compose_path() {
        let mut s = Settings::default();
        std::env::set_var("DCW_DOCKER_COMPOSE_PATH", "/custom/compose");
        Settings::apply_env_overrides(&mut s);
        assert_eq!(s.docker.compose_path, "/custom/compose");
        std::env::remove_var("DCW_DOCKER_COMPOSE_PATH");
    }
}
