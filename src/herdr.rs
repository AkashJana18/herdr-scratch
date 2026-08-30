use std::{
    collections::BTreeMap,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    config::{PopupConfig, PopupSize, ScratchpadPlacement, SplitDirection},
    registry::{FocusSnapshot, RuntimeHandle},
};

#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

pub const PLUGIN_ID: &str = "herdr.scratch";
const RUNTIME_ENTRYPOINT: &str = "scratch";
const POPUP_ENTRYPOINT: &str = "popup";
const GUIDE_ENTRYPOINT: &str = "guide";

pub trait Herdr {
    fn available(&self) -> bool;
    fn version(&self) -> Option<String>;
    fn current_pane(&self) -> Result<PaneInfo, HerdrError>;
    fn open_guide(&self) -> Result<(), HerdrError>;
    fn tab_get(&self, tab_id: &str) -> Result<TabInfo, HerdrError>;
    fn handle_get(&self, handle: &RuntimeHandle) -> Result<PaneInfo, HerdrError>;
    fn show_handle(
        &self,
        handle: &RuntimeHandle,
        registry_key: &str,
        popup: &PopupConfig,
        size: Option<&PopupSize>,
    ) -> Result<(), HerdrError>;
    fn hide_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError>;
    fn focus_previous(&self, previous: &FocusSnapshot) -> Result<(), HerdrError>;
    fn open_scratchpad(&self, request: OpenScratchpadRequest) -> Result<RuntimeHandle, HerdrError>;
    fn rename_handle(&self, handle: &RuntimeHandle, title: &str) -> Result<(), HerdrError>;
    fn close_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError>;
    fn send_text(&self, handle: &RuntimeHandle, text: &str) -> Result<(), HerdrError>;
    fn run_command(&self, handle: &RuntimeHandle, command: &str) -> Result<(), HerdrError>;
}

#[derive(Debug, Clone)]
pub struct HerdrCli {
    bin: String,
}

