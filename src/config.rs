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
    pub runtime: RuntimeConfig,
    pub scope: ScopeConfig,
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub scratchpads: BTreeMap<String, ScratchpadConfig>,
    pub notes: NotesConfig,
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
            runtime: RuntimeConfig::default(),
            scope: ScopeConfig::default(),
            profiles,
            scratchpads,
            notes: NotesConfig::default(),
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
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = toml::to_string_pretty(self)?;
        std::fs::write(path, encoded)?;
        Ok(())
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

    fn validate(&self) -> anyhow::Result<()> {
        self.ui.popup.width.validate("ui.popup.width")?;
        self.ui.popup.height.validate("ui.popup.height")?;
        if self.runtime.backing_session.trim().is_empty() {
            anyhow::bail!("runtime.backing_session must not be empty");
        }
        Ok(())
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotesConfig {
    /// Explicit vault path. Wins over auto-detection when set.
    #[serde(default)]
    pub vault_path: Option<String>,
    /// Parse Obsidian's `obsidian.json` registry when no explicit vault is set.
    #[serde(default = "default_true")]
    pub vault_auto: bool,
    /// Override path to `obsidian.json` (portable installs, Flatpak/Snap, tests).
    #[serde(default)]
    pub obsidian_config: Option<String>,
    /// Force daily-note subdir inside the vault. Empty = read from
    /// `.obsidian/daily-notes.json`, then vault root.
    #[serde(default)]
    pub daily_subdir: Option<String>,
    /// Force Moment.js daily-note format. Empty = read from vault config.
    #[serde(default)]
    pub daily_format: Option<String>,
    /// Force template file path. Empty = read from vault config.
    #[serde(default)]
    pub template_path: Option<String>,
    /// Directory for local daily files when no vault resolves.
    /// Empty = `<state_dir>/daily`.
    #[serde(default)]
    pub fallback_dir: Option<String>,
    /// Editor binary. Empty = `$VISUAL`/`$EDITOR`, then `vim`.
    #[serde(default)]
    pub editor: Option<String>,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            vault_path: None,
            vault_auto: true,
            obsidian_config: None,
            daily_subdir: None,
            daily_format: None,
            template_path: None,
            fallback_dir: None,
            editor: None,
        }
    }
}

