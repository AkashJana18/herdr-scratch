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
    None,
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
            println!("herdr: {}", status_word(report.herdr_available));
            println!("config dir: {}", report.config_dir);
            println!("config: {}", report.config_path);
            println!("state dir: {}", report.state_dir);
            println!("state: {}", report.state_path);
            println!("scratchpads: {}", report.scratchpad_count);
            for issue in report.issues {
                println!("issue: {issue}");
            }
            Ok(())
        }
        Output::None => Ok(()),
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