impl HerdrCli {
    pub fn discover() -> Self {
        let bin = std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string());
        Self { bin }
    }

    fn run(&self, args: &[String]) -> Result<serde_json::Value, HerdrError> {
        self.run_for(None, args)
    }

    fn run_for(
        &self,
        session: Option<&str>,
        args: &[String],
    ) -> Result<serde_json::Value, HerdrError> {
        let stdout = self.run_raw_for(session, args)?;
        serde_json::from_str(&stdout).map_err(|source| HerdrError::InvalidJson { stdout, source })
    }

    fn run_raw_for(&self, session: Option<&str>, args: &[String]) -> Result<String, HerdrError> {
        let command_args = target_args(session, args);
        let output = Command::new(&self.bin)
            .args(&command_args)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| HerdrError::CommandSpawn {
                binary: self.bin.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(HerdrError::CommandFailed {
                command: format!("{} {}", self.bin, command_args.join(" ")),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        String::from_utf8(output.stdout).map_err(HerdrError::InvalidUtf8)
    }

    fn run_ok(&self, args: &[String]) -> Result<(), HerdrError> {
        self.run_ok_for(None, args)
    }

    fn run_ok_for(&self, session: Option<&str>, args: &[String]) -> Result<(), HerdrError> {
        let stdout = self.run_raw_for(session, args)?;
        if stdout.trim().is_empty() {
            return Ok(());
        }
        let value: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|source| HerdrError::InvalidJson { stdout, source })?;
        if value.get("error").is_some() {
            return Err(HerdrError::Api(value));
        }
        Ok(())
    }

    fn ensure_server(&self, session: &str) -> Result<(), HerdrError> {
        // `herdr status server` intentionally exits zero even when it prints
        // `status: not running`, so use a real socket-backed read as the probe.
        let probe_args = vec!["workspace".to_string(), "list".to_string()];
        if self.run_raw_for(Some(session), &probe_args).is_ok() {
            return Ok(());
        }

        let mut command = Command::new(&self.bin);
        command
            .args(["--session", session, "server"])
            .env_remove("HERDR_SOCKET_PATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn().map_err(|source| HerdrError::CommandSpawn {
            binary: self.bin.clone(),
            source,
        })?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.run_raw_for(Some(session), &probe_args).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(HerdrError::Unsupported(format!(
            "timed out starting backing Herdr session `{session}`"
        )))
    }

    pub fn attach_terminal(&self, session: &str, terminal_id: &str) -> Result<(), HerdrError> {
        let status = Command::new(&self.bin)
            .args(["--session", session, "terminal", "attach", terminal_id])
            .env_remove("HERDR_SOCKET_PATH")
            .status()
            .map_err(|source| HerdrError::CommandSpawn {
                binary: self.bin.clone(),
                source,
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(HerdrError::CommandFailed {
                command: format!(
                    "{} --session {} terminal attach {}",
                    self.bin, session, terminal_id
                ),
                status: status.code(),
                stderr: String::new(),
            })
        }
    }

    fn focus_pane_by_id(&self, pane_id: &str) -> Result<(), HerdrError> {
        #[cfg(unix)]
        {
            let socket_path = std::env::var_os("HERDR_SOCKET_PATH")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    HerdrError::Unsupported(
                        "HERDR_SOCKET_PATH is not set; cannot focus an exact pane".to_string(),
                    )
                })?;
            let request = serde_json::json!({
                "id": "herdr-scratch:pane-focus",
                "method": "pane.focus",
                "params": {
                    "pane_id": pane_id,
                },
            });
            let response = socket_request(&socket_path, &request)?;
            parse_result(response)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = pane_id;
            Err(HerdrError::Unsupported(
                "exact pane focus is only implemented for Unix sockets".to_string(),
            ))
        }
    }

    fn open_popup_runtime(
        &self,
        request: OpenScratchpadRequest,
    ) -> Result<RuntimeHandle, HerdrError> {
        let session = request.backing_session.clone();
        self.ensure_server(&session)?;

        let mut workspace_args = vec![
            "workspace".to_string(),
            "create".to_string(),
            "--label".to_string(),
            request.title.clone(),
            "--no-focus".to_string(),
        ];
        if let Some(cwd) = request.cwd.as_ref() {
            workspace_args.push("--cwd".to_string());
            workspace_args.push(cwd.clone());
        }
        let workspace_value = self.run_for(Some(&session), &workspace_args)?;
        let workspace = parse_workspace_created(workspace_value)?;

        let result = (|| {
            let mut pane_args = vec![
                "plugin".to_string(),
                "pane".to_string(),
                "open".to_string(),
                "--plugin".to_string(),
                PLUGIN_ID.to_string(),
                "--entrypoint".to_string(),
                RUNTIME_ENTRYPOINT.to_string(),
                "--placement".to_string(),
                "tab".to_string(),
                "--workspace".to_string(),
                workspace.workspace_id.clone(),
                "--no-focus".to_string(),
            ];
            if let Some(cwd) = request.cwd.as_ref() {
                pane_args.push("--cwd".to_string());
                pane_args.push(cwd.clone());
            }
            for (key, value) in &request.env {
                pane_args.push("--env".to_string());
                pane_args.push(format!("{key}={value}"));
            }
            let pane_value = self.run_for(Some(&session), &pane_args)?;
            let pane = parse_plugin_pane_opened(pane_value)?;

            self.run_ok_for(
                Some(&session),
                &["tab".into(), "close".into(), workspace.root_tab_id.clone()],
            )?;

            Ok(RuntimeHandle {
                kind: "herdr_popup".to_string(),
                pane_id: Some(pane.pane_id),
                terminal_id: Some(pane.terminal_id),
                workspace_id: Some(pane.workspace_id),
                session: Some(session.clone()),
                opaque: BTreeMap::from([
                    (
                        "focus_token".to_string(),
                        serde_json::Value::String(pane.tab_id),
                    ),
                    (
                        "surface".to_string(),
                        serde_json::Value::String("popup".to_string()),
                    ),
                ]),
            })
        })();

        if result.is_err() {
            let _ = self.run_ok_for(
                Some(&session),
                &["workspace".into(), "close".into(), workspace.workspace_id],
            );
        }
        result
    }
}

impl Herdr for HerdrCli {
    fn available(&self) -> bool {
        Command::new(&self.bin)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn version(&self) -> Option<String> {
        Command::new(&self.bin)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|output| output.trim().to_string())
    }

    fn current_pane(&self) -> Result<PaneInfo, HerdrError> {
        let value = self.run(&["pane".into(), "current".into()])?;
        parse_pane_result(value)
    }

