use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::{
    cli,
    config::{Config, CwdMode, Paths, ProfileConfig, ScopeKind, ScratchpadConfig},
    herdr::{Herdr, OpenScratchpadRequest},
    output::Output,
    registry::{
        FocusSnapshot, LifecycleStatus, Registry, RegistryStore, RuntimeHandle, ScopeRecord,
        ScratchpadRecord, now_rfc3339, registry_key,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadSummary {
    pub name: String,
    pub scope: String,
    pub status: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub herdr_available: bool,
    pub config_dir: String,
    pub config_path: String,
    pub state_dir: String,
    pub state_path: String,
    pub scratchpad_count: usize,
    pub issues: Vec<String>,
}

pub struct ScratchApp<H> {
    config: Config,
    registry: Registry,
    paths: Paths,
    herdr: H,
    store: RegistryStore,
}

impl<H: Herdr> ScratchApp<H> {
    pub fn new(config: Config, registry: Registry, paths: Paths, herdr: H) -> Self {
        let store = RegistryStore::new(paths.registry_file.clone());
        Self {
            config,
            registry,
            paths,
            herdr,
            store,
        }
    }

    pub fn handle(&mut self, command: cli::Command) -> anyhow::Result<Output> {
        match command {
            cli::Command::Toggle(args) => {
                self.toggle(args.name.as_deref(), command_override(args.command))
            }
            cli::Command::Open(args) => {
                self.open(args.name.as_deref(), command_override(args.command))
            }
            cli::Command::Focus(args) => self.focus(args.name.as_deref()),
            cli::Command::Hide(args) => self.hide(args.name.as_deref()),
            cli::Command::Close(args) => self.close(args.name.as_deref()),
            cli::Command::List(args) => Ok(Output::Scratchpads {
                scratchpads: self.summaries(),
                json: args.json,
            }),
            cli::Command::Status(args) => self.status(args.name.as_deref(), args.json),
            cli::Command::Rename(args) => self.rename(&args.old, &args.new),
            cli::Command::Send(args) => self.send(&args.name, &args.text.join(" ")),
            cli::Command::Run(args) => self.run_in_scratchpad(&args.name, &args.command.join(" ")),
            cli::Command::Doctor(args) => Ok(Output::Doctor {
                report: self.doctor(),
                json: args.json,
            }),
            cli::Command::Config(args) => self.config_command(args),
            cli::Command::State(cli::PathArgs {
                command: cli::PathSubcommand::Path,
            }) => Ok(Output::Text(self.paths.registry_file.display().to_string())),
            cli::Command::Session => self.session(),
        }
    }

    fn toggle(
        &mut self,
        name: Option<&str>,
        command: Option<Vec<String>>,
    ) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        if self
            .registry
            .scratchpads
            .get(&target.key)
            .and_then(|record| record.handle.as_ref())
            .and_then(|handle| handle.pane_id.as_ref())
            .zip(current.as_ref().map(|pane| &pane.pane_id))
            .is_some_and(|(scratch_pane, current_pane)| scratch_pane == current_pane)
            && self.config.behavior.toggle_returns_to_previous
        {
            return self.hide_by_target(target);
        }
        self.activate_or_open(target, current, command)
    }

    fn open(&mut self, name: Option<&str>, command: Option<Vec<String>>) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        self.activate_or_open(target, current, command)
    }

    fn focus(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        let Some(record) = self.registry.scratchpads.get(&target.key).cloned() else {
            anyhow::bail!("scratchpad `{}` does not exist", target.name);
        };
        let Some(handle) = record.handle.as_ref() else {
            anyhow::bail!("scratchpad `{}` has no live runtime", target.name);
        };
        self.ensure_live(handle)?;
        self.herdr.focus_handle(handle)?;
        self.update_visible(&target.key, current.map(FocusSnapshot::from));
        self.save()?;
        Ok(Output::Text(format!(
            "focused scratchpad `{}`",
            target.name
        )))
    }

    fn hide(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        self.hide_by_target(target)
    }

    fn close(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        let Some(mut record) = self.registry.scratchpads.get(&target.key).cloned() else {
            return Ok(Output::Text(format!(
                "scratchpad `{}` is not open",
                target.name
            )));
        };
        if let Some(handle) = record.handle.as_ref() {
            // Closing a stale handle is allowed to become a registry cleanup.
            let _ = self.herdr.close_handle(handle);
        }
        record.status = LifecycleStatus::Closed;
        record.handle = None;
        record.last_hidden_at = Some(now_rfc3339());
        self.registry.insert(target.key, record);
        self.save()?;
        Ok(Output::Text(format!("closed scratchpad `{}`", target.name)))
    }

    fn status(&mut self, name: Option<&str>, json: bool) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        let Some(record) = self.registry.scratchpads.get(&target.key) else {
            let summary = ScratchpadSummary {
                name: target.name,
                scope: target.scope.to_string(),
                status: LifecycleStatus::Closed.to_string(),
                cwd: None,
            };
            return if json {
                Ok(Output::Json(serde_json::to_value(summary)?))
            } else {
                Ok(Output::Scratchpads {
                    scratchpads: vec![summary],
                    json,
                })
            };
        };
        let summary = summary_for(record);
        if json {
            Ok(Output::Json(serde_json::to_value(summary)?))
        } else {
            Ok(Output::Scratchpads {
                scratchpads: vec![summary],
                json,
            })
        }
    }

    fn rename(&mut self, old: &str, new: &str) -> anyhow::Result<Output> {
        let new = normalize_name(new)?;
        let keys = self.registry.keys_for_name(old);
        if keys.is_empty() {
            anyhow::bail!("scratchpad `{old}` does not exist");
        }
        for key in keys {
            let mut record = self.registry.remove(&key).expect("key came from registry");
            record.name = new.clone();
            if let Some(handle) = record.handle.as_ref() {
                self.rename_handle_best_effort(handle, &self.title_for(&new));
            }
            let new_key = registry_key(&record.scope, &new);
            self.registry.insert(new_key, record);
        }
        self.save()?;
        Ok(Output::Text(format!(
            "renamed scratchpad `{old}` to `{new}`"
        )))
    }

    fn send(&mut self, name: &str, text: &str) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(Some(name), current.as_ref())?;
        let handle = self.live_handle(&target)?;
        self.herdr.send_text(&handle, text)?;
        Ok(Output::Text(format!(
            "sent text to scratchpad `{}`",
            target.name
        )))
    }

    fn run_in_scratchpad(&mut self, name: &str, command: &str) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(Some(name), current.as_ref())?;
        let handle = self.live_handle(&target)?;
        self.herdr.run_command(&handle, command)?;
        Ok(Output::Text(format!(
            "ran command in scratchpad `{}`",
            target.name
        )))
    }

    fn activate_or_open(
        &mut self,
        target: Target,
        previous: Option<crate::herdr::PaneInfo>,
        command: Option<Vec<String>>,
    ) -> anyhow::Result<Output> {
        let existing = self.registry.scratchpads.get(&target.key).cloned();
        if self.config.behavior.reuse_existing {
            if let Some(record) = existing.as_ref() {
                if let Some(handle) = record.handle.as_ref() {
                    if self.ensure_live(handle).is_ok() {
                        self.rename_handle_best_effort(handle, &self.title_for(&target.name));
                        self.herdr.focus_handle(handle)?;
                        self.update_visible(&target.key, previous.map(FocusSnapshot::from));
                        self.save()?;
                        return Ok(Output::Text(format!("opened scratchpad `{}`", target.name)));
                    }
                }
            }
        }

        let record = self.create_record(
            target,
            previous.map(FocusSnapshot::from),
            existing,
            command.as_deref(),
        )?;
        let name = record.name.clone();
        self.registry
            .insert(registry_key(&record.scope, &record.name), record);
        self.save()?;
        Ok(Output::Text(format!("opened scratchpad `{name}`")))
    }

    fn create_record(
        &self,
        target: Target,
        previous_focus: Option<FocusSnapshot>,
        previous_record: Option<ScratchpadRecord>,
        command_override: Option<&[String]>,
    ) -> anyhow::Result<ScratchpadRecord> {
        let scratch_config = self.config.scratchpad(&target.name);
        let profile = self.config.profile(&scratch_config.profile);
        let cwd = resolve_cwd(&profile, &target.context);
        let mut env = BTreeMap::new();
        env.insert("HERDR_SCRATCH_NAME".to_string(), target.name.clone());
        env.insert(
            "HERDR_SCRATCH_PROFILE".to_string(),
            scratch_config.profile.clone(),
        );
        let launch_command = launch_command(&profile, command_override);
        if !launch_command.is_empty() {
            env.insert(
                "HERDR_SCRATCH_COMMAND_JSON".to_string(),
                serde_json::to_string(&launch_command)?,
            );
        }
        for (key, value) in profile.env {
            env.insert(key, value);
        }
        let handle = self.herdr.open_scratchpad(OpenScratchpadRequest {
            cwd: cwd.clone(),
            env,
            placement: self.config.behavior.placement,
            split_direction: self.config.behavior.split_direction,
        })?;
        self.rename_handle_best_effort(&handle, &self.title_for(&target.name));
        let now = now_rfc3339();
        Ok(ScratchpadRecord {
            name: target.name,
            scope: target.scope,
            profile: scratch_config.profile,
            status: LifecycleStatus::Visible,
            handle: Some(handle),
            cwd,
            created_at: previous_record
                .as_ref()
                .map(|record| record.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            last_shown_at: now,
            last_hidden_at: previous_record.and_then(|record| record.last_hidden_at),
            previous_focus,
        })
    }

    fn hide_by_target(&mut self, target: Target) -> anyhow::Result<Output> {
        let Some(mut record) = self.registry.scratchpads.get(&target.key).cloned() else {
            return Ok(Output::Text(format!(
                "scratchpad `{}` is not open",
                target.name
            )));
        };
        if let Some(previous) = record.previous_focus.as_ref() {
            let _ = self.herdr.focus_previous(previous);
        }
        record.status = LifecycleStatus::Available;
        record.last_hidden_at = Some(now_rfc3339());
        self.registry.insert(target.key, record);
        self.save()?;
        Ok(Output::Text(format!("left scratchpad `{}`", target.name)))
    }

    fn live_handle(&self, target: &Target) -> anyhow::Result<RuntimeHandle> {
        let record = self
            .registry
            .scratchpads
            .get(&target.key)
            .ok_or_else(|| anyhow::anyhow!("scratchpad `{}` does not exist", target.name))?;
        let handle = record
            .handle
            .clone()
            .ok_or_else(|| anyhow::anyhow!("scratchpad `{}` has no live runtime", target.name))?;
        self.ensure_live(&handle)?;
        Ok(handle)
    }

    fn ensure_live(&self, handle: &RuntimeHandle) -> anyhow::Result<()> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            anyhow::bail!("scratchpad runtime is missing a pane handle");
        };
        self.herdr.pane_get(pane_id)?;
        if let Some(focus_token) = handle.focus_token() {
            self.herdr.tab_get(focus_token)?;
        }
        Ok(())
    }

    fn update_visible(&mut self, key: &str, previous: Option<FocusSnapshot>) {
        if let Some(record) = self.registry.scratchpads.get_mut(key) {
            record.status = LifecycleStatus::Visible;
            record.last_shown_at = now_rfc3339();
            record.previous_focus = previous;
        }
    }

    fn title_for(&self, name: &str) -> String {
        let title = self.config.ui.title_template.replace("{name}", name);
        let title = title.trim();
        if title.is_empty() {
            name.to_string()
        } else {
            title.to_string()
        }
    }

    fn rename_handle_best_effort(&self, handle: &RuntimeHandle, title: &str) {
        let _ = self.herdr.rename_handle(handle, title);
    }

    fn summaries(&self) -> Vec<ScratchpadSummary> {
        self.registry
            .scratchpads
            .values()
            .map(summary_for)
            .collect()
    }

    fn doctor(&self) -> DoctorReport {
        let mut issues = Vec::new();
        let herdr_available = self.herdr.available();
        if !herdr_available {
            issues
                .push("Herdr CLI is not available; set HERDR_BIN_PATH or add herdr to PATH".into());
        }
        if self.config.version != 1 {
            issues.push(format!(
                "unsupported config version {}; expected 1",
                self.config.version
            ));
        }
        DoctorReport {
            herdr_available,
            config_dir: self.paths.config_dir.display().to_string(),
            config_path: self.paths.config_file.display().to_string(),
            state_dir: self.paths.state_dir.display().to_string(),
            state_path: self.paths.registry_file.display().to_string(),
            scratchpad_count: self.registry.scratchpads.len(),
            issues,
        }
    }

    fn session(&self) -> anyhow::Result<Output> {
        run_session_process()?;
        Ok(Output::None)
    }

    fn save(&self) -> anyhow::Result<()> {
        self.store.save(&self.registry)
    }

    fn config_command(&mut self, args: cli::ConfigArgs) -> anyhow::Result<Output> {
        match args.command {
            cli::ConfigSubcommand::Path => {
                Ok(Output::Text(self.paths.config_file.display().to_string()))
            }
            cli::ConfigSubcommand::Init(args) => self.config_init(args.force),
            cli::ConfigSubcommand::Add(args) => self.config_add(args),
        }
    }

    fn config_init(&mut self, force: bool) -> anyhow::Result<Output> {
        if self.paths.config_file.exists() && !force {
            anyhow::bail!(
                "config already exists at {}; pass --force to overwrite",
                self.paths.config_file.display()
            );
        }
        self.config = Config::default();
        self.config.save(&self.paths.config_file)?;
        Ok(Output::Text(format!(
            "initialized config {}",
            self.paths.config_file.display()
        )))
    }

    fn config_add(&mut self, args: cli::ConfigAddArgs) -> anyhow::Result<Output> {
        let name = normalize_name(&args.name)?;
        if self.config.profiles.contains_key(&name) || self.config.scratchpads.contains_key(&name) {
            anyhow::bail!("scratchpad/profile `{name}` already exists");
        }
        let scope = parse_scope(args.scope.as_deref())?;
        let cwd = parse_cwd(args.cwd.as_deref());
        let command = command_override(args.command)
            .ok_or_else(|| anyhow::anyhow!("config add requires a command after --"))?;

        self.config.profiles.insert(
            name.clone(),
            ProfileConfig {
                command,
                cwd,
                env: HashMap::new(),
            },
        );
        self.config.scratchpads.insert(
            name.clone(),
            ScratchpadConfig {
                profile: name.clone(),
                scope: Some(scope),
            },
        );
        self.config.save(&self.paths.config_file)?;
        Ok(Output::Text(format!(
            "added scratchpad `{name}` to {}",
            self.paths.config_file.display()
        )))
    }

    fn target(
        &self,
        requested_name: Option<&str>,
        current: Option<&crate::herdr::PaneInfo>,
    ) -> anyhow::Result<Target> {
        let name = normalize_name(self.config.scratchpad_name(requested_name))?;
        let scratch_config = self.config.scratchpad(&name);
        let scope_kind = scratch_config.scope.unwrap_or(self.config.scope.default);
        let context = InvocationContext::load().with_current(current);
        let scope = resolve_scope(scope_kind, &context);
        let key = registry_key(&scope, &name);
        Ok(Target {
            name,
            scope,
            key,
            context,
        })
    }
}

