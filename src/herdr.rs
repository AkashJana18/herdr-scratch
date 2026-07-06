use std::{
    collections::BTreeMap,
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    config::{ScratchpadPlacement, SplitDirection},
    registry::{FocusSnapshot, RuntimeHandle},
};

#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

pub const PLUGIN_ID: &str = "herdr.scratch";
const ENTRYPOINT: &str = "scratch";

pub trait Herdr {
    fn available(&self) -> bool;
    fn current_pane(&self) -> Result<PaneInfo, HerdrError>;
    fn pane_get(&self, pane_id: &str) -> Result<PaneInfo, HerdrError>;
    fn tab_get(&self, tab_id: &str) -> Result<TabInfo, HerdrError>;
    fn focus_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError>;
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
        let stdout = self.run_raw(args)?;
        serde_json::from_str(&stdout).map_err(|source| HerdrError::InvalidJson { stdout, source })
    }

    fn run_raw(&self, args: &[String]) -> Result<String, HerdrError> {
        let output = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| HerdrError::CommandSpawn {
                binary: self.bin.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(HerdrError::CommandFailed {
                command: format!("{} {}", self.bin, args.join(" ")),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        String::from_utf8(output.stdout).map_err(HerdrError::InvalidUtf8)
    }

    fn run_ok(&self, args: &[String]) -> Result<(), HerdrError> {
        let stdout = self.run_raw(args)?;
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

    fn current_pane(&self) -> Result<PaneInfo, HerdrError> {
        let value = self.run(&["pane".into(), "current".into()])?;
        parse_pane_result(value)
    }

    fn pane_get(&self, pane_id: &str) -> Result<PaneInfo, HerdrError> {
        let value = self.run(&["pane".into(), "get".into(), pane_id.into()])?;
        parse_pane_result(value)
    }

    fn tab_get(&self, tab_id: &str) -> Result<TabInfo, HerdrError> {
        let value = self.run(&["tab".into(), "get".into(), tab_id.into()])?;
        parse_tab_result(value)
    }

    fn focus_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError> {
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

    fn focus_previous(&self, previous: &FocusSnapshot) -> Result<(), HerdrError> {
        if let Some(pane_id) = previous.pane_id.as_deref() {
            if self.focus_pane_by_id(pane_id).is_ok() {
                return Ok(());
            }
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
        let mut args = vec![
            "plugin".to_string(),
            "pane".to_string(),
            "open".to_string(),
            "--plugin".to_string(),
            PLUGIN_ID.to_string(),
            "--entrypoint".to_string(),
            ENTRYPOINT.to_string(),
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
            workspace_id: Some(pane.workspace_id),
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
        self.run_ok(&["pane".into(), "rename".into(), pane_id.into(), title.into()])
    }

    fn close_handle(&self, handle: &RuntimeHandle) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok(&[
            "plugin".into(),
            "pane".into(),
            "close".into(),
            pane_id.into(),
        ])
    }

    fn send_text(&self, handle: &RuntimeHandle, text: &str) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok(&[
            "pane".into(),
            "send-text".into(),
            pane_id.into(),
            text.into(),
        ])
    }

    fn run_command(&self, handle: &RuntimeHandle, command: &str) -> Result<(), HerdrError> {
        let Some(pane_id) = handle.pane_id.as_deref() else {
            return Err(HerdrError::MissingHandle("pane_id"));
        };
        self.run_ok(&["pane".into(), "run".into(), pane_id.into(), command.into()])
    }
}

#[derive(Debug, Clone)]
pub struct OpenScratchpadRequest {
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub placement: ScratchpadPlacement,
    pub split_direction: SplitDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
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
                        "workspace_id": "w1",
                        "tab_id": "w1:t2",
                        "focused": true
                    }
                }
            }
        });
        let pane = parse_plugin_pane_opened(value).unwrap();
        assert_eq!(pane.pane_id, "w1:p2");
        assert_eq!(pane.tab_id, "w1:t2");
    }
}