    fn open_guide(&self) -> Result<(), HerdrError> {
        self.run_ok(&[
            "plugin".into(),
            "pane".into(),
            "open".into(),
            "--plugin".into(),
            PLUGIN_ID.into(),
            "--entrypoint".into(),
            GUIDE_ENTRYPOINT.into(),
            "--placement".into(),
            "popup".into(),
            "--width".into(),
            "70%".into(),
            "--height".into(),
            "60%".into(),
        ])
    }

    fn tab_get(&self, tab_id: &str) -> Result<TabInfo, HerdrError> {
        let value = self.run(&["tab".into(), "get".into(), tab_id.into()])?;
        parse_tab_result(value)
    }

    fn handle_get(&self, handle: &RuntimeHandle) -> Result<PaneInfo, HerdrError> {
        if let Some(session) = handle.session.as_deref() {
            self.ensure_server(session)?;
        }
        let pane_id = handle
            .pane_id
            .as_deref()
            .ok_or(HerdrError::MissingHandle("pane_id"))?;
        let value = self.run_for(
            handle.session.as_deref(),
            &["pane".into(), "get".into(), pane_id.into()],
        )?;
        parse_pane_result(value)
    }

    fn show_handle(
        &self,
        handle: &RuntimeHandle,
        registry_key: &str,
        popup: &PopupConfig,
        size: Option<&PopupSize>,
    ) -> Result<(), HerdrError> {
        if handle.is_popup() {
            let session = handle
                .session
                .as_deref()
                .ok_or(HerdrError::MissingHandle("session"))?;
            let terminal_id = handle
                .terminal_id
                .as_deref()
                .ok_or(HerdrError::MissingHandle("terminal_id"))?;
            let (width, height) = match size {
                Some(size) => size.to_arg_pairs(),
                None => (popup.width.as_arg(), popup.height.as_arg()),
            };
            let args = vec![
                "plugin".to_string(),
                "pane".to_string(),
                "open".to_string(),
                "--plugin".to_string(),
                PLUGIN_ID.to_string(),
                "--entrypoint".to_string(),
                POPUP_ENTRYPOINT.to_string(),
                "--placement".to_string(),
                "popup".to_string(),
                "--width".to_string(),
                width,
                "--height".to_string(),
                height,
                "--env".to_string(),
                format!("HERDR_SCRATCH_BACKING_SESSION={session}"),
                "--env".to_string(),
                format!("HERDR_SCRATCH_TERMINAL_ID={terminal_id}"),
                "--env".to_string(),
                format!("HERDR_SCRATCH_REGISTRY_KEY={registry_key}"),
            ];
            return self.run_ok(&args);
        }

        if let Some(focus_token) = handle.focus_token() {
            self.run_ok(&["tab".into(), "focus".into(), focus_token.into()])?;
        }
        if let Some(pane_id) = handle.pane_id.as_deref() {
            self.run_ok(&[
                "plugin".into(),
                "pane".into(),
                "focus".into(),
                pane_id.into(),
            ])?;
        }
        Ok(())
    }