#[derive(Debug, Clone)]
struct Target {
    name: String,
    scope: ScopeRecord,
    key: String,
    context: InvocationContext,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct InvocationContext {
    workspace_id: Option<String>,
    workspace_cwd: Option<String>,
    #[serde(rename = "tab_id")]
    _tab_id: Option<String>,
    #[serde(rename = "focused_pane_id")]
    _focused_pane_id: Option<String>,
    focused_pane_cwd: Option<String>,
}

impl InvocationContext {
    fn load() -> Self {
        std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_else(|| Self {
                workspace_id: std::env::var("HERDR_WORKSPACE_ID").ok(),
                _tab_id: std::env::var("HERDR_TAB_ID").ok(),
                _focused_pane_id: std::env::var("HERDR_PANE_ID").ok(),
                workspace_cwd: None,
                focused_pane_cwd: None,
            })
    }

    fn context_cwd(&self) -> Option<String> {
        self.focused_pane_cwd
            .clone()
            .or_else(|| self.workspace_cwd.clone())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.display().to_string())
            })
    }

    fn with_current(mut self, current: Option<&crate::herdr::PaneInfo>) -> Self {
        let Some(current) = current else {
            return self;
        };
        if self.workspace_id.is_none() {
            self.workspace_id = Some(current.workspace_id.clone());
        }
        if self._tab_id.is_none() {
            self._tab_id = Some(current.tab_id.clone());
        }
        if self._focused_pane_id.is_none() {
            self._focused_pane_id = Some(current.pane_id.clone());
        }
        if self.focused_pane_cwd.is_none() {
            self.focused_pane_cwd = current.cwd.clone();
        }
        self
    }
}

