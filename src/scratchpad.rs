use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::{
    cli,
    config::{Config, CwdMode, Paths, ProfileConfig, ScopeKind, ScratchpadConfig},
    herdr::{Herdr, HerdrCli, OpenScratchpadRequest},
    obsidian::{self, NotesSource},
    output::Output,
    registry::{
        FocusSnapshot, LifecycleStatus, Registry, RegistryStore, RuntimeHandle, ScopeRecord,
        ScratchpadRecord, ViewerInfo, now_rfc3339, registry_key,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadSummary {
    pub name: String,
    pub scope: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub herdr_available: bool,
    pub herdr_version: Option<String>,
    pub server_ok: bool,
    pub config_dir: String,
    pub config_path: String,
    pub state_dir: String,
    pub state_path: String,
    pub scratchpad_count: usize,
    #[serde(default)]
    pub keybinding_missing: usize,
    #[serde(default)]
    pub notes_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_file: Option<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
struct DailyTarget {
    file: PathBuf,
    source: NotesSource,
    editor: String,
    date_str: String,
    template: Option<PathBuf>,
    cwd: PathBuf,
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
            cli::Command::Daily(args) => self.daily(&args),
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
            cli::Command::Setup => self.setup(),
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

    fn daily(&mut self, args: &cli::DailyArgs) -> anyhow::Result<Output> {
        let date = parse_daily_date(args.date.as_deref())?;
        let target = self.resolve_daily_target(args.vault.as_deref(), date)?;
        obsidian::ensure_daily_file(&target.file, target.template.as_deref(), &target.date_str)?;
        if args.print_path {
            return Ok(Output::Text(target.file.display().to_string()));
        }
        self.open_daily(target)
    }

    fn resolve_daily_target(
        &self,
        vault_override: Option<&str>,
        date: time::Date,
    ) -> anyhow::Result<DailyTarget> {
        let notes = &self.config.notes;
        let cli_vault = vault_override.map(str::trim).filter(|s| !s.is_empty());
        let explicit = cli_vault.or_else(|| notes.vault_path_set());
        let from_cli = cli_vault.is_some();

        let (vault, source) = if let Some(raw) = explicit {
            let path = PathBuf::from(raw);
            if !path.is_dir() {
                if from_cli {
                    anyhow::bail!(
                        "vault `{raw}` does not exist; pass an existing Obsidian vault path"
                    );
                }
                anyhow::bail!(
                    "configured notes vault `{raw}` does not exist; fix `notes.vault_path` or unset it to use auto-detect"
                );
            }
            (Some(path), NotesSource::Explicit)
        } else if notes.vault_auto {
            let detected =
                obsidian::pick_vault(obsidian::detect_vaults(notes.obsidian_config.as_deref()))
                    .or_else(obsidian::fallback_vault);
            match detected {
                Some(path) => (Some(path), NotesSource::Auto),
                None => (None, NotesSource::Fallback),
            }
        } else {
            (None, NotesSource::Fallback)
        };

        let vault_config = vault
            .as_deref()
            .map(obsidian::read_daily_config)
            .unwrap_or_default();
        let folder = notes
            .daily_subdir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_matches('/').to_string())
            .unwrap_or(vault_config.folder);
        let format = notes
            .daily_format
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&vault_config.format)
            .to_string();
        let format = if format.trim().is_empty() {
            "YYYY-MM-DD".to_string()
        } else {
            format
        };
        let template = match notes
            .template_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(raw) => Some(PathBuf::from(raw)),
            None => {
                obsidian::resolve_template_path(vault.as_deref(), vault_config.template.as_deref())
            }
        };

        let filename = obsidian::daily_filename(date, &format);
        let file = match vault.as_deref() {
            Some(vault) => {
                let folder_path = if folder.is_empty() {
                    vault.to_path_buf()
                } else {
                    vault.join(&folder)
                };
                folder_path.join(&filename)
            }
            None => {
                let base = notes
                    .fallback_dir
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.paths.state_dir.join("daily"));
                base.join(obsidian::daily_filename(date, &format))
            }
        };
        let date_str = obsidian::iso_date(date);
        let editor = obsidian::resolve_editor(notes.editor.as_deref());
        let cwd = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.paths.state_dir.clone());
        Ok(DailyTarget {
            file,
            source,
            editor,
            date_str,
            template,
            cwd,
        })
    }

    fn open_daily(&mut self, target: DailyTarget) -> anyhow::Result<Output> {
        let scope = ScopeRecord {
            kind: "global".to_string(),
            key: "default".to_string(),
        };
        let key = registry_key(&scope, "daily");
        let file_str = target.file.display().to_string();
        let launch = vec![target.editor.clone(), file_str.clone()];
        let title = Self::daily_title(&target.date_str);
        let current = self.herdr.current_pane().ok();

        // Same-day reuse: a live `daily` runtime already on today's file just
        // gets shown/focused like any other scratchpad.
        if self.config.behavior.reuse_existing
            && let Some(record) = self.registry.scratchpads.get(&key).cloned()
            && let Some(handle) = record.handle.clone()
            && record.launch_command.as_deref() == Some(launch.as_slice())
            && self.ensure_live(&handle).is_ok()
        {
            if self.viewer_active(&key) {
                self.rename_viewer_best_effort(&key, &title);
                self.viewer_focus(&key);
                self.update_visible(&key, current.map(FocusSnapshot::from));
                self.save()?;
                return Ok(Output::Text(format!(
                    "daily note `{}` is already visible ({})",
                    target.date_str, target.source
                )));
            }
            let viewer = self.show_or_friendly(&handle, &key)?;
            self.record_viewer(&key, viewer);
            self.rename_viewer_best_effort(&key, &title);
            self.viewer_focus(&key);
            self.update_visible(&key, current.map(FocusSnapshot::from));
            self.save()?;
            return Ok(Output::Text(format!(
                "opened daily note `{}` ({})",
                target.date_str, target.source
            )));
        }

        // New day (or stale/missing runtime): tear down the old runtime so the
        // single global `daily` scratchpad always tracks the resolved file.
        if let Some(old) = self.registry.scratchpads.get(&key).cloned()
            && let Some(handle) = old.handle.as_ref()
        {
            let _ = self.herdr.close_handle(handle);
        }

        let previous_focus = current.map(FocusSnapshot::from);
        let mut env = BTreeMap::new();
        env.insert("HERDR_SCRATCH_NAME".to_string(), "daily".to_string());
        env.insert("HERDR_SCRATCH_PROFILE".to_string(), "daily".to_string());
        env.insert(
            "HERDR_SCRATCH_COMMAND_JSON".to_string(),
            serde_json::to_string(&launch)?,
        );
        let handle = self.herdr.open_scratchpad(OpenScratchpadRequest {
            cwd: Some(target.cwd.display().to_string()),
            env,
            title: title.clone(),
            backing_session: self.config.runtime.backing_session.clone(),
            placement: self.config.behavior.placement,
            split_direction: self.config.behavior.split_direction,
        })?;
        self.rename_handle_best_effort(&handle, &title);
        let now = now_rfc3339();
        let created_at = self
            .registry
            .scratchpads
            .get(&key)
            .map(|record| record.created_at.clone())
            .unwrap_or_else(|| now.clone());
        self.registry.insert(
            key.clone(),
            ScratchpadRecord {
                name: "daily".to_string(),
                scope,
                profile: "daily".to_string(),
                status: LifecycleStatus::Visible,
                launch_command: Some(launch),
                handle: Some(handle.clone()),
                cwd: Some(target.cwd.display().to_string()),
                created_at,
                last_shown_at: now,
                last_hidden_at: None,
                previous_focus: previous_focus.clone(),
                viewer: None,
            },
        );
        self.save()?;
        let viewer = match self.show_or_friendly(&handle, &key) {
            Ok(viewer) => viewer,
            Err(err) => {
                if let Some(record) = self.registry.scratchpads.get_mut(&key) {
                    record.status = LifecycleStatus::Available;
                }
                self.save()?;
                return Err(err);
            }
        };
        self.record_viewer(&key, viewer);
        self.rename_viewer_best_effort(&key, &title);
        self.update_visible(&key, previous_focus);
        self.save()?;
        Ok(Output::Text(format!(
            "opened daily note `{}` ({})",
            target.date_str, target.source
        )))
    }

    fn toggle(
        &mut self,
        name: Option<&str>,
        command: Option<Vec<String>>,
    ) -> anyhow::Result<Output> {
        let current = self.herdr.current_pane().ok();
        let target = self.target(name, current.as_ref())?;
        if self.config.behavior.toggle_returns_to_previous && self.viewer_focused(&target.key) {
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
        if !self.viewer_active(&target.key) {
            let viewer = self.show_or_friendly(handle, &target.key)?;
            self.record_viewer(&target.key, viewer);
            self.ensure_path_sync(&target, host_cwd(current.as_ref(), &target).as_deref());
        }
        let title = self.display_title(&record.name, record.launch_command.as_deref());
        self.rename_viewer_best_effort(&target.key, &title);
        self.viewer_focus(&target.key);
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
            // Closing the backing runtime tears down the overlay viewer too,
            // since the attach process exits when its terminal is gone.
            let _ = self.herdr.close_handle(handle);
        }
        record.status = LifecycleStatus::Closed;
        record.handle = None;
        record.viewer = None;
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
                surface: None,
                size: None,
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
            if self.viewer_active(&key) {
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
        _direction: cli::ResizeDirection,
        name: Option<&str>,
    ) -> anyhow::Result<Output> {
        let _ = name;
        Ok(Output::Text(
            "sizing no longer applies: the scratchpad viewer now uses the overlay surface, which always fills the pane (see `herdr-scratch guide`)".to_string(),
        ))
    }

    fn fullscreen(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let _ = name;
        Ok(Output::Text(
            "fullscreen no longer applies: the scratchpad viewer now uses the overlay surface, which always fills the pane (see `herdr-scratch guide`)".to_string(),
        ))
    }

    fn reset(&mut self, name: Option<&str>) -> anyhow::Result<Output> {
        let _ = name;
        Ok(Output::Text(
            "sizing no longer applies: the scratchpad viewer now uses the overlay surface, which always fills the pane (see `herdr-scratch guide`)".to_string(),
        ))
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
            let title = self.display_title(&record.name, record.launch_command.as_deref());
            self.rename_handle_best_effort(handle, &title);
            if self.viewer_active(&target.key) {
                self.rename_viewer_best_effort(&target.key, &title);
                self.viewer_focus(&target.key);
                self.update_visible(&target.key, previous.map(FocusSnapshot::from));
                self.save()?;
                return Ok(Output::Text(format!(
                    "scratchpad `{}` is already visible",
                    target.name
                )));
            }
            let viewer = self.show_or_friendly(handle, &target.key)?;
            self.record_viewer(&target.key, viewer);
            self.rename_viewer_best_effort(&target.key, &title);
            self.ensure_path_sync(&target, host_cwd.as_deref());
            self.viewer_focus(&target.key);
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
        let viewer = match self.show_or_friendly(&handle, &key) {
            Ok(viewer) => viewer,
            Err(err) => {
                if let Some(record) = self.registry.scratchpads.get_mut(&key) {
                    record.status = LifecycleStatus::Available;
                }
                self.save()?;
                return Err(err);
            }
        };
        self.ensure_path_sync(&target, host_cwd.as_deref());
        self.record_viewer(&key, viewer);
        let title = self.display_title(&target.name, command.as_deref());
        self.rename_viewer_best_effort(&key, &title);
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
            title: self.display_title(&target.name, command_override),
            backing_session: self.config.runtime.backing_session.clone(),
            placement: self.config.behavior.placement,
            split_direction: self.config.behavior.split_direction,
        })?;
        self.rename_handle_best_effort(
            &handle,
            &self.display_title(&target.name, command_override),
        );
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
            viewer: None,
        })
    }

    fn hide_by_target(&mut self, target: Target) -> anyhow::Result<Output> {
        let Some(mut record) = self.registry.scratchpads.get(&target.key).cloned() else {
            return Ok(Output::Text(format!(
                "scratchpad `{}` is not open",
                target.name
            )));
        };
        // With the overlay viewer, hiding means returning focus to the context
        // we were in before showing the scratchpad. The overlay pane stays
        // alive so a later toggle can refocus it without reopening.
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

    /// Show a scratchpad, mapping a Herdr "popup already open" rejection to a
    /// friendly hint before propagating any other error. Returns the overlay
    /// viewer pane that is now showing the scratchpad, if it is an overlay.
    fn show_or_friendly(
        &self,
        handle: &RuntimeHandle,
        key: &str,
    ) -> anyhow::Result<Option<ViewerInfo>> {
        if let Some(viewer) = self.alive_viewer(key) {
            return Ok(Some(viewer));
        }
        match self.herdr.show_handle(handle, key) {
            Ok(Some(pane)) => Ok(Some(viewer_from_pane(&pane))),
            Ok(None) => Ok(None),
            Err(err) if err.is_popup_open() => Err(anyhow::anyhow!(
                "another popup is already open; detach it with ctrl+b q and try again"
            )),
            Err(err) => Err(err.into()),
        }
    }

    /// Returns the recorded viewer pane if it is still alive.
    fn alive_viewer(&self, key: &str) -> Option<ViewerInfo> {
        let viewer = self.registry.scratchpads.get(key)?.viewer.clone()?;
        if self.herdr.handle_get(&handle_from_viewer(&viewer)).is_ok() {
            Some(viewer)
        } else {
            None
        }
    }

    /// True when the overlay viewer pane for `key` is alive and is currently
    /// the focused pane (i.e. the scratchpad is on screen).
    fn viewer_focused(&self, key: &str) -> bool {
        let Some(viewer) = self.alive_viewer(key) else {
            return false;
        };
        self.herdr
            .current_pane()
            .ok()
            .is_some_and(|pane| pane.pane_id == viewer.pane_id)
    }

    /// True when the overlay viewer pane for `key` is alive, regardless of
    /// whether it currently has focus.
    fn viewer_active(&self, key: &str) -> bool {
        self.alive_viewer(key).is_some()
    }

    fn update_visible(&mut self, key: &str, previous: Option<FocusSnapshot>) {
        if let Some(record) = self.registry.scratchpads.get_mut(key) {
            record.status = LifecycleStatus::Visible;
            record.last_shown_at = now_rfc3339();
            record.previous_focus = previous;
        }
    }

    fn record_viewer(&mut self, key: &str, viewer: Option<ViewerInfo>) {
        if let Some(record) = self.registry.scratchpads.get_mut(key) {
            record.viewer = viewer;
        }
    }

    /// Give focus to the alive overlay viewer pane for `key`, if one exists.
    fn viewer_focus(&self, key: &str) {
        let Some(viewer) = self.alive_viewer(key) else {
            return;
        };
        let _ = self.herdr.focus_pane(&viewer.pane_id);
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

    /// Display title for a scratchpad. Follows the configured template,
    /// except a one-shot command on the default scratchpad shows the
    /// command's basename (`toggle -- lazygit` -> `Scratchpad:lazygit`
    /// instead of `Scratchpad:scratch`).
    fn display_title(&self, name: &str, launch: Option<&[String]>) -> String {
        if name == self.config.default_scratchpad
            && let Some(command) = launch.and_then(|args| args.first())
        {
            let base = command.rsplit('/').next().unwrap_or(command).trim();
            if !base.is_empty() {
                return self.title_for(base);
            }
        }
        self.title_for(name)
    }

    fn daily_title(date_str: &str) -> String {
        format!("daily:{date_str}")
    }

    /// Rename the visible overlay viewer pane, when one is alive.
    /// Best-effort: a failed rename never breaks the open/focus flow.
    fn rename_viewer_best_effort(&self, key: &str, title: &str) {
        let Some(viewer) = self.alive_viewer(key) else {
            return;
        };
        self.rename_handle_best_effort(&handle_from_viewer(&viewer), title);
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
            if self.viewer_active(key) {
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
        if let Some(handle) = record.handle.as_ref() {
            summary.surface = handle.surface().map(str::to_string);
        }
        if record.handle.as_ref().is_some_and(RuntimeHandle::is_popup) {
            summary.status = if matches!(
                record.status,
                LifecycleStatus::Stale | LifecycleStatus::Error
            ) {
                record.status.to_string()
            } else if self.viewer_active(key) {
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
        let server_ok = herdr_available && self.herdr.server_reachable();
        if !herdr_available {
            issues
                .push("Herdr CLI is not available; set HERDR_BIN_PATH or add herdr to PATH".into());
        }
        if !server_ok {
            issues.push("Herdr server is not reachable; start it with `herdr server`".into());
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
        let keybinding_missing = match pending_keybindings(&herdr_config_path()) {
            Ok(pending) => {
                if pending.is_empty() {
                    0
                } else {
                    issues.push(format!(
                        "{} recommended keybinding(s) are not configured; run `herdr-scratch setup`",
                        pending.len()
                    ));
                    pending.len()
                }
            }
            Err(err) => {
                issues.push(format!(
                    "could not read the Herdr config for setup: {err:#}"
                ));
                0
            }
        };
        let (notes_source, notes_file) = match self.probe_daily_target() {
            Ok((source, file)) => (source.to_string(), Some(file)),
            Err(err) => {
                issues.push(format!("daily notes: {err:#}"));
                ("error".to_string(), None)
            }
        };
        if notes_source == NotesSource::Fallback.to_string() {
            issues.push(
                "no Obsidian vault detected; daily notes use a local fallback file (set `notes.vault_path` or open a vault in Obsidian)".to_string(),
            );
        }
        if let Some(template) = self
            .config
            .notes
            .template_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !Path::new(template).exists()
        {
            issues.push(format!(
                "notes template `{template}` does not exist; daily notes use the builtin skeleton"
            ));
        }
        DoctorReport {
            herdr_available,
            herdr_version,
            server_ok,
            config_dir: self.paths.config_dir.display().to_string(),
            config_path: self.paths.config_file.display().to_string(),
            state_dir: self.paths.state_dir.display().to_string(),
            state_path: self.paths.registry_file.display().to_string(),
            scratchpad_count: self.registry.scratchpads.len(),
            keybinding_missing,
            notes_source,
            notes_file,
            issues,
        }
    }

    /// Best-effort daily resolution for `doctor` (never creates files).
    fn probe_daily_target(&self) -> anyhow::Result<(NotesSource, String)> {
        let today = time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .date();
        let target = self.resolve_daily_target(None, today)?;
        Ok((target.source, target.file.display().to_string()))
    }

    fn save(&self) -> anyhow::Result<()> {
        self.store.save(&self.registry)
    }

    fn setup(&mut self) -> anyhow::Result<Output> {
        let path = herdr_config_path();
        let (added, skipped) = write_keybindings(&path)?;
        if added == 0 {
            return Ok(Output::Text(
                "keybindings are already configured for herdr-scratch".to_string(),
            ));
        }
        Ok(Output::Text(format!(
            "wrote {added} recommended keybinding(s) to {}\n({skipped} already configured)\nreload Herdr with: herdr server reload-config",
            path.display()
        )))
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
    let herdr = HerdrCli::discover();
    let attach_result = herdr.attach_terminal(&session, &terminal_id);

    let live = store
        .load()
        .ok()
        .and_then(|registry| registry.scratchpads.get(&key).cloned())
        .and_then(|record| record.handle)
        .is_some_and(|handle| herdr.handle_get(&handle).is_ok());
    if !live {
        update_attach_status(&store, &key, LifecycleStatus::Closed, !live)?;
    }
    attach_result?;
    Ok(())
}

const GUIDE_TEXT: &str = r#"
Scratch is ready
================

1. Open or hide your persistent scratchpad:
   herdr plugin action invoke toggle --plugin herdr.scratch

2. The scratchpad viewer uses an overlay pane, so all Herdr keys
   (including ctrl+b p for toggle) keep working while it is focused.

3. Run the toggle action again to hide it.
   The terminal keeps running in the background.

Recommended keybinding (~/.config/herdr/config.toml):

   [[keys.command]]
   key = "prefix+p"
   type = "plugin_action"
   command = "herdr.scratch.toggle"
   description = "toggle scratchpad"

Then run: herdr server reload-config

Or let the plugin write the recommended bindings:
   herdr-scratch setup

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

/// Recommended Herdr keybindings written by `setup`: (key, action, description).
const RECOMMENDED_KEYS: &[(&str, &str, &str)] = &[
    ("prefix+p", "herdr.scratch.toggle", "toggle scratchpad"),
    ("prefix+shift+p", "herdr.scratch.list", "list scratchpads"),
    (
        "prefix+shift+g",
        "herdr.scratch.lazygit",
        "toggle lazygit scratchpad",
    ),
    ("prefix+n", "herdr.scratch.notes", "toggle notes scratchpad"),
    ("prefix+d", "herdr.scratch.daily", "open daily note"),
];

fn herdr_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("HERDR_CONFIG_FILE")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("herdr/config.toml");
    }
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/herdr/config.toml")
}

/// Bindings already present in the Herdr config as `herdr.scratch.*` actions.
fn bound_actions(value: &toml::Value) -> HashSet<String> {
    value
        .get("keys")
        .and_then(|keys| keys.get("command"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("command").and_then(toml::Value::as_str))
        .filter(|command| command.starts_with("herdr.scratch."))
        .map(str::to_string)
        .collect()
}

/// Keys already bound to any action, so `setup` never steals a user's keys.
/// Every `[[keys.*]]` chord table counts (command, goto, session, tab, ...).
fn taken_keys(value: &toml::Value) -> HashSet<String> {
    value
        .get("keys")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|keys| keys.values())
        .filter_map(toml::Value::as_array)
        .flatten()
        .filter_map(|entry| entry.get("key").and_then(toml::Value::as_str))
        .map(str::to_string)
        .collect()
}

fn recommended_block(bindings: &[(&str, &str, &str)]) -> String {
    let mut out = String::new();
    out.push_str("# Added by herdr-scratch setup\n");
    for (key, action, description) in bindings {
        out.push_str("[[keys.command]]\n");
        out.push_str(&format!("key = {key:?}\n"));
        out.push_str("type = \"plugin_action\"\n");
        out.push_str(&format!("command = {action:?}\n"));
        out.push_str(&format!("description = {description:?}\n"));
    }
    out
}

/// Recommended bindings from `RECOMMENDED_KEYS` that are NOT yet present in
/// the given Herdr config (read-only; nothing is written).
fn pending_keybindings(
    path: &std::path::Path,
) -> anyhow::Result<Vec<(&'static str, &'static str, &'static str)>> {
    let content = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };
    let value: toml::Value = if content.trim().is_empty() {
        toml::Value::Table(Default::default())
    } else {
        toml::from_str(&content).map_err(|err| {
            anyhow::anyhow!("could not parse {path}: {err}", path = path.display())
        })?
    };
    let actions = bound_actions(&value);
    let keys = taken_keys(&value);
    Ok(RECOMMENDED_KEYS
        .iter()
        .copied()
        .filter(|(key, action, _)| !keys.contains(*key) && !actions.contains(*action))
        .collect())
}

/// Idempotently append the recommended `[[keys.command]]` bindings to a Herdr
/// config file. Returns the number added and the number skipped.
fn write_keybindings(path: &std::path::Path) -> anyhow::Result<(usize, usize)> {
    let pending = pending_keybindings(path)?;
    let added = pending.len();
    let skipped = RECOMMENDED_KEYS.len() - added;
    if added == 0 {
        return Ok((0, skipped));
    }
    let content = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut block = recommended_block(&pending);
    if !content.is_empty() {
        if !content.ends_with('\n') {
            block.insert(0, '\n');
        }
        block.insert(0, '\n');
    }
    std::fs::write(path, format!("{content}{block}"))?;
    Ok((added, skipped))
}

/// A minimal handle describing the overlay viewer pane, used to probe liveness.
fn handle_from_viewer(viewer: &ViewerInfo) -> RuntimeHandle {
    RuntimeHandle {
        kind: "herdr_pane".to_string(),
        pane_id: Some(viewer.pane_id.clone()),
        terminal_id: Some(viewer.terminal_id.clone()),
        workspace_id: Some(viewer.workspace_id.clone()),
        session: None,
        opaque: Default::default(),
    }
}

/// Records the pane Herdr opened for the overlay viewer.
fn viewer_from_pane(pane: &crate::herdr::PaneInfo) -> ViewerInfo {
    ViewerInfo {
        pane_id: pane.pane_id.clone(),
        workspace_id: pane.workspace_id.clone(),
        tab_id: pane.tab_id.clone(),
        terminal_id: pane.terminal_id.clone(),
        focus_token: pane.tab_id.clone(),
    }
}

fn parse_scope(raw: Option<&str>) -> anyhow::Result<ScopeKind> {
    match raw.unwrap_or("workspace") {
        "global" => Ok(ScopeKind::Global),
        "workspace" => Ok(ScopeKind::Workspace),
        "cwd" => Ok(ScopeKind::Cwd),
        other => anyhow::bail!("invalid scope `{other}`; expected global, workspace, or cwd"),
    }
}

/// Parse `--date YYYY-MM-DD`, defaulting to today (local, else UTC).
fn parse_daily_date(raw: Option<&str>) -> anyhow::Result<time::Date> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .date());
    };
    let mut parts = raw.split('-');
    let invalid = || anyhow::anyhow!("invalid --date `{raw}`; expected YYYY-MM-DD");
    let year: i32 = parts
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let month_num: u8 = parts
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let day: u8 = parts
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    let month = time::Month::try_from(month_num).map_err(|_| invalid())?;
    time::Date::from_calendar_date(year, month, day).map_err(|_| invalid())
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
        surface: None,
        size: None,
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
    use crate::herdr::{HerdrError, PaneInfo, TabInfo};
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone, Default)]
    struct FakeHerdr {
        calls: Rc<RefCell<Vec<String>>>,
        fail_show: bool,
        popup_busy: bool,
        backing_cwd: Option<String>,
        /// The pane the fake reports as currently focused (defaults to w1:p1).
        focused_pane: Option<String>,
        /// When set, `handle_get` reports the pane as gone.
        handle_gone: bool,
    }

    const FAKE_VIEWER_PANE: &str = "w9:p9";

    impl Herdr for FakeHerdr {
        fn available(&self) -> bool {
            true
        }

        fn version(&self) -> Option<String> {
            Some("herdr 0.8.0".to_string())
        }

        fn server_reachable(&self) -> bool {
            true
        }

        fn current_pane(&self) -> Result<PaneInfo, HerdrError> {
            Ok(PaneInfo {
                pane_id: self
                    .focused_pane
                    .clone()
                    .unwrap_or_else(|| "w1:p1".to_string()),
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
            if self.handle_gone {
                return Err(HerdrError::Unsupported("pane gone".to_string()));
            }
            Ok(PaneInfo {
                pane_id: handle.pane_id.clone().unwrap(),
                terminal_id: handle.terminal_id.clone().unwrap(),
                workspace_id: handle.workspace_id.clone().unwrap(),
                tab_id: handle.focus_token().unwrap_or("w1:t1").to_string(),
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
        ) -> Result<Option<PaneInfo>, HerdrError> {
            self.calls.borrow_mut().push(format!("show:{registry_key}"));
            if self.fail_show {
                Err(HerdrError::Unsupported("popup already open".to_string()))
            } else if self.popup_busy {
                Err(HerdrError::Api(serde_json::json!({
                    "error": { "message": "another popup is already open" }
                })))
            } else {
                Ok(Some(PaneInfo {
                    pane_id: FAKE_VIEWER_PANE.to_string(),
                    terminal_id: "term-viewer".to_string(),
                    workspace_id: "w9".to_string(),
                    tab_id: "w9:t1".to_string(),
                    focused: true,
                    cwd: Some("/repo".to_string()),
                }))
            }
        }

        fn focus_pane(&self, _pane_id: &str) -> Result<(), HerdrError> {
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

        fn rename_handle(&self, handle: &RuntimeHandle, title: &str) -> Result<(), HerdrError> {
            self.calls.borrow_mut().push(format!(
                "rename:{}:{title}",
                handle.pane_id.as_deref().unwrap_or("?")
            ));
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
            vec![
                "open:popup".to_string(),
                "rename:w9:p1:Scratchpad:scratch".to_string(),
                format!("show:{key}"),
                "rename:w9:p9:Scratchpad:scratch".to_string(),
            ]
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
    fn toggle_hides_overlay_when_viewer_pane_is_focused() {
        let fake = FakeHerdr {
            focused_pane: Some(FAKE_VIEWER_PANE.to_string()),
            ..FakeHerdr::default()
        };
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();
        assert!(app.registry.scratchpads.get(&key).unwrap().viewer.is_some());

        app.toggle(None, None).unwrap();
        assert_eq!(
            app.registry.scratchpads.get(&key).unwrap().status,
            LifecycleStatus::Available
        );
    }

    #[test]
    fn toggle_from_outside_refocuses_an_alive_viewer_without_reopening() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();
        let show_count = calls.borrow().iter().filter(|c| *c == "show").count();

        let record = app.registry.scratchpads.get(&key).unwrap().clone();
        assert_eq!(record.status, LifecycleStatus::Visible);

        app.toggle(None, None).unwrap();
        let new_show_count = calls.borrow().iter().filter(|c| *c == "show").count();
        assert_eq!(new_show_count, show_count);
    }

    #[test]
    fn refresh_marks_overlay_available_when_viewer_pane_is_gone() {
        let fake = FakeHerdr {
            handle_gone: true,
            ..FakeHerdr::default()
        };
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();
        app.refresh_status(&key).unwrap();
        let record = app.registry.scratchpads.get(&key).unwrap();
        assert_eq!(record.status, LifecycleStatus::Stale);
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
            viewer: None,
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

    #[test]
    fn setup_writes_recommended_keybindings_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr/config.toml");

        let (added, skipped) = write_keybindings(&path).unwrap();
        assert_eq!(added, RECOMMENDED_KEYS.len());
        assert_eq!(skipped, 0);

        let content = std::fs::read_to_string(&path).unwrap();
        for (key, action, description) in RECOMMENDED_KEYS {
            assert!(content.contains(&format!("key = {key:?}")));
            assert!(content.contains(&format!("command = {action:?}")));
            assert!(content.contains(&format!("description = {description:?}")));
        }

        let (added_again, skipped_again) = write_keybindings(&path).unwrap();
        assert_eq!(added_again, 0);
        assert_eq!(skipped_again, RECOMMENDED_KEYS.len());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn setup_preserves_config_and_skips_conflicting_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let herdr_dir = dir.path().join("herdr");
        std::fs::create_dir_all(&herdr_dir).unwrap();
        let path = herdr_dir.join("config.toml");
        std::fs::write(
            &path,
            r#"
# existing user config
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr.scratch.toggle"
description = "mine"

[[keys.command]]
key = "prefix+z"
type = "builtin"
command = "unrelated"
"#,
        )
        .unwrap();

        let (added, skipped) = write_keybindings(&path).unwrap();
        assert_eq!(added, RECOMMENDED_KEYS.len() - 1);
        assert_eq!(skipped, 1);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("\n# existing user config"));
        assert!(content.contains("# Added by herdr-scratch setup"));
        assert_eq!(content.matches("key = \"prefix+p\"").count(), 1);
    }

    #[test]
    fn setup_reserves_keys_used_by_goto_and_other_key_tables() {
        let dir = tempfile::tempdir().unwrap();
        let herdr_dir = dir.path().join("herdr");
        std::fs::create_dir_all(&herdr_dir).unwrap();
        let path = herdr_dir.join("config.toml");
        std::fs::write(
            &path,
            r#"
# user uses prefix+shift+g for the goto overlay
[[keys.goto]]
key = "prefix+shift+g"

[[keys.session]]
key = "prefix+n"
command = "attach"
"#,
        )
        .unwrap();

        let (added, skipped) = write_keybindings(&path).unwrap();
        assert_eq!(added, RECOMMENDED_KEYS.len() - 2);
        assert_eq!(skipped, 2);

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("key = \"prefix+shift+g\"").count(), 1);
        assert_eq!(content.matches("key = \"prefix+n\"").count(), 1);
    }

    #[test]
    fn setup_appends_cleanly_to_a_file_without_a_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let herdr_dir = dir.path().join("herdr");
        std::fs::create_dir_all(&herdr_dir).unwrap();
        let path = herdr_dir.join("config.toml");
        std::fs::write(&path, "[keys]").unwrap();

        let (added, _) = write_keybindings(&path).unwrap();
        assert_eq!(added, RECOMMENDED_KEYS.len());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("[keys]\n"));
        assert!(content.contains("# Added by herdr-scratch setup"));
    }

    #[test]
    fn setup_rejects_malformed_config_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let herdr_dir = dir.path().join("herdr");
        std::fs::create_dir_all(&herdr_dir).unwrap();
        let path = herdr_dir.join("config.toml");
        std::fs::write(&path, "not a valid toml [[").unwrap();

        assert!(write_keybindings(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not a valid toml [["
        );
    }

    #[test]
    fn popup_busy_show_maps_to_a_friendly_hint() {
        let fake = FakeHerdr {
            popup_busy: true,
            ..FakeHerdr::default()
        };
        let (_dir, mut app) = app_with_fake(fake);

        let err = app.toggle(None, None).unwrap_err();

        assert!(
            err.to_string()
                .contains("another popup is already open; detach it with ctrl+b q")
        );
        let record = app.registry.scratchpads.values().next().unwrap();
        assert_eq!(record.status, LifecycleStatus::Available);
    }

    #[test]
    fn non_popup_show_errors_propagate_unchanged() {
        let fake = FakeHerdr {
            fail_show: true,
            ..FakeHerdr::default()
        };
        let (_dir, mut app) = app_with_fake(fake);

        let err = app.toggle(None, None).unwrap_err();

        assert!(err.to_string().contains("popup already open"));
        assert!(!err.to_string().contains("ctrl+b"));
    }

    #[test]
    fn doctor_reports_server_and_keybinding_status() {
        let fake = FakeHerdr::default();
        let (_dir, app) = app_with_fake(fake);
        let config_dir = tempfile::tempdir().unwrap();
        let config_path = config_dir.path().join("config.toml");
        std::fs::write(&config_path, "").unwrap();
        unsafe {
            std::env::set_var("HERDR_CONFIG_FILE", &config_path);
        }
        let report = app.doctor();
        unsafe {
            std::env::remove_var("HERDR_CONFIG_FILE");
        }

        assert!(report.server_ok);
        assert_eq!(report.keybinding_missing, RECOMMENDED_KEYS.len());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("herdr-scratch setup"))
        );
    }

    #[test]
    fn summary_reports_surface_and_status() {
        let fake = FakeHerdr::default();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(None, None).unwrap();
        let key = app.registry.scratchpads.keys().next().unwrap().clone();

        let record = app.registry.scratchpads.get(&key).unwrap().clone();
        let summary = app.summary_for_key(&key, &record);
        assert_eq!(summary.surface.as_deref(), Some("popup"));
        assert_eq!(summary.size, None);
        assert_eq!(summary.status, "visible");

        // Once the viewer pane is gone the summary reports the scratchpad as
        // available rather than visible.
        let fake2 = FakeHerdr {
            handle_gone: true,
            ..FakeHerdr::default()
        };
        let (_dir2, mut app2) = app_with_fake(fake2);
        app2.toggle(None, None).unwrap();
        let key2 = app2.registry.scratchpads.keys().next().unwrap().clone();
        let record = app2.registry.scratchpads.get(&key2).unwrap().clone();
        let summary = app2.summary_for_key(&key2, &record);
        assert_eq!(summary.status, "available");
    }

    fn daily_config(vault: Option<&std::path::Path>) -> Config {
        let mut config = Config::default();
        config.notes.vault_path = vault.map(|v| v.display().to_string());
        config.notes.vault_auto = false;
        config.notes.editor = Some("nvim".to_string());
        config
    }

    fn daily_args(date: &str) -> cli::DailyArgs {
        cli::DailyArgs {
            vault: None,
            print_path: false,
            date: Some(date.to_string()),
        }
    }

    #[test]
    fn daily_opens_vault_note_with_editor_and_template_defaults() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join(".obsidian")).unwrap();
        std::fs::write(
            vault.path().join(".obsidian").join("daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD"}"#,
        )
        .unwrap();
        let (_dir, mut app) =
            app_with_config(FakeHerdr::default(), daily_config(Some(vault.path())));

        let output = app.daily(&daily_args("2026-09-20")).unwrap();

        let expected = vault.path().join("Daily").join("2026-09-20.md");
        assert!(expected.is_file());
        let content = std::fs::read_to_string(&expected).unwrap();
        assert!(content.contains("# 2026-09-20"));
        assert!(
            matches!(output, Output::Text(text) if text.contains("2026-09-20") && text.contains("explicit"))
        );
        let record = app
            .registry
            .scratchpads
            .get("global:default:daily")
            .expect("daily record");
        assert_eq!(
            record.launch_command.as_deref(),
            Some(vec!["nvim".to_string(), expected.display().to_string()].as_slice())
        );
        assert_eq!(record.scope.kind, "global");
    }

    #[test]
    fn daily_honors_nested_moment_format() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join(".obsidian")).unwrap();
        std::fs::write(
            vault.path().join(".obsidian").join("daily-notes.json"),
            r#"{"folder":"Journal","format":"YYYY/MM/YYYY-MM-DD"}"#,
        )
        .unwrap();
        let (_dir, mut app) =
            app_with_config(FakeHerdr::default(), daily_config(Some(vault.path())));

        app.daily(&daily_args("2026-01-05")).unwrap();

        assert!(
            vault
                .path()
                .join("Journal")
                .join("2026")
                .join("01")
                .join("2026-01-05.md")
                .is_file()
        );
    }

    #[test]
    fn daily_falls_back_to_local_file_without_a_vault() {
        let (_dir, mut app) = app_with_config(FakeHerdr::default(), daily_config(None));

        let output = app.daily(&daily_args("2026-09-20")).unwrap();

        assert!(matches!(output, Output::Text(text) if text.contains("fallback")));
        let record = app
            .registry
            .scratchpads
            .get("global:default:daily")
            .expect("daily record");
        let file = record.launch_command.as_ref().unwrap()[1].clone();
        assert!(std::path::Path::new(&file).is_file());
        assert!(file.ends_with("2026-09-20.md"));
    }

    #[test]
    fn daily_print_path_resolves_without_opening_a_runtime() {
        let vault = tempfile::tempdir().unwrap();
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_config(fake, daily_config(Some(vault.path())));
        let mut args = daily_args("2026-09-20");
        args.print_path = true;

        let output = app.daily(&args).unwrap();

        assert!(calls.borrow().is_empty());
        assert!(app.registry.scratchpads.is_empty());
        let expected = vault.path().join("2026-09-20.md").display().to_string();
        assert!(matches!(output, Output::Text(text) if text == expected));
    }

    #[test]
    fn daily_reuses_same_day_and_rolls_over_to_a_new_file() {
        let vault = tempfile::tempdir().unwrap();
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_config(fake, daily_config(Some(vault.path())));

        app.daily(&daily_args("2026-09-20")).unwrap();
        let opens = || {
            calls
                .borrow()
                .iter()
                .filter(|c| c.starts_with("open:"))
                .count()
        };
        assert_eq!(opens(), 1);

        // Same day: no new backing runtime, viewer just refocused.
        app.daily(&daily_args("2026-09-20")).unwrap();
        assert_eq!(opens(), 1);

        // Next day: old runtime closed, new file tracked.
        app.daily(&daily_args("2026-09-21")).unwrap();
        assert_eq!(opens(), 2);
        assert!(calls.borrow().contains(&"close".to_string()));
        assert!(vault.path().join("2026-09-21.md").is_file());
        let record = app
            .registry
            .scratchpads
            .get("global:default:daily")
            .unwrap();
        assert!(record.launch_command.as_ref().unwrap()[1].ends_with("2026-09-21.md"));
    }

    #[test]
    fn daily_rejects_a_missing_explicit_vault() {
        let mut config = Config::default();
        config.notes.vault_path = Some("/does/not/exist-vault".to_string());
        config.notes.vault_auto = false;
        let (_dir, mut app) = app_with_config(FakeHerdr::default(), config);

        let err = app.daily(&daily_args("2026-09-20")).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn daily_cli_vault_override_wins_over_config() {
        let vault_a = tempfile::tempdir().unwrap();
        let vault_b = tempfile::tempdir().unwrap();
        let (_dir, mut app) =
            app_with_config(FakeHerdr::default(), daily_config(Some(vault_a.path())));
        let args = cli::DailyArgs {
            vault: Some(vault_b.path().display().to_string()),
            print_path: true,
            date: Some("2026-09-20".to_string()),
        };

        let output = app.daily(&args).unwrap();

        let expected = vault_b.path().join("2026-09-20.md").display().to_string();
        assert!(matches!(output, Output::Text(text) if text == expected));
    }

    #[test]
    fn parse_daily_date_accepts_iso_and_rejects_garbage() {
        assert!(parse_daily_date(Some("2026-09-20")).is_ok());
        assert!(parse_daily_date(Some("not-a-date")).is_err());
        assert!(parse_daily_date(Some("2026-13-01")).is_err());
        assert!(parse_daily_date(None).is_ok());
    }

    #[test]
    fn viewer_pane_receives_the_scratchpad_title_on_open() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        app.toggle(Some("notes"), None).unwrap();

        assert!(
            calls
                .borrow()
                .contains(&format!("rename:{FAKE_VIEWER_PANE}:Scratchpad:notes"))
        );
    }

    #[test]
    fn one_shot_command_on_default_scratchpad_shows_command_basename() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        app.toggle(None, Some(vec!["lazygit".to_string()])).unwrap();

        assert!(
            calls
                .borrow()
                .contains(&format!("rename:{FAKE_VIEWER_PANE}:Scratchpad:lazygit"))
        );
        assert!(
            !calls
                .borrow()
                .iter()
                .any(|call| call.contains("Scratchpad:scratch"))
        );
    }

    #[test]
    fn named_scratchpad_keeps_its_name_despite_command_override() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);

        app.toggle(Some("git"), Some(vec!["/usr/bin/lazygit".to_string()]))
            .unwrap();

        assert!(
            calls
                .borrow()
                .contains(&format!("rename:{FAKE_VIEWER_PANE}:Scratchpad:git"))
        );
    }

    #[test]
    fn focus_renames_a_stale_viewer_title() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_fake(fake);
        app.toggle(Some("notes"), None).unwrap();
        calls.borrow_mut().clear();

        app.focus(Some("notes")).unwrap();

        assert!(
            calls
                .borrow()
                .contains(&format!("rename:{FAKE_VIEWER_PANE}:Scratchpad:notes"))
        );
    }

    #[test]
    fn daily_viewer_shows_the_dated_title() {
        let fake = FakeHerdr::default();
        let calls = fake.calls.clone();
        let (_dir, mut app) = app_with_config(fake, daily_config(None));

        app.daily(&daily_args("2026-09-20")).unwrap();

        assert!(
            calls
                .borrow()
                .contains(&format!("rename:{FAKE_VIEWER_PANE}:daily:2026-09-20"))
        );
    }

    #[test]
    fn display_title_falls_back_to_name_for_empty_commands() {
        let (_dir, app) = app_with_fake(FakeHerdr::default());
        assert_eq!(
            app.display_title("scratch", Some(&[])),
            "Scratchpad:scratch"
        );
        assert_eq!(
            app.display_title("scratch", Some(&["  ".to_string()])),
            "Scratchpad:scratch"
        );
        assert_eq!(
            app.display_title("notes", Some(&["lazygit".to_string()])),
            "Scratchpad:notes"
        );
    }
}