    fn hide_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError> {
        if !handle.is_popup() {
            return Ok(());
        }
        #[cfg(unix)]
        {
            let socket_path = std::env::var_os("HERDR_SOCKET_PATH")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    HerdrError::Unsupported(
                        "HERDR_SOCKET_PATH is not set; cannot close the active popup".to_string(),
                    )
                })?;
            let response = socket_request(
                &socket_path,
                &serde_json::json!({
                    "id": "herdr-scratch:popup-close",
                    "method": "popup.close",
                    "params": {},
                }),
            )?;
            parse_result(response)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(HerdrError::Unsupported(
                "popup close is only implemented for Unix sockets".to_string(),
            ))
        }
    }

    fn focus_previous(&self, previous: &FocusSnapshot) -> Result<(), HerdrError> {
        if let Some(pane_id) = previous.pane_id.as_deref()
            && self.focus_pane_by_id(pane_id).is_ok()
        {
            return Ok(());
        }
        if let Some(focus_token) = previous.focus_token.as_deref() {
            self.run_ok(&["tab".into(), "focus".into(), focus_token.into()])?;
            return Ok(());
        }
        Err(HerdrError::Unsupported(
            "previous context does not include a focusable tab".to_string(),
        ))
    }

    fn open_scratchpad(&self, request: OpenScratchpadRequest) -> Result<RuntimeHandle, HerdrError> {
        if request.placement == ScratchpadPlacement::Popup {
            return self.open_popup_runtime(request);
        }
        let mut args = vec![
            "plugin".to_string(),
            "pane".to_string(),
            "open".to_string(),
            "--plugin".to_string(),
            PLUGIN_ID.to_string(),
            "--entrypoint".to_string(),
            RUNTIME_ENTRYPOINT.to_string(),
            "--placement".to_string(),
            request.placement.as_str().to_string(),
            "--focus".to_string(),
        ];
        if request.placement == ScratchpadPlacement::Split {
            args.push("--direction".to_string());
            args.push(request.split_direction.as_str().to_string());
        }
        if let Some(cwd) = request.cwd {
            args.push("--cwd".to_string());
            args.push(cwd);
        }
        for (key, value) in request.env {
            args.push("--env".to_string());
            args.push(format!("{key}={value}"));
        }
        let value = self.run(&args)?;
        let pane = parse_plugin_pane_opened(value)?;
        Ok(RuntimeHandle {
            kind: "herdr".to_string(),
            pane_id: Some(pane.pane_id),
            terminal_id: Some(pane.terminal_id),
            workspace_id: Some(pane.workspace_id),
            session: None,
            opaque: BTreeMap::from([
                (
                    "focus_token".to_string(),
                    serde_json::Value::String(pane.tab_id),
                ),
                (
                    "surface".to_string(),
                    serde_json::Value::String(request.placement.as_str().to_string()),
                ),
            ]),
        })
    }

    fn rename_handle(&self, handle: &RuntimeHandle, title: &str) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok_for(
            handle.session.as_deref(),
            &["pane".into(), "rename".into(), pane_id.into(), title.into()],
        )
    }

    fn close_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok_for(
            handle.session.as_deref(),
            &[
                "plugin".into(),
                "pane".into(),
                "close".into(),
                pane_id.into(),
            ],
        )
    }

    fn send_text(&self, handle: &RuntimeHandle, text: &str) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok_for(
            handle.session.as_deref(),
            &[
                "pane".into(),
                "send-text".into(),
                pane_id.into(),
                text.into(),
            ],
        )
    }

    fn run_command(&self, handle: &RuntimeHandle, command: &str) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok_for(
            handle.session.as_deref(),
            &["pane".into(), "run".into(), pane_id.into(), command.into()],
        )
    }
}

#[derive(Debug, Clone)]
pub struct OpenScratchpadRequest {
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub title: String,
    pub backing_session: String,
    pub placement: ScratchpadPlacement,
    pub split_direction: SplitDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub terminal_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Clone)]
struct WorkspaceCreated {
    workspace_id: String,
    root_tab_id: String,
}

