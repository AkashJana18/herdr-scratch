use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::{
    cli,
    config::{
        Config, CwdMode, Paths, PopupDimension, PopupSize, ProfileConfig, ScopeKind,
        ScratchpadConfig,
    },
    herdr::{Herdr, HerdrCli, OpenScratchpadRequest},
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
    pub herdr_version: Option<String>,
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
            cli::Command::Guide => self.guide(),
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
                scratchpads: self.summaries()?,
                json: args.json,
            }),
            cli::Command::Status(args) => self.status(args.name.as_deref(), args.json),
            cli::Command::Rename(args) => self.rename(&args.old, &args.new),
            cli::Command::Send(args) => self.send(&args.name, &args.text.join(" ")),
            cli::Command::Run(args) => self.run_in_scratchpad(&args.name, &args.command.join(" ")),
            cli::Command::Resize(args) => self.resize(args.direction, args.name.as_deref()),
            cli::Command::Fullscreen(args) => self.fullscreen(args.name.as_deref()),
            cli::Command::Reset(args) => self.reset(args.name.as_deref()),
            cli::Command::Doctor(args) => Ok(Output::Doctor {
                report: self.doctor(),
                json: args.json,
            }),
            cli::Command::Config(args) => self.config_command(args),
            cli::Command::State(cli::PathArgs {
                command: cli::PathSubcommand::Path,
            }) => Ok(Output::Text(self.paths.registry_file.display().to_string())),
            cli::Command::Session => anyhow::bail!("session must be handled before app startup"),
            cli::Command::Attach => anyhow::bail!("attach must be handled before app startup"),
            cli::Command::GuidePane => {
                anyhow::bail!("guide-pane must be handled before app startup")
            }
        }
    }

    fn guide(&self) -> anyhow::Result<Output> {
        self.herdr.open_guide().map_err(|err| {
            anyhow::anyhow!(
                "failed to open the Scratch guide popup: {err}. If another popup is open, detach it with ctrl+b q and try again"
            )
        })?;
        Ok(Output::Text("opened Scratch quick start".to_string()))
    }

    fn toggle(
        &mut self,
        name: Option<&str>,
        command: Option<Vec<String>>,
    ) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        if self.config.behavior.toggle_returns_to_previous
            && self.store.viewer_is_active(&target.key)
        {
            return self.hide_by_target(target);
        }
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
        if !self.store.viewer_is_active(&target.key) {
            self.herdr
                .show_handle(handle, &target.key, &self.config.ui.popup, None)?;
            self.ensure_path_sync(&target, host_cwd(current.as_ref(), &target).as_deref());
        }
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
            if self.store.viewer_is_active(&target.key) {
                let _ = self.herdr.hide_handle(handle);
            }
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
        self.refresh_status(&target.key)?;
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
        let summary = self.summary_for_key(&target.key, record);
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
            if self.store.viewer_is_active(&key) {
                anyhow::bail!("hide scratchpad `{old}` before renaming it");
            }
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

    fn resize(
        &mut self,
        direction: cli::ResizeDirection,
        name: Option<&str>,
    ) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        let mut record = self.live_record(&target)?;
        let current_size = self.popup_size(&record)?;
        let step = &self.config.behavior.resize_step;
        let up = direction == cli::ResizeDirection::Up;
        let size = PopupSize {
            width: current_size.width.apply_step(step, up),
            height: current_size.height.apply_step(step, up),
        };
        self.apply_popup_size(&target, &mut record, size)?;
        Ok(Output::Text(format!(
            "resized scratchpad `{}`",
            target.name
        )))
    }

    fn fullscreen(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        let mut record = self.live_record(&target)?;
        let current_size = self.popup_size(&record)?;
        let fullsize = self.config.behavior.fullscreen_size.clone();
        let full_size = PopupSize {
            width: fullsize.clone(),
            height: fullsize,
        };
        let size = if current_size == full_size {
            self.stored_previous_size(&record)
                .unwrap_or_else(|| PopupSize {
                    width: self.config.ui.popup.width.clone(),
                    height: self.config.ui.popup.height.clone(),
                })
        } else {
            self.store_previous_size(&mut record, &current_size);
            full_size
        };
        self.apply_popup_size(&target, &mut record, size)?;
        Ok(Output::Text(format!(
            "toggled scratchpad `{}` fullscreen",
            target.name
        )))
    }

    fn reset(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        let mut record = self.live_record(&target)?;
        let size = PopupSize {
            width: self.config.ui.popup.width.clone(),
            height: self.config.ui.popup.height.clone(),
        };
        self.apply_popup_size(&target, &mut record, size)?;
        Ok(Output::Text(format!(
            "reset scratchpad `{}` to its configured size",
            target.name
        )))
    }

    fn live_record(&self, target: &Target) -> anyhow::Result<ScratchpadRecord> {
        let Some(record) = self.registry.scratchpads.get(&target.key).cloned() else {
            anyhow::bail!("scratchpad `{}` is not open", target.name);
        };
        if record.handle.is_none() {
            anyhow::bail!("scratchpad `{}` has no live runtime", target.name);
        }
        Ok(record)
    }

    fn popup_size(&self, record: &ScratchpadRecord) -> anyhow::Result<PopupSize> {
        let Some(handle) = record.handle.as_ref() else {
            anyhow::bail!("scratchpad has no live runtime");
        };
        if !handle.is_popup() {
            anyhow::bail!("popup sizing applies to popup scratchpads only");
        }
        self.ensure_live(handle)?;
        Ok(stored_popup_size(handle).unwrap_or_else(|| self.config.ui.popup.size()))
    }

    fn stored_previous_size(&self, record: &ScratchpadRecord) -> Option<PopupSize> {
        stored_popup_size_key(record.handle.as_ref()?, "previous_size")
    }

    fn store_previous_size(&mut self, record: &mut ScratchpadRecord, size: &PopupSize) {
        if let Some(handle) = record.handle.as_mut() {
            handle
                .opaque
                .insert("previous_size".to_string(), popup_size_json(size));
        }
    }

    fn apply_popup_size(
        &mut self,
        target: &Target,
        record: &mut ScratchpadRecord,
        size: PopupSize,
    ) -> anyhow::Result<()> {
        let Some(handle) = record.handle.as_ref() else {
            anyhow::bail!("scratchpad has no live runtime");
        };
        if self.store.viewer_is_active(&target.key) {
            self.herdr.hide_handle(handle)?;
        }
        self.herdr
            .show_handle(handle, &target.key, &self.config.ui.popup, Some(&size))?;

        let mut handle = handle.clone();
        handle
            .opaque
            .insert("size".to_string(), popup_size_json(&size));
        record.handle = Some(handle);
        let previous = self.herdr.current_pane().ok().map(FocusSnapshot::from);
        record.status = LifecycleStatus::Visible;
        record.last_shown_at = now_rfc3339();
        record.previous_focus = previous;
        self.registry.insert(target.key.clone(), record.clone());
        self.save()?;
        Ok(())
    }

    fn activate_or_open(
        &mut self,
        target: Target,
        previous: Option<crate::herdr::PaneInfo>,
        command: Option<Vec<String>>,
    ) -> anyhow::Result<Output> {
        let existing = self.registry.scratchpads.get(&target.key).cloned();
        let host_cwd = host_cwd(previous.as_ref(), &target);
        if self.config.behavior.reuse_existing
            && let Some(record) = existing.as_ref()
            && let Some(handle) = record.handle.as_ref()
            && self.ensure_live(handle).is_ok()
        {
            self.rename_handle_best_effort(handle, &self.title_for(&target.name));
            if handle.is_popup() && self.store.viewer_is_active(&target.key) {
                self.update_visible(&target.key, previous.map(FocusSnapshot::from));
                self.save()?;
                return Ok(Output::Text(format!(
                    "scratchpad `{}` is already visible",
                    target.name
                )));
            }
            self.herdr
                .show_handle(handle, &target.key, &self.config.ui.popup, None)?;
            self.ensure_path_sync(&target, host_cwd.as_deref());
            self.update_visible(&target.key, previous.map(FocusSnapshot::from));
            self.save()?;
            return Ok(Output::Text(format!("opened scratchpad `{}`", target.name)));
        }

        let previous_focus = previous.map(FocusSnapshot::from);
        let record = self.create_record(
            &target,
            previous_focus.clone(),
            existing,
            command.as_deref(),
        )?;
        let name = record.name.clone();
        let handle = record
            .handle
            .clone()
            .ok_or_else(|| anyhow::anyhow!("new scratchpad is missing its runtime handle"))?;
        let key = target.key.clone();
        self.registry.insert(key.clone(), record);
        self.save()?;
        if let Err(err) = self
            .herdr
            .show_handle(&handle, &key, &self.config.ui.popup, None)
        {
            if let Some(record) = self.registry.scratchpads.get_mut(&key) {
                record.status = LifecycleStatus::Available;
            }
            self.save()?;
            return Err(err.into());
        }
        self.ensure_path_sync(&target, host_cwd.as_deref());
        self.update_visible(&key, previous_focus);
        self.save()?;
        Ok(Output::Text(format!("opened scratchpad `{name}`")))
    }

    fn create_record(
        &self,
        target: &Target,
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
            title: self.title_for(&target.name),
            backing_session: self.config.runtime.backing_session.clone(),
            placement: self.config.behavior.placement,
            split_direction: self.config.behavior.split_direction,
        })?;
        self.rename_handle_best_effort(&handle, &self.title_for(&target.name));
        let now = now_rfc3339();
        let stored_command = (!launch_command.is_empty()).then_some(launch_command);
        Ok(ScratchpadRecord {
            name: target.name.clone(),
            scope: target.scope.clone(),
            profile: scratch_config.profile,
            status: LifecycleStatus::Visible,
            launch_command: stored_command,
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
        if let Some(handle) = record.handle.as_ref() {
            if handle.is_popup() {
                if self.store.viewer_is_active(&target.key) {
                    self.herdr.hide_handle(handle)?;
                }
            } else if let Some(previous) = record.previous_focus.as_ref() {
                let _ = self.herdr.focus_previous(previous);
            }
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
        let Some(_pane_id) = handle.pane_id.as_deref() else {
            anyhow::bail!("scratchpad runtime is missing a pane handle");
        };
        self.herdr.handle_get(handle)?;
        if !handle.is_popup()
            && let Some(focus_token) = handle.focus_token()
        {
            self.herdr.tab_get(focus_token)?;
        }
        Ok(())
    }

    /// Mirrors the floax `change_path` behavior: after a bare-shell popup is
    /// shown, cd its backing terminal to the directory of the pane it was
    /// opened from. This is best-effort; a failed sync never breaks the open.
    fn ensure_path_sync(&mut self, target: &Target, host_cwd: Option<&str>) {
        if !self.config.behavior.change_path {
            return;
        }
        let Some(host_cwd) = host_cwd.filter(|cwd| !cwd.is_empty()) else {
            return;
        };
        let Some(record) = self.registry.scratchpads.get(&target.key).cloned() else {
            return;
        };
        if !bare_shell(&self.config, &record) {
            return;
        }
        let Some(handle) = record.handle else {
            return;
        };
        if !handle.is_popup() {
            return;
        }
        let current = self
            .herdr
            .handle_get(&handle)
            .ok()
            .and_then(|info| info.cwd);
        if current.as_deref() == Some(host_cwd) {
            return;
        }
        if self
            .herdr
            .run_command(&handle, &format!("cd {}", shell_quote(host_cwd)))
            .is_err()
        {
            return;
        }
        if let Some(record) = self.registry.scratchpads.get_mut(&target.key) {
            record.cwd = Some(host_cwd.to_string());
        }
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

    fn summaries(&mut self) -> anyhow::Result<Vec<ScratchpadSummary>> {
        let keys = self
            .registry
            .scratchpads
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.refresh_status(&key)?;
        }
        Ok(self
            .registry
            .scratchpads
            .iter()
            .map(|(key, record)| self.summary_for_key(key, record))
            .collect())
    }

    fn refresh_status(&mut self, key: &str) -> anyhow::Result<()> {
        let Some(handle) = self
            .registry
            .scratchpads
            .get(key)
            .and_then(|record| record.handle.clone())
        else {
            return Ok(());
        };
        let next = if self.ensure_live(&handle).is_err() {
            LifecycleStatus::Stale
        } else if handle.is_popup() {
            if self.store.viewer_is_active(key) {
                LifecycleStatus::Visible
            } else {
                LifecycleStatus::Available
            }
        } else {
            return Ok(());
        };
        if let Some(record) = self.registry.scratchpads.get_mut(key)
            && record.status != next
        {
            record.status = next;
            self.save()?;
        }
        Ok(())
    }

    fn summary_for_key(&self, key: &str, record: &ScratchpadRecord) -> ScratchpadSummary {
        let mut summary = summary_for(record);
        if record.handle.as_ref().is_some_and(RuntimeHandle::is_popup) {
            summary.status = if matches!(
                record.status,
                LifecycleStatus::Stale | LifecycleStatus::Error
            ) {
                record.status.to_string()
            } else if self.store.viewer_is_active(key) {
                LifecycleStatus::Visible.to_string()
            } else if record.handle.is_some() {
                LifecycleStatus::Available.to_string()
            } else {
                record.status.to_string()
            };
        }
        summary
    }

    fn doctor(&self) -> DoctorReport {
        let mut issues = Vec::new();
        let herdr_available = self.herdr.available();
        let herdr_version = self.herdr.version();
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
        if self.config.behavior.placement == crate::config::ScratchpadPlacement::Popup
            && herdr_version
                .as_deref()
                .is_some_and(|version| !herdr_supports_popup(version))
        {
            issues.push("popup placement requires Herdr 0.7.4 or newer".into());
        }
        DoctorReport {
            herdr_available,
            herdr_version,
            config_dir: self.paths.config_dir.display().to_string(),
            config_path: self.paths.config_file.display().to_string(),
            state_dir: self.paths.state_dir.display().to_string(),
            state_path: self.paths.registry_file.display().to_string(),
            scratchpad_count: self.registry.scratchpads.len(),
            issues,
        }
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

pub fn run_popup_attach(paths: Paths) -> anyhow::Result<()> {
    let session = required_env("HERDR_SCRATCH_BACKING_SESSION")?;
    let terminal_id = required_env("HERDR_SCRATCH_TERMINAL_ID")?;
    let key = required_env("HERDR_SCRATCH_REGISTRY_KEY")?;
    let store = RegistryStore::new(paths.registry_file.clone());
    let _viewer_lease = store.viewer_lease(&key)?;

    update_attach_status(&store, &key, LifecycleStatus::Visible, false)?;
    let herdr = HerdrCli::discover();
    let attach_result = herdr.attach_terminal(&session, &terminal_id);

    let live = store
        .load()
        .ok()
        .and_then(|registry| registry.scratchpads.get(&key).cloned())
        .and_then(|record| record.handle)
        .is_some_and(|handle| herdr.handle_get(&handle).is_ok());
    update_attach_status(
        &store,
        &key,
        if live {
            LifecycleStatus::Available
        } else {
            LifecycleStatus::Closed
        },
        !live,
    )?;
    attach_result?;
    Ok(())
}

const GUIDE_TEXT: &str = r#"
Scratch is ready
================

1. Open or hide your persistent scratchpad:
   herdr plugin action invoke toggle --plugin herdr.scratch

2. While the popup has focus, press ctrl+b q to hide it.
   The terminal keeps running in the background.

3. Run the toggle action again to bring it back.

Recommended keybinding (~/.config/herdr/config.toml):

   [[keys.command]]
   key = "prefix+p"
   type = "plugin_action"
   command = "herdr.scratch.toggle"
   description = "toggle scratchpad"

Then run: herdr server reload-config

Diagnostics: herdr plugin action invoke doctor --plugin herdr.scratch
Docs: https://github.com/AkashJana18/herdr-scratch

Press Enter to close this guide. You can also press ctrl+b q.
"#;

pub fn run_guide_pane() -> anyhow::Result<()> {
    use std::io::{self, Write};

    print!("{GUIDE_TEXT}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(())
}

fn update_attach_status(
    store: &RegistryStore,
    key: &str,
    status: LifecycleStatus,
    clear_handle: bool,
) -> anyhow::Result<()> {
    let _lock = store.lock()?;
    let mut registry = store.load()?;
    if let Some(record) = registry.scratchpads.get_mut(key) {
        record.status = status;
        if status == LifecycleStatus::Visible {
            record.last_shown_at = now_rfc3339();
        } else {
            record.last_hidden_at = Some(now_rfc3339());
        }
        if clear_handle {
            record.handle = None;
        }
        store.save(&registry)?;
    }
    Ok(())
}

fn required_env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("{key} is not set"))
}

fn herdr_supports_popup(version_output: &str) -> bool {
    let version = version_output
        .split_whitespace()
        .find(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        .unwrap_or_default();
    let mut parts = version
        .split('.')
        .filter_map(|part| part.parse::<u64>().ok());
    let current = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    current >= (0, 7, 4)
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

/// Working directory of the pane a scratchpad was opened from.
fn host_cwd(previous: Option<&crate::herdr::PaneInfo>, target: &Target) -> Option<String> {
    previous
        .and_then(|pane| pane.cwd.clone())
        .or_else(|| target.context.context_cwd())
}

/// A scratchpad is a bare shell when it has no launch command; legacy records
/// without a stored command fall back to their profile's command.
fn bare_shell(config: &Config, record: &ScratchpadRecord) -> bool {
    match record.launch_command.as_deref() {
        Some(command) => command.is_empty(),
        None => config.profile(&record.profile).command.is_empty(),
    }
}

/// POSIX single-quote escaping for a single-argument shell word.
fn shell_quote(path: &str) -> String {
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('\'');
    for ch in path.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn popup_size_json(size: &PopupSize) -> serde_json::Value {
    serde_json::json!({
        "width": size.width.as_arg(),
        "height": size.height.as_arg(),
    })
}

fn stored_popup_size(handle: &RuntimeHandle) -> Option<PopupSize> {
    stored_popup_size_key(handle, "size")
}

fn stored_popup_size_key(handle: &RuntimeHandle, key: &str) -> Option<PopupSize> {
    let value = handle.opaque.get(key)?;
    let width = PopupDimension::parse_str(value.get("width")?.as_str()?)?;
    let height = PopupDimension::parse_str(value.get("height")?.as_str()?)?;
    Some(PopupSize { width, height })
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

pub fn run_runtime_session() -> anyhow::Result<()> {
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
    use crate::config::PopupConfig;
    use crate::herdr::{HerdrError, PaneInfo, TabInfo};
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone, Default)]
    struct FakeHerdr {
        calls: Rc<RefCell<Vec<String>>>,
        fail_show: bool,
        backing_cwd: Option<String>,
    }

    impl Herdr for FakeHerdr {
        fn available(&self) -> bool {
            true
        }

        fn version(&self) -> Option<String> {
            Some("herdr 0.8.0".to_string())
        }

        fn current_pane(&self) -> Result<PaneInfo, HerdrError> {
            Ok(PaneInfo {
                pane_id: "w1:p1".to_string(),
                terminal_id: "term-1".to_string(),
                workspace_id: "w1".to_string(),
                tab_id: "w1:t1".to_string(),
                focused: true,
                cwd: Some("/repo".to_string()),
            })
        }

        fn open_guide(&self) -> Result<(), HerdrError> {
            self.calls.borrow_mut().push("open:guide".to_string());
            Ok(())
        }

        fn tab_get(&self, tab_id: &str) -> Result<TabInfo, HerdrError> {
            Ok(TabInfo {
                tab_id: tab_id.to_string(),
                workspace_id: "w1".to_string(),
                focused: false,
            })
        }

        fn handle_get(&self, handle: &RuntimeHandle) -> Result<PaneInfo, HerdrError> {
            Ok(PaneInfo {
                pane_id: handle.pane_id.clone().unwrap(),
                terminal_id: handle.terminal_id.clone().unwrap(),
                workspace_id: handle.workspace_id.clone().unwrap(),
                tab_id: handle.focus_token().unwrap().to_string(),
                focused: false,
                cwd: Some(
                    self.backing_cwd
                        .clone()
                        .unwrap_or_else(|| "/repo".to_string()),
                ),
            })
        }

        fn show_handle(
            &self,
            _handle: &RuntimeHandle,
            registry_key: &str,
            _popup: &PopupConfig,
            size: Option<&PopupSize>,
        ) -> Result<(), HerdrError> {
            match size {
                Some(size) => self.calls.borrow_mut().push(format!(
                    "show:{registry_key}:{}x{}",
                    size.width.as_arg(),
                    size.height.as_arg()
                )),
                None => self.calls.borrow_mut().push(format!("show:{registry_key}")),
            }
            if self.fail_show {
                Err(HerdrError::Unsupported("popup already open".to_string()))
            } else {
                Ok(())
            }
        }

        fn hide_handle(&self, _handle: &RuntimeHandle) -> Result<(), HerdrError> {
            self.calls.borrow_mut().push("hide".to_string());
            Ok(())
        }

        fn focus_previous(&self, _previous: &FocusSnapshot) -> Result<(), HerdrError> {
            Ok(())
        }

        fn open_scratchpad(
            &self,
            request: OpenScratchpadRequest,
        ) -> Result<RuntimeHandle, HerdrError> {
            self.calls
                .borrow_mut()
                .push(format!("open:{}", request.placement.as_str()));
            Ok(RuntimeHandle {
                kind: "herdr_popup".to_string(),
                pane_id: Some("w9:p1".to_string()),
                terminal_id: Some("term-9".to_string()),
                workspace_id: Some("w9".to_string()),
                session: Some(request.backing_session),
                opaque: BTreeMap::from([
                    (
                        "surface".to_string(),
                        serde_json::Value::String("popup".to_string()),
                    ),
                    (
                        "focus_token".to_string(),
                        serde_json::Value::String("w9:t1".to_string()),
                    ),
                ]),
            })
        }

        fn rename_handle(&self, _handle: &RuntimeHandle, _title: &str) -> Result<(), HerdrError> {
            Ok(())
        }

        fn close_handle(&self, _handle: &RuntimeHandle) -> Result<(), HerdrError> {
            self.calls.borrow_mut().push("close".to_string());
            Ok(())
        }

        fn send_text(&self, _handle: &RuntimeHandle, text: &str) -> Result<(), HerdrError> {
            self.calls.borrow_mut().push(format!("send:{text}"));
            Ok(())
        }

        fn run_command(&self, _handle: &RuntimeHandle, command: &str) -> Result<(), HerdrError> {
            self.calls.borrow_mut().push(format!("run:{command}"));
            Ok(())
        }
    }

    fn app_with_fake(fake: FakeHerdr) -> (tempfile::TempDir, ScratchApp<FakeHerdr>) {
        app_with_config(fake, Config::default())
    }

    fn app_with_config(
        fake: FakeHerdr,
        config: Config,
    ) -> (tempfile::TempDir, ScratchApp<FakeHerdr>) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("config"),
            state_dir: dir.path().join("state"),
            config_file: dir.path().join("config/config.toml"),
            registry_file: dir.path().join("state/registry.json"),
        };
        let app = ScratchApp::new(config, Registry::default(), paths, fake);
        (dir, app)
    }

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

    #[test]
    fn popup_version_check_accepts_minimum_and_newer() {
        assert!(!herdr_supports_popup("herdr 0.7.3"));
        assert!(herdr_supports_popup("herdr 0.7.4"));
        assert!(herdr_supports_popup("herdr 0.8.0"));
    }

    #[test]
    fn guide_opens_visible_popup() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        let output = app.handle(cli::Command::Guide).unwrap();

        assert!(matches!(output, Output::Text(text) if text == "opened Scratch quick start"));
        assert_eq!(calls.borrow().as_slice(), ["open:guide"]);
    }

    #[test]
    fn guide_covers_the_first_run_workflow() {
        assert!(GUIDE_TEXT.contains("action invoke toggle"));
        assert!(GUIDE_TEXT.contains("ctrl+b q"));
        assert!(GUIDE_TEXT.contains("herdr.scratch.toggle"));
        assert!(GUIDE_TEXT.contains("server reload-config"));
        assert!(GUIDE_TEXT.contains("action invoke doctor"));
    }

    #[test]
    fn first_toggle_creates_backing_runtime_then_shows_popup() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        app.toggle(None, None).unwrap();

        assert_eq!(app.registry.scratchpads.len(), 1);
        let key = app.registry.scratchpads.keys().next().unwrap();
        let record = app.registry.scratchpads.values().next().unwrap();
        assert_eq!(record.status, LifecycleStatus::Visible);
        assert_eq!(
            record.handle.as_ref().unwrap().session.as_deref(),
            Some("herdr-scratch")
        );
        assert_eq!(
            calls.borrow().clone(),
            vec!["open:popup".to_string(), format!("show:{key}")]
        );
    }

    #[test]
    fn failed_popup_open_keeps_backing_runtime_available() {
        let fake = FakeHerdr {
            fail_show: true,
            ..FakeHerdr::default()
        };
        let (_dir, mut app) = app_with_fake(fake);

        assert!(app.toggle(None, None).is_err());

        let record = app.registry.scratchpads.values().next().unwrap();
        assert_eq!(record.status, LifecycleStatus::Available);
        assert!(record.handle.is_some());
    }

    #[test]
    fn toggle_hides_popup_when_viewer_lease_is_active() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();
        let _lease = app.store.viewer_lease(&key).unwrap();

        app.toggle(None, None).unwrap();

        assert!(calls.borrow().iter().any(|call| call == "hide"));
        assert_eq!(
            app.registry.scratchpads.get(&key).unwrap().status,
            LifecycleStatus::Available
        );
    }

    #[test]
    fn resize_steps_a_popup_and_persists_the_size() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();

        app.handle(cli::Command::Resize(cli::ResizeArgs {
            direction: cli::ResizeDirection::Up,
            name: None,
        }))
        .unwrap();

        let record = app.registry.scratchpads.get(&key).unwrap();
        let size = stored_popup_size(record.handle.as_ref().unwrap()).unwrap();
        assert_eq!(size.width.as_arg(), "85%");
        assert_eq!(size.height.as_arg(), "85%");
        assert!(calls.borrow().contains(&format!("show:{key}:85%x85%")));

        app.handle(cli::Command::Resize(cli::ResizeArgs {
            direction: cli::ResizeDirection::Down,
            name: None,
        }))
        .unwrap();
        let size = stored_popup_size(
            app.registry
                .scratchpads
                .get(&key)
                .unwrap()
                .handle
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(size.width.as_arg(), "80%");
        assert_eq!(size.height.as_arg(), "80%");
    }

    #[test]
    fn fullscreen_toggles_and_restores_the_previous_size() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();

        app.handle(cli::Command::Resize(cli::ResizeArgs {
            direction: cli::ResizeDirection::Down,
            name: None,
        }))
        .unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();

        app.handle(cli::Command::Fullscreen(cli::NameArg { name: None }))
            .unwrap();
        let record = app.registry.scratchpads.get(&key).unwrap();
        let size = stored_popup_size(record.handle.as_ref().unwrap()).unwrap();
        assert_eq!(size.width.as_arg(), "100%");
        assert_eq!(size.height.as_arg(), "100%");
        assert!(calls.borrow().contains(&format!("show:{key}:100%x100%")));

        app.handle(cli::Command::Fullscreen(cli::NameArg { name: None }))
            .unwrap();
        let record = app.registry.scratchpads.get(&key).unwrap();
        let size = stored_popup_size(record.handle.as_ref().unwrap()).unwrap();
        assert_eq!(size.width.as_arg(), "75%");
        assert_eq!(size.height.as_arg(), "75%");
    }

    #[test]
    fn reset_returns_a_popup_to_its_configured_size() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();

        app.handle(cli::Command::Resize(cli::ResizeArgs {
            direction: cli::ResizeDirection::Up,
            name: None,
        }))
        .unwrap();
        app.handle(cli::Command::Reset(cli::NameArg { name: None }))
            .unwrap();

        let record = app.registry.scratchpads.get(&key).unwrap();
        let size = stored_popup_size(record.handle.as_ref().unwrap()).unwrap();
        assert_eq!(size.width.as_arg(), "80%");
        assert_eq!(size.height.as_arg(), "80%");
        assert!(calls.borrow().contains(&format!("show:{key}:80%x80%")));
    }

    #[test]
    fn shell_quote_single_quotes_and_escapes_embedded_quotes() {
        assert_eq!(shell_quote("/repo"), "'/repo'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn bare_shell_detects_command_scratchpads() {
        let mut config = Config::default();
        let bare_record = ScratchpadRecord {
            name: "scratch".to_string(),
            scope: ScopeRecord {
                kind: "workspace".to_string(),
                key: "w1".to_string(),
            },
            profile: "default".to_string(),
            status: LifecycleStatus::Available,
            launch_command: None,
            handle: None,
            cwd: None,
            created_at: now_rfc3339(),
            last_shown_at: now_rfc3339(),
            last_hidden_at: None,
            previous_focus: None,
        };
        assert!(bare_shell(&config, &bare_record));

        config.profiles.insert(
            "lazygit".to_string(),
            ProfileConfig {
                command: vec!["lazygit".into()],
                cwd: CwdMode::Context,
                env: Default::default(),
            },
        );
        let lazygit_record = ScratchpadRecord {
            profile: "lazygit".to_string(),
            launch_command: None,
            ..bare_record.clone()
        };
        assert!(!bare_shell(&config, &lazygit_record));

        let one_shot = ScratchpadRecord {
            profile: "default".to_string(),
            launch_command: Some(vec!["lazygit".into()]),
            ..bare_record.clone()
        };
        assert!(!bare_shell(&config, &one_shot));
        let empty_override = ScratchpadRecord {
            launch_command: Some(vec![]),
            ..bare_record
        };
        assert!(bare_shell(&config, &empty_override));
    }

    #[test]
    fn toggle_syncs_bare_popup_to_host_cwd_after_show() {
        let fake = FakeHerdr {
            backing_cwd: Some("/elsewhere".to_string()),
            ..FakeHerdr::default()
        };
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        app.toggle(None, None).unwrap();

        let key = app.registry.scratchpads.keys().next().unwrap().clone();
        assert!(calls.borrow().contains(&format!("show:{key}")));
        assert!(calls.borrow().contains(&"run:cd '/repo'".to_string()));
        assert_eq!(
            app.registry.scratchpads.get(&key).unwrap().cwd.as_deref(),
            Some("/repo")
        );
    }

    #[test]
    fn change_path_skips_sync_when_backing_cwd_already_matches() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        app.toggle(None, None).unwrap();

        assert!(!calls.borrow().iter().any(|call| call.starts_with("run:")));
    }

    #[test]
    fn change_path_disabled_never_syncs() {
        let mut config = Config::default();
        config.behavior.change_path = false;
        let fake = FakeHerdr {
            backing_cwd: Some("/elsewhere".to_string()),
            ..FakeHerdr::default()
        };
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_config(fake, config);

        app.toggle(None, None).unwrap();

        assert!(!calls.borrow().iter().any(|call| call.starts_with("run:")));
    }

    #[test]
    fn change_path_skips_command_scratchpads() {
        let mut config = Config::default();
        config.profiles.insert(
            "lazygit".to_string(),
            ProfileConfig {
                command: vec!["lazygit".into()],
                cwd: CwdMode::Context,
                env: Default::default(),
            },
        );
        config.scratchpads.insert(
            "git".to_string(),
            ScratchpadConfig {
                profile: "lazygit".to_string(),
                scope: None,
            },
        );
        let fake = FakeHerdr {
            backing_cwd: Some("/elsewhere".to_string()),
            ..FakeHerdr::default()
        };
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_config(fake, config);

        app.toggle(Some("git"), None).unwrap();

        assert!(!calls.borrow().iter().any(|call| call.starts_with("run:")));
        let key = app.registry.scratchpads.keys().next().unwrap().clone();
        assert_eq!(
            app.registry.scratchpads.get(&key).unwrap().cwd.as_deref(),
            Some("/repo")
        );
    }
}