impl From<crate::herdr::PaneInfo> for FocusSnapshot {
    fn from(pane: crate::herdr::PaneInfo) -> Self {
        Self {
            pane_id: Some(pane.pane_id),
            focus_token: Some(pane.tab_id),
            workspace_id: Some(pane.workspace_id),
        }
    }
}

fn resolve_scope(kind: ScopeKind, context: &InvocationContext) -> ScopeRecord {
    match kind {
        ScopeKind::Global => ScopeRecord {
            kind: "global".to_string(),
            key: "default".to_string(),
        },
        ScopeKind::Workspace => ScopeRecord {
            kind: "workspace".to_string(),
            key: context
                .workspace_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        },
        ScopeKind::Cwd => ScopeRecord {
            kind: "cwd".to_string(),
            key: context
                .context_cwd()
                .unwrap_or_else(|| "unknown".to_string()),
        },
    }
}

fn resolve_cwd(profile: &ProfileConfig, context: &InvocationContext) -> Option<String> {
    match &profile.cwd {
        CwdMode::Context => context.context_cwd(),
        CwdMode::Workspace => context
            .workspace_cwd
            .clone()
            .or_else(|| context.context_cwd()),
        CwdMode::Home => dirs::home_dir().map(|path| path.display().to_string()),
        CwdMode::Path(path) => Some(path.clone()),
    }
}