impl NotesConfig {
    pub fn vault_path_set(&self) -> Option<&str> {
        self.vault_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
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
    /// Sync a popup scratchpad's working directory to the pane it was opened
    /// from (the `change_path` floax behavior).
    pub change_path: bool,
    /// Popup size delta applied by `resize up` and `resize down`.
    pub resize_step: PopupDimension,
    /// Popup size used by `fullscreen`.
    pub fullscreen_size: PopupDimension,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            toggle_returns_to_previous: true,
            reuse_existing: true,
            restore_last_cwd: true,
            close_confirmation: true,
            placement: ScratchpadPlacement::Popup,
            split_direction: SplitDirection::Right,
            change_path: true,
            resize_step: PopupDimension::Percent("5%".to_string()),
            fullscreen_size: PopupDimension::Percent("100%".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScratchpadPlacement {
    #[default]
    Popup,
    Split,
    Tab,
}

impl ScratchpadPlacement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Popup => "popup",
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
    pub popup: PopupConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            title_template: "Scratchpad:{name}".to_string(),
            status_notifications: NotificationMode::Errors,
            popup: PopupConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupConfig {
    pub width: PopupDimension,
    pub height: PopupDimension,
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            width: PopupDimension::Percent("80%".to_string()),
            height: PopupDimension::Percent("80%".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PopupDimension {
    Cells(u16),
    Percent(String),
}

#[allow(dead_code)] // sizing is deprecated; helpers kept for config round-trips
impl PopupDimension {
    pub fn as_arg(&self) -> String {
        match self {
            Self::Cells(value) => value.to_string(),
            Self::Percent(value) => value.clone(),
        }
    }

    /// Parse a stored dimension string (`"NN"` cells or `"NN%"` percentage).
    pub fn parse_str(raw: &str) -> Option<Self> {
        if let Some(percent) = raw.strip_suffix('%') {
            let value = percent.parse::<u16>().ok()?;
            if (1..=100).contains(&value) {
                return Some(Self::Percent(format!("{value}%")));
            }
            None
        } else {
            raw.parse::<u16>()
                .ok()
                .filter(|value| *value > 0)
                .map(Self::Cells)
        }
    }

    /// Move a popup dimension toward fullscreen (`up`) or smaller (`down`).
    ///
    /// Mixed units follow the dimension's own unit: a percentage step applies to
    /// a percentage dimension, and an absolute-cell step applies to a cell
    /// dimension. Results clamp to a sane outer popup range.
    /// Apply a sizing step (legacy; sizing is deprecated for the overlay viewer).
    #[allow(dead_code)] // kept for config round-trip tests
    pub fn apply_step(&self, step: &PopupDimension, up: bool) -> Self {
        match self {
            Self::Percent(_) => {
                let current = percent_points(self).unwrap_or(80).clamp(10, 100);
                let delta = percent_points(step).unwrap_or(5).abs().max(1);
                let next = if up { current + delta } else { current - delta };
                Self::Percent(format!("{}%", next.clamp(10, 100)))
            }
            Self::Cells(_) => {
                let current = match self {
                    Self::Cells(value) => i32::from(*value),
                    Self::Percent(_) => unreachable!("matched branch"),
                };
                let delta = match step {
                    Self::Cells(value) => i32::from(*value),
                    Self::Percent(_) => percent_points(step).unwrap_or(5).abs().max(1),
                };
                let next = if up { current + delta } else { current - delta };
                Self::Cells(next.clamp(1, i32::from(u16::MAX)) as u16)
            }
        }
    }

    fn validate(&self, field: &str) -> anyhow::Result<()> {
        match self {
            Self::Cells(0) => anyhow::bail!("{field} must be greater than zero"),
            Self::Cells(_) => Ok(()),
            Self::Percent(value) => {
                let valid = value
                    .strip_suffix('%')
                    .and_then(|raw| raw.parse::<u16>().ok())
                    .is_some_and(|value| (1..=100).contains(&value));
                if !valid {
                    anyhow::bail!(
                        "{field} must be a positive cell count or percentage from 1% to 100%"
                    );
                }
                Ok(())
            }
        }
    }
}

#[allow(dead_code)] // sizing is deprecated; kept for config round-trip tests
fn percent_points(value: &PopupDimension) -> Option<i32> {
    match value {
        PopupDimension::Percent(raw) => raw.strip_suffix('%')?.parse().ok(),
        PopupDimension::Cells(cells) => Some(i32::from(*cells)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub backing_session: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            backing_session: "herdr-scratch".to_string(),
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
        assert_eq!(config.behavior.placement, ScratchpadPlacement::Popup);
        assert_eq!(config.behavior.split_direction, SplitDirection::Right);
        assert_eq!(config.ui.popup.width.as_arg(), "80%");
        assert_eq!(config.runtime.backing_session, "herdr-scratch");
        assert!(config.profiles.contains_key("default"));
        assert_eq!(config.behavior.resize_step.as_arg(), "5%");
        assert_eq!(config.behavior.fullscreen_size.as_arg(), "100%");
        assert!(config.behavior.change_path);
        assert_eq!(config.ui.title_template, "Scratchpad:{name}");
        assert!(config.notes.vault_path.is_none());
        assert!(config.notes.vault_auto);
        assert!(config.notes.editor.is_none());
    }

    #[test]
    fn parses_notes_config_keys() {
        let config: Config = toml::from_str(
            r#"
version = 1

[notes]
vault_path = "/vault"
vault_auto = false
daily_subdir = "Daily"
daily_format = "YYYY/MM/DD"
template_path = "/vault/Tpl.md"
fallback_dir = "/tmp/fallback"
editor = "nvim"
        "#,
        )
        .unwrap();
        assert_eq!(config.notes.vault_path.as_deref(), Some("/vault"));
        assert!(!config.notes.vault_auto);
        assert_eq!(config.notes.daily_subdir.as_deref(), Some("Daily"));
        assert_eq!(config.notes.daily_format.as_deref(), Some("YYYY/MM/DD"));
        assert_eq!(config.notes.template_path.as_deref(), Some("/vault/Tpl.md"));
        assert_eq!(config.notes.fallback_dir.as_deref(), Some("/tmp/fallback"));
        assert_eq!(config.notes.editor.as_deref(), Some("nvim"));
    }

    #[test]
    fn parses_resize_config_keys() {
        let config: Config = toml::from_str(
            r#"
version = 1

[behavior]
resize_step = "4%"
fullscreen_size = "95%"
change_path = false
        "#,
        )
        .unwrap();
        assert_eq!(config.behavior.resize_step.as_arg(), "4%");
        assert_eq!(config.behavior.fullscreen_size.as_arg(), "95%");
        assert!(!config.behavior.change_path);
    }

    #[test]
    fn popup_dimension_parse_str_round_trips() {
        assert_eq!(PopupDimension::parse_str("85%").unwrap().as_arg(), "85%");
        assert_eq!(PopupDimension::parse_str("42").unwrap().as_arg(), "42");
        assert_eq!(PopupDimension::parse_str("0%"), None);
        assert_eq!(PopupDimension::parse_str("101%"), None);
        assert_eq!(PopupDimension::parse_str("0"), None);
        assert_eq!(PopupDimension::parse_str("nope"), None);
    }

    #[test]
    fn resize_step_moves_percent_dimensions_and_clamps() {
        let base = PopupDimension::Percent("80%".to_string());
        let step = PopupDimension::Percent("5%".to_string());
        assert_eq!(base.apply_step(&step, true).as_arg(), "85%");
        assert_eq!(base.apply_step(&step, false).as_arg(), "75%");
        let at_floor = PopupDimension::Percent("12%".to_string());
        assert_eq!(at_floor.apply_step(&step, false).as_arg(), "10%");
        let near_ceiling = PopupDimension::Percent("97%".to_string());
        assert_eq!(near_ceiling.apply_step(&step, true).as_arg(), "100%");
    }

    #[test]
    fn resize_step_moves_cell_dimensions() {
        let base = PopupDimension::Cells(80);
        let step = PopupDimension::Cells(5);
        assert_eq!(base.apply_step(&step, true).as_arg(), "85");
        assert_eq!(base.apply_step(&step, false).as_arg(), "75");
        assert_eq!(
            PopupDimension::Cells(3).apply_step(&step, false).as_arg(),
            "1"
        );
    }

    #[test]
    fn mixed_resize_step_follows_dimension_unit() {
        let percent = PopupDimension::Percent("50%".to_string());
        let cells_step = PopupDimension::Cells(7);
        assert_eq!(percent.apply_step(&cells_step, true).as_arg(), "57%");
        let cells = PopupDimension::Cells(50);
        let percent_step = PopupDimension::Percent("7%".to_string());
        assert_eq!(cells.apply_step(&percent_step, true).as_arg(), "57");
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
    fn parses_popup_dimensions_as_percentages_or_cells() {
        let config: Config = toml::from_str(
            r#"
[ui.popup]
width = "90%"
height = 42
        "#,
        )
        .unwrap();
        assert_eq!(config.ui.popup.width.as_arg(), "90%");
        assert_eq!(config.ui.popup.height.as_arg(), "42");
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

    #[test]
    fn config_save_round_trips_path_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.profiles.insert(
            "logs".to_string(),
            ProfileConfig {
                command: vec!["tail".into(), "-f".into(), "app.log".into()],
                cwd: CwdMode::Path("/tmp/project".into()),
                env: HashMap::new(),
            },
        );

        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        let profile = loaded.profile("logs");
        assert_eq!(profile.command, vec!["tail", "-f", "app.log"]);
        assert!(matches!(profile.cwd, CwdMode::Path(path) if path == "/tmp/project"));
    }
}
