use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct RegistryStore {
    path: PathBuf,
}

impl RegistryStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> anyhow::Result<Registry> {
        Registry::load(&self.path)
    }

    pub fn save(&self, registry: &Registry) -> anyhow::Result<()> {
        registry.save(&self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub version: u32,
    pub scratchpads: BTreeMap<String, ScratchpadRecord>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: 1,
            scratchpads: BTreeMap::new(),
        }
    }
}

impl Registry {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let registry = serde_json::from_str(&content)?;
        Ok(registry)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let encoded = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, encoded)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn insert(&mut self, key: String, record: ScratchpadRecord) {
        self.scratchpads.insert(key, record);
    }

    pub fn remove(&mut self, key: &str) -> Option<ScratchpadRecord> {
        self.scratchpads.remove(key)
    }

    pub fn keys_for_name(&self, name: &str) -> BTreeSet<String> {
        self.scratchpads
            .iter()
            .filter(|(_, record)| record.name == name)
            .map(|(key, _)| key.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadRecord {
    pub name: String,
    pub scope: ScopeRecord,
    pub profile: String,
    pub status: LifecycleStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<RuntimeHandle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub created_at: String,
    pub last_shown_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_hidden_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_focus: Option<FocusSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeRecord {
    pub kind: String,
    pub key: String,
}

impl std::fmt::Display for ScopeRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.kind, self.key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeHandle {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub opaque: BTreeMap<String, serde_json::Value>,
}

impl RuntimeHandle {
    pub fn focus_token(&self) -> Option<&str> {
        self.opaque
            .get("focus_token")
            .or_else(|| self.opaque.get("tab_id"))
            .and_then(serde_json::Value::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleStatus {
    Unknown,
    Available,
    Visible,
    Hidden,
    Stale,
    #[default]
    Closed,
    Error,
}

impl std::fmt::Display for LifecycleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleStatus::Unknown => f.write_str("unknown"),
            LifecycleStatus::Available => f.write_str("available"),
            LifecycleStatus::Visible => f.write_str("visible"),
            LifecycleStatus::Hidden => f.write_str("hidden"),
            LifecycleStatus::Stale => f.write_str("stale"),
            LifecycleStatus::Closed => f.write_str("closed"),
            LifecycleStatus::Error => f.write_str("error"),
        }
    }
}

pub fn registry_key(scope: &ScopeRecord, name: &str) -> String {
    format!("{}:{}:{name}", scope.kind, scope.key)
}

pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        let mut registry = Registry::default();
        let scope = ScopeRecord {
            kind: "workspace".to_string(),
            key: "w1".to_string(),
        };
        registry.insert(
            registry_key(&scope, "scratch"),
            ScratchpadRecord {
                name: "scratch".to_string(),
                scope,
                profile: "default".to_string(),
                status: LifecycleStatus::Available,
                handle: None,
                cwd: Some("/repo".to_string()),
                created_at: now_rfc3339(),
                last_shown_at: now_rfc3339(),
                last_hidden_at: None,
                previous_focus: None,
            },
        );
        registry.save(&path).unwrap();
        let loaded = Registry::load(&path).unwrap();
        assert_eq!(loaded.scratchpads.len(), 1);
    }
}