fn command_override(command: Vec<String>) -> Option<Vec<String>> {
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

fn launch_command(profile: &ProfileConfig, command_override: Option<&[String]>) -> Vec<String> {
    command_override
        .filter(|command| !command.is_empty())
        .map(|command| command.to_vec())
        .unwrap_or_else(|| profile.command.clone())
}

fn parse_scope(raw: Option<&str>) -> anyhow::Result<ScopeKind> {
    match raw.unwrap_or("workspace") {
        "global" => Ok(ScopeKind::Global),
        "workspace" => Ok(ScopeKind::Workspace),
        "cwd" => Ok(ScopeKind::Cwd),
        other => anyhow::bail!("invalid scope `{other}`; expected global, workspace, or cwd"),
    }
}

fn parse_cwd(raw: Option<&str>) -> CwdMode {
    match raw.unwrap_or("context") {
        "context" => CwdMode::Context,
        "workspace" => CwdMode::Workspace,
        "home" => CwdMode::Home,
        path => CwdMode::Path(path.to_string()),
    }
}

fn normalize_name(raw: &str) -> anyhow::Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        anyhow::bail!("scratchpad name must not be empty");
    }
    if name.chars().any(|ch| ch.is_control()) {
        anyhow::bail!("scratchpad name must not contain control characters");
    }
    Ok(name.to_string())
}

