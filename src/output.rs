use anyhow::Context;
use serde::Serialize;

use crate::scratchpad::{DoctorReport, ScratchpadSummary};

#[derive(Debug)]
pub enum Output {
    Text(String),
    Json(serde_json::Value),
    Scratchpads {
        scratchpads: Vec<ScratchpadSummary>,
        json: bool,
    },
    Doctor {
        report: DoctorReport,
        json: bool,
    },
}

pub fn print(output: Output) -> anyhow::Result<()> {
    match output {
        Output::Text(text) => {
            println!("{text}");
            Ok(())
        }
        Output::Json(value) => print_json(&value),
        Output::Scratchpads { scratchpads, json } => {
            if json {
                return print_json(&scratchpads);
            }
            if scratchpads.is_empty() {
                println!("no scratchpads");
                return Ok(());
            }
            for item in scratchpads {
                println!(
                    "{}\t{}\t{}\t{}",
                    item.name,
                    item.status,
                    item.scope,
                    item.cwd.unwrap_or_else(|| "-".to_string())
                );
            }
            Ok(())
        }
        Output::Doctor { report, json } => {
            if json {
                return print_json(&report);
            }
            println!("{}", format_doctor(&report));
            Ok(())
        }
    }
}

fn print_json(value: &impl Serialize) -> anyhow::Result<()> {
    let encoded = serde_json::to_string_pretty(value).context("failed to encode JSON output")?;
    println!("{encoded}");
    Ok(())
}

fn status_word(ok: bool) -> &'static str {
    if ok { "ok" } else { "unavailable" }
}

fn format_doctor(report: &DoctorReport) -> String {
    let mut lines = vec![
        format!("herdr: {}", status_word(report.herdr_available)),
        format!(
            "herdr version: {}",
            report.herdr_version.as_deref().unwrap_or("unknown")
        ),
        format!("config dir: {}", report.config_dir),
        format!("config: {}", report.config_path),
        format!("state dir: {}", report.state_dir),
        format!("state: {}", report.state_path),
        format!("scratchpads: {}", report.scratchpad_count),
    ];
    lines.extend(report.issues.iter().map(|issue| format!("issue: {issue}")));
    lines.push("next: herdr plugin action invoke guide --plugin herdr.scratch".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_doctor_output_points_to_the_guide() {
        let report = DoctorReport {
            herdr_available: true,
            herdr_version: Some("herdr 0.8.0".to_string()),
            config_dir: "/config".to_string(),
            config_path: "/config/config.toml".to_string(),
            state_dir: "/state".to_string(),
            state_path: "/state/registry.json".to_string(),
            scratchpad_count: 0,
            issues: Vec::new(),
        };

        let output = format_doctor(&report);
        assert!(output.contains("herdr: ok"));
        assert!(output.ends_with("next: herdr plugin action invoke guide --plugin herdr.scratch"));
    }
}