#[derive(Debug, Error)]
pub enum HerdrError {
    #[error("failed to start `{binary}`: {source}")]
    CommandSpawn {
        binary: String,
        source: std::io::Error,
    },
    #[error("Herdr command failed ({status:?}): {command}: {stderr}")]
    CommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    #[error("Herdr output was not UTF-8: {0}")]
    InvalidUtf8(std::string::FromUtf8Error),
    #[error("Herdr output was not JSON: {source}; stdout: {stdout}")]
    InvalidJson {
        stdout: String,
        source: serde_json::Error,
    },
    #[error("Herdr API returned an error: {0}")]
    Api(serde_json::Value),
    #[error("Herdr socket request failed: {0}")]
    Socket(std::io::Error),
    #[error("Herdr response did not contain {0}")]
    MissingField(&'static str),
    #[error("runtime handle is missing {0}")]
    MissingHandle(&'static str),
    #[error("{0}")]
    Unsupported(String),
}

#[cfg(unix)]
fn socket_request(
    socket_path: &PathBuf,
    request: &serde_json::Value,
) -> Result<serde_json::Value, HerdrError> {
    let mut stream = UnixStream::connect(socket_path).map_err(HerdrError::Socket)?;
    stream
        .write_all(request.to_string().as_bytes())
        .map_err(HerdrError::Socket)?;
    stream.write_all(b"\n").map_err(HerdrError::Socket)?;
    stream.flush().map_err(HerdrError::Socket)?;

    let mut line = String::new();
    let mut reader = BufReader::new(stream);
    let read = reader.read_line(&mut line).map_err(HerdrError::Socket)?;
    if read == 0 || line.trim().is_empty() {
        return Err(HerdrError::Unsupported(
            "Herdr socket returned an empty response".to_string(),
        ));
    }
    serde_json::from_str(&line).map_err(|source| HerdrError::InvalidJson {
        stdout: line,
        source,
    })
}

fn parse_result(value: serde_json::Value) -> Result<serde_json::Value, HerdrError> {
    if value.get("error").is_some() {
        return Err(HerdrError::Api(value));
    }
    value
        .get("result")
        .cloned()
        .ok_or(HerdrError::MissingField("result"))
}

fn parse_pane_result(value: serde_json::Value) -> Result<PaneInfo, HerdrError> {
    let result = parse_result(value)?;
    let pane = result
        .get("pane")
        .cloned()
        .ok_or(HerdrError::MissingField("result.pane"))?;
    serde_json::from_value(pane).map_err(|source| HerdrError::InvalidJson {
        stdout: "result.pane".to_string(),
        source,
    })
}

fn parse_tab_result(value: serde_json::Value) -> Result<TabInfo, HerdrError> {
    let result = parse_result(value)?;
    let tab = result
        .get("tab")
        .cloned()
        .ok_or(HerdrError::MissingField("result.tab"))?;
    serde_json::from_value(tab).map_err(|source| HerdrError::InvalidJson {
        stdout: "result.tab".to_string(),
        source,
    })
}

fn parse_plugin_pane_opened(value: serde_json::Value) -> Result<PaneInfo, HerdrError> {
    let result = parse_result(value)?;
    let pane = result
        .get("plugin_pane")
        .and_then(|plugin_pane| plugin_pane.get("pane"))
        .cloned()
        .ok_or(HerdrError::MissingField("result.plugin_pane.pane"))?;
    serde_json::from_value(pane).map_err(|source| HerdrError::InvalidJson {
        stdout: "result.plugin_pane.pane".to_string(),
        source,
    })
}

fn parse_workspace_created(value: serde_json::Value) -> Result<WorkspaceCreated, HerdrError> {
    let result = parse_result(value)?;
    let workspace_id = result
        .get("workspace")
        .and_then(|workspace| workspace.get("workspace_id"))
        .and_then(serde_json::Value::as_str)
        .ok_or(HerdrError::MissingField("result.workspace.workspace_id"))?;
    let root_tab_id = result
        .get("tab")
        .and_then(|tab| tab.get("tab_id"))
        .and_then(serde_json::Value::as_str)
        .ok_or(HerdrError::MissingField("result.tab.tab_id"))?;
    Ok(WorkspaceCreated {
        workspace_id: workspace_id.to_string(),
        root_tab_id: root_tab_id.to_string(),
    })
}

fn target_args(session: Option<&str>, args: &[String]) -> Vec<String> {
    let mut result = Vec::with_capacity(args.len() + usize::from(session.is_some()) * 2);
    if let Some(session) = session {
        result.push("--session".to_string());
        result.push(session.to_string());
    }
    result.extend_from_slice(args);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plugin_pane_open_response() {
        let value = serde_json::json!({
            "id": "req",
            "result": {
                "type": "plugin_pane_opened",
                "plugin_pane": {
                    "plugin_id": "herdr.scratch",
                    "entrypoint": "scratch",
                    "pane": {
                        "pane_id": "w1:p2",
                        "terminal_id": "term-2",
                        "workspace_id": "w1",
                        "tab_id": "w1:t2",
                        "focused": true
                    }
                }
            }
        });
        let pane = parse_plugin_pane_opened(value).unwrap();
        assert_eq!(pane.pane_id, "w1:p2");
        assert_eq!(pane.terminal_id, "term-2");
        assert_eq!(pane.tab_id, "w1:t2");
    }

    #[test]
    fn qualifies_commands_with_named_session() {
        assert_eq!(
            target_args(
                Some("herdr-scratch"),
                &["pane".into(), "get".into(), "w1:p1".into()]
            ),
            vec!["--session", "herdr-scratch", "pane", "get", "w1:p1"]
        );
    }

    #[test]
    fn parses_workspace_creation_response() {
        let value = serde_json::json!({
            "result": {
                "workspace": { "workspace_id": "w7" },
                "tab": { "tab_id": "w7:t1" },
                "root_pane": { "pane_id": "w7:p1" }
            }
        });
        let created = parse_workspace_created(value).unwrap();
        assert_eq!(created.workspace_id, "w7");
        assert_eq!(created.root_tab_id, "w7:t1");
    }
}