fn summary_for(record: &ScratchpadRecord) -> ScratchpadSummary {
    ScratchpadSummary {
        name: record.name.clone(),
        scope: record.scope.to_string(),
        status: record.status.to_string(),
        cwd: record.cwd.clone(),
    }
}

fn run_session_process() -> anyhow::Result<()> {
    let command = std::env::var("HERDR_SCRATCH_COMMAND_JSON")
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .filter(|command| !command.is_empty());
    let status = if let Some(command) = command {
        let (program, args) = command.split_first().expect("filtered non-empty");
        Command::new(program).args(args).status()?
    } else {
        let shell = default_shell();
        Command::new(shell).status()?
    };
    if !status.success() {
        anyhow::bail!("scratchpad session exited with status {status}");
    }
    Ok(())
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

#[allow(dead_code)]
fn display_path(path: Option<PathBuf>) -> Option<String> {
    path.map(|path| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_validation_rejects_empty_names() {
        assert!(normalize_name(" ").is_err());
        assert_eq!(normalize_name("scratch").unwrap(), "scratch");
    }

    #[test]
    fn cwd_scope_uses_context_cwd() {
        let context = InvocationContext {
            focused_pane_cwd: Some("/repo".into()),
            ..InvocationContext::default()
        };
        let scope = resolve_scope(ScopeKind::Cwd, &context);
        assert_eq!(scope.kind, "cwd");
        assert_eq!(scope.key, "/repo");
    }

    #[test]
    fn launch_command_prefers_one_shot_override() {
        let profile = ProfileConfig {
            command: vec!["bash".into()],
            cwd: CwdMode::Context,
            env: Default::default(),
        };
        let override_command = vec!["lazygit".to_string()];
        assert_eq!(
            launch_command(&profile, Some(&override_command)),
            vec!["lazygit"]
        );
    }

    #[test]
    fn launch_command_uses_profile_without_override() {
        let profile = ProfileConfig {
            command: vec!["python".into()],
            cwd: CwdMode::Context,
            env: Default::default(),
        };
        assert_eq!(launch_command(&profile, None), vec!["python"]);
    }

    #[test]
    fn parse_config_add_defaults() {
        assert_eq!(parse_scope(None).unwrap(), ScopeKind::Workspace);
        assert!(matches!(parse_cwd(None), CwdMode::Context));
        assert!(matches!(
            parse_cwd(Some("/tmp/project")),
            CwdMode::Path(path) if path == "/tmp/project"
        ));
    }
}
