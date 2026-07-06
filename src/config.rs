use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "config.toml";
const REGISTRY_FILE: &str = "registry.json";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub config_file: PathBuf,
    pub registry_file: PathBuf,
}

impl Paths {
    pub fn discover() -> anyhow::Result<Self> {
        let config_dir = env_path("HERDR_PLUGIN_CONFIG_DIR").unwrap_or_else(default_config_dir);
        let state_dir = env_path("HERDR_PLUGIN_STATE_DIR").unwrap_or_else(default_state_dir);
        Ok(Self {
            config_file: config_dir.join(CONFIG_FILE),
            registry_file: state_dir.join(REGISTRY_FILE),
            config_dir,
            state_dir,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub default_scratchpad: String,
    pub behavior: BehaviorConfig,
    pub ui: UiConfig,
    pub scope: ScopeConfig,
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub scratchpads: BTreeMap<String, ScratchpadConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert("default".to_string(), ProfileConfig::default());

        let mut scratchpads = BTreeMap::new();
        scratchpads.insert("scratch".to_string(), ScratchpadConfig::default());

        Self {
            version: 1,
            default_scratchpad: "scratch".to_string(),
            behavior: BehaviorConfig::default(),
            ui: UiConfig::default(),
            scope: ScopeConfig::default(),
            profiles,
            scratchpads,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn scratchpad_name<'a>(&'a self, requested: Option<&'a str>) -> &'a str {
        requested.unwrap_or(&self.default_scratchpad)
    }

    pub fn scratchpad(&self, name: &str) -> ScratchpadConfig {
        self.scratchpads.get(name).cloned().unwrap_or_default()
    }

    pub fn profile(&self, name: &str) -> ProfileConfig {
        self.profiles.get(name).cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    pub toggle_returns_to_previous: bool,
    pub reuse_existing: bool,
    pub restore_last_cwd: bool,
    pub close_confirmation: bool,
    pub placement: ScratchpadPlacement,
    pub split_direction: SplitDirection,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            toggle_returns_to_previous: true,
            reuse_existing: true,
            restore_last_cwd: true,
            close_confirmation: true,
            placement: ScratchpadPlacement::Split,
            split_direction: SplitDirection::Right,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScratchpadPlacement {
    #[default]
    Split,
    Tab,
}

impl ScratchpadPlacement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Tab => "tab",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    #[default]
    Right,
    Down,
}

impl SplitDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub title_template: String,
    pub status_notifications: NotificationMode,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            title_template: "scratch:{name}".to_string(),
            status_notifications: NotificationMode::Errors,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationMode {
    Off,
    #[default]
    Errors,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeConfig {
    pub default: ScopeKind,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            default: ScopeKind::Workspace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Global,
    #[default]
    Workspace,
    Cwd,
}

impl std::fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeKind::Global => f.write_str("global"),
            ScopeKind::Workspace => f.write_str("workspace"),
            ScopeKind::Cwd => f.write_str("cwd"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileConfig {
    pub command: Vec<String>,
    pub cwd: CwdMode,
    pub env: HashMap<String, String>,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            command: Vec::new(),
            cwd: CwdMode::Context,
            env: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CwdMode {
    #[default]
    Context,
    Workspace,
    Home,
    Path(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScratchpadConfig {
    pub profile: String,
    pub scope: Option<ScopeKind>,
}

impl Default for ScratchpadConfig {
    fn default() -> Self {
        Self {
            profile: "default".to_string(),
            scope: None,
        }
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn default_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("herdr-scratch")
}

fn default_state_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("herdr-scratch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_public_defaults() {
        let config = Config::default();
        assert_eq!(config.default_scratchpad, "scratch");
        assert_eq!(config.scope.default, ScopeKind::Workspace);
        assert_eq!(config.behavior.placement, ScratchpadPlacement::Split);
        assert_eq!(config.behavior.split_direction, SplitDirection::Right);
        assert!(config.profiles.contains_key("default"));
    }

    #[test]
    fn parses_minimal_config() {
        let config: Config = toml::from_str(
            r#"
version = 1
default_scratchpad = "notes"

[scope]
default = "cwd"
        "#,
        )
        .unwrap();
        assert_eq!(config.default_scratchpad, "notes");
        assert_eq!(config.scope.default, ScopeKind::Cwd);
    }

    #[test]
    fn parses_scratchpad_surface_behavior() {
        let config: Config = toml::from_str(
            r#"
version = 1

[behavior]
placement = "tab"
split_direction = "down"
        "#,
        )
        .unwrap();
        assert_eq!(config.behavior.placement, ScratchpadPlacement::Tab);
        assert_eq!(config.behavior.split_direction, SplitDirection::Down);
    }

    #[test]
    fn parses_documented_profile_shape() {
        let config: Config = toml::from_str(
            r#"
version = 1
default_scratchpad = "scratch"

[profiles.default]
command = []
cwd = "context"
env = {}

[scratchpads.scratch]
profile = "default"
scope = "workspace"
        "#,
        )
        .unwrap();
        assert!(matches!(config.profile("default").cwd, CwdMode::Context));
    }
}
