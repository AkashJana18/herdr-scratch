use std::path::{Path, PathBuf};

use time::Date;

/// A vault registered in Obsidian's `obsidian.json` registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    pub path: PathBuf,
    pub ts: u64,
}

/// Daily-note settings for one vault (from `.obsidian/daily-notes.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyVaultConfig {
    /// Subdirectory inside the vault (may contain `/` for nested folders).
    pub folder: String,
    /// Moment.js date format, e.g. `YYYY-MM-DD` or `YYYY/MM/YYYY-MM-DD`.
    pub format: String,
    /// Vault-relative template path (without extension), if configured.
    pub template: Option<String>,
}

impl Default for DailyVaultConfig {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: "YYYY-MM-DD".to_string(),
            template: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesSource {
    Explicit,
    Auto,
    Fallback,
}

impl std::fmt::Display for NotesSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Explicit => f.write_str("explicit"),
            Self::Auto => f.write_str("auto"),
            Self::Fallback => f.write_str("fallback"),
        }
    }
}

/// Candidate `obsidian.json` registry paths, highest priority first.
///
/// Order: explicit override param, `$HERDR_SCRATCH_OBSIDIAN_CONFIG` env,
/// native per-OS default, then Linux Flatpak/Snap variants.
pub fn candidate_registry_paths(override_path: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(raw) = override_path.filter(|s| !s.trim().is_empty()) {
        paths.push(PathBuf::from(raw.trim()));
    }
    if let Ok(env) = std::env::var("HERDR_SCRATCH_OBSIDIAN_CONFIG")
        && !env.trim().is_empty()
    {
        let path = PathBuf::from(env.trim());
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    for candidate in default_registry_paths() {
        if !paths.contains(&candidate) {
            paths.push(candidate);
        }
    }
    paths
}

fn default_registry_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    if cfg!(target_os = "macos") {
        return vec![
            home.join("Library")
                .join("Application Support")
                .join("obsidian")
                .join("obsidian.json"),
        ];
    }
    if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA")
            && !appdata.is_empty()
        {
            return vec![
                PathBuf::from(appdata)
                    .join("obsidian")
                    .join("obsidian.json"),
            ];
        }
        return Vec::new();
    }
    // Linux and friends.
    let mut paths = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        paths.push(
            PathBuf::from(xdg.trim())
                .join("obsidian")
                .join("obsidian.json"),
        );
    }
    paths.push(home.join(".config").join("obsidian").join("obsidian.json"));
    paths.push(
        home.join(".var")
            .join("app")
            .join("md.obsidian.Obsidian")
            .join("config")
            .join("obsidian")
            .join("obsidian.json"),
    );
    paths.push(
        home.join("snap")
            .join("obsidian")
            .join("current")
            .join(".config")
            .join("obsidian")
            .join("obsidian.json"),
    );
    paths
}

/// Parse vaults out of one `obsidian.json` file. Tolerant: any error -> empty.
pub fn registered_vaults(registry_path: &Path) -> Vec<Vault> {
    let Ok(content) = std::fs::read_to_string(registry_path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(vaults) = value.get("vaults").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in vaults.values() {
        let Some(path) = entry.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if path.trim().is_empty() {
            continue;
        }
        let ts = entry
            .get("ts")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        out.push(Vault {
            path: PathBuf::from(path),
            ts,
        });
    }
    out
}

/// Detect vaults using the first registry file that yields a non-empty list.
pub fn detect_vaults(override_path: Option<&str>) -> Vec<Vault> {
    for candidate in candidate_registry_paths(override_path) {
        let vaults = registered_vaults(&candidate);
        if !vaults.is_empty() {
            return vaults;
        }
    }
    Vec::new()
}

/// Most-recently-opened vault that still exists on disk.
pub fn pick_vault(mut vaults: Vec<Vault>) -> Option<PathBuf> {
    vaults.sort_by_key(|vault| vault.ts);
    vaults
        .into_iter()
        .rev()
        .map(|vault| vault.path)
        .find(|path| path.is_dir())
}

/// Common vault locations used when no registry entry exists.
pub fn fallback_vault_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![
        home.join("Obsidian"),
        home.join("Documents").join("Obsidian Vault"),
        home.join("Documents").join("Obsidian"),
    ]
}

/// First fallback vault dir that exists (preferring ones with `.obsidian`).
pub fn fallback_vault() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = fallback_vault_dirs()
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
    candidates
        .iter()
        .find(|p| p.join(".obsidian").is_dir())
        .or_else(|| candidates.first())
        .cloned()
}

/// Read `<vault>/.obsidian/daily-notes.json`. Missing/malformed -> defaults.
pub fn read_daily_config(vault: &Path) -> DailyVaultConfig {
    let path = vault.join(".obsidian").join("daily-notes.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return DailyVaultConfig::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return DailyVaultConfig::default();
    };
    let folder = value
        .get("folder")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .trim_matches('/')
        .to_string();
    let format = value
        .get("format")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("YYYY-MM-DD")
        .to_string();
    let template = value
        .get("template")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    DailyVaultConfig {
        folder,
        format,
        template,
    }
}

const MONTH_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAY_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const WEEKDAY_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Render a Moment.js date format for `date`.
///
/// Supported tokens: `YYYY YY MMMM MMM MM M DD D dddd ddd`.
/// Anything else passes through literally. Empty results fall back to
/// `YYYY-MM-DD` rendering.
pub fn render_moment_format(date: Date, moment: &str) -> String {
    let year = date.year();
    let month_idx = u8::from(date.month()).saturating_sub(1) as usize;
    let day = date.day();
    let weekday_idx = date.weekday().number_days_from_monday() as usize;
    let yy = (year % 100 + 100) % 100;

    let mut out = String::with_capacity(moment.len() + 8);
    let bytes = moment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &moment[i..];
        // Longest tokens first to avoid prefix collisions (MM vs M, etc.).
        if rest.starts_with("MMMM") {
            out.push_str(MONTH_FULL[month_idx.min(11)]);
            i += 4;
        } else if rest.starts_with("dddd") {
            out.push_str(WEEKDAY_FULL[weekday_idx.min(6)]);
            i += 4;
        } else if rest.starts_with("YYYY") {
            out.push_str(&format!("{year:04}"));
            i += 4;
        } else if rest.starts_with("MMM") {
            out.push_str(MONTH_SHORT[month_idx.min(11)]);
            i += 3;
        } else if rest.starts_with("ddd") {
            out.push_str(WEEKDAY_SHORT[weekday_idx.min(6)]);
            i += 3;
        } else if rest.starts_with("YY") {
            out.push_str(&format!("{yy:02}"));
            i += 2;
        } else if rest.starts_with("MM") {
            out.push_str(&format!("{:02}", month_idx + 1));
            i += 2;
        } else if rest.starts_with("DD") {
            out.push_str(&format!("{day:02}"));
            i += 2;
        } else if rest.starts_with('M') {
            out.push_str(&format!("{}", month_idx + 1));
            i += 1;
        } else if rest.starts_with('D') {
            out.push_str(&format!("{day}"));
            i += 1;
        } else if rest.starts_with('d') {
            // Day-of-week numeric tokens are not filename-meaningful; keep literal.
            out.push('d');
            i += 1;
        } else {
            let ch = rest.chars().next().expect("non-empty rest");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    if out.trim().is_empty() {
        return format!("{year:04}-{:02}-{day:02}", month_idx + 1);
    }
    out
}

/// Filename for `date` under a Moment.js format (appends `.md` if missing).
pub fn daily_filename(date: Date, moment: &str) -> String {
    let moment = moment.trim();
    let moment = if moment.is_empty() {
        "YYYY-MM-DD"
    } else {
        moment
    };
    let rendered = render_moment_format(date, moment);
    if rendered.to_ascii_lowercase().ends_with(".md") {
        rendered
    } else {
        format!("{rendered}.md")
    }
}

/// `YYYY-MM-DD` string for titles and the builtin template.
pub fn iso_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

/// Resolve the editor: explicit config, `$VISUAL`/`$EDITOR`, else `vim`.
pub fn resolve_editor(configured: Option<&str>) -> String {
    if let Some(editor) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        return editor.to_string();
    }
    for key in ["VISUAL", "EDITOR"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            return value.trim().to_string();
        }
    }
    "vim".to_string()
}

/// Create the daily file if missing. Copies the template verbatim when it
/// exists, otherwise writes a minimal `# date / Tasks / Notes` skeleton.
/// Never overwrites an existing file.
pub fn ensure_daily_file(
    file: &Path,
    template_path: Option<&Path>,
    date_str: &str,
) -> anyhow::Result<()> {
    if file.exists() {
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(template) = template_path
        && template.is_file()
    {
        let content = std::fs::read_to_string(template)?;
        std::fs::write(file, content)?;
        return Ok(());
    }
    std::fs::write(
        file,
        format!("# {date_str}\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n"),
    )?;
    Ok(())
}

/// Resolve a vault-relative template reference to a filesystem path.
/// Handles `Templates/Daily`, `Templates/Daily.md`, and absolute paths.
pub fn resolve_template_path(vault: Option<&Path>, template: Option<&str>) -> Option<PathBuf> {
    let raw = template.map(str::trim).filter(|s| !s.is_empty())?;
    let direct = PathBuf::from(raw);
    if direct.is_absolute() {
        return Some(direct);
    }
    let vault = vault?;
    let with_ext = if direct.extension().is_some() {
        direct.clone()
    } else {
        direct.with_extension("md")
    };
    Some(vault.join(with_ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn date(year: i32, month: Month, day: u8) -> Date {
        Date::from_calendar_date(year, month, day).unwrap()
    }

    #[test]
    fn renders_default_iso_format() {
        let d = date(2026, Month::September, 20);
        assert_eq!(render_moment_format(d, "YYYY-MM-DD"), "2026-09-20");
    }

    #[test]
    fn renders_nested_moment_format_with_subfolders() {
        let d = date(2026, Month::January, 5);
        assert_eq!(
            render_moment_format(d, "YYYY/MM/YYYY-MM-DD"),
            "2026/01/2026-01-05"
        );
    }

    #[test]
    fn renders_month_names() {
        let d = date(2026, Month::September, 20);
        assert_eq!(render_moment_format(d, "YYYY-MMMM-DD"), "2026-September-20");
        assert_eq!(render_moment_format(d, "YY-MMM-D"), "26-Sep-20");
    }

    #[test]
    fn renders_unpadded_tokens() {
        let d = date(2026, Month::January, 5);
        assert_eq!(render_moment_format(d, "YYYY-M-D"), "2026-1-5");
    }

    #[test]
    fn empty_format_falls_back_to_iso() {
        let d = date(2026, Month::September, 20);
        assert_eq!(render_moment_format(d, "   "), "2026-09-20");
    }

    #[test]
    fn daily_filename_appends_md_once() {
        let d = date(2026, Month::September, 20);
        assert_eq!(daily_filename(d, "YYYY-MM-DD"), "2026-09-20.md");
        assert_eq!(daily_filename(d, "YYYY-MM-DD.md"), "2026-09-20.md");
        assert_eq!(daily_filename(d, ""), "2026-09-20.md");
    }

    #[test]
    fn registered_vaults_parses_registry_and_ignores_bad_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obsidian.json");
        std::fs::write(
            &path,
            r#"{"vaults":{"a":{"path":"/v/a","ts":10},"b":{"path":"  "},"c":{"path":"/v/c"}}}"#,
        )
        .unwrap();
        let vaults = registered_vaults(&path);
        assert_eq!(vaults.len(), 2);
        assert!(
            vaults
                .iter()
                .any(|v| v.path == Path::new("/v/a") && v.ts == 10)
        );
    }

    #[test]
    fn registered_vaults_tolerates_missing_and_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(registered_vaults(&dir.path().join("missing.json")).is_empty());
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "not json").unwrap();
        assert!(registered_vaults(&bad).is_empty());
    }

    #[test]
    fn pick_vault_prefers_most_recent_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        let picked = pick_vault(vec![
            Vault {
                path: new.clone(),
                ts: 99,
            },
            Vault {
                path: PathBuf::from("/does/not/exist"),
                ts: 999,
            },
            Vault {
                path: old.clone(),
                ts: 5,
            },
        ]);
        assert_eq!(picked, Some(new));
    }

    #[test]
    fn read_daily_config_defaults_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = read_daily_config(dir.path());
        assert_eq!(config.folder, "");
        assert_eq!(config.format, "YYYY-MM-DD");
        assert_eq!(config.template, None);
    }

    #[test]
    fn read_daily_config_parses_folder_format_template() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        std::fs::write(
            dir.path().join(".obsidian").join("daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY/MM/DD","template":"Templates/Daily"}"#,
        )
        .unwrap();
        let config = read_daily_config(dir.path());
        assert_eq!(config.folder, "Daily");
        assert_eq!(config.format, "YYYY/MM/DD");
        assert_eq!(config.template.as_deref(), Some("Templates/Daily"));
    }

    #[test]
    fn ensure_daily_file_writes_skeleton_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Daily").join("2026-09-20.md");
        ensure_daily_file(&file, None, "2026-09-20").unwrap();
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("# 2026-09-20"));
        std::fs::write(&file, "keep me").unwrap();
        ensure_daily_file(&file, None, "2026-09-20").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep me");
    }

    #[test]
    fn ensure_daily_file_copies_template_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("tpl.md");
        std::fs::write(&template, "# {{date}}\n").unwrap();
        let file = dir.path().join("2026-09-20.md");
        ensure_daily_file(&file, Some(&template), "2026-09-20").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# {{date}}\n");
    }

    #[test]
    fn resolve_template_path_handles_relative_and_absolute() {
        let vault = Path::new("/vault");
        assert_eq!(
            resolve_template_path(Some(vault), Some("Templates/Daily")),
            Some(PathBuf::from("/vault/Templates/Daily.md"))
        );
        assert_eq!(
            resolve_template_path(Some(vault), Some("/abs/t.md")),
            Some(PathBuf::from("/abs/t.md"))
        );
        assert_eq!(resolve_template_path(None, Some("T/D")), None);
        assert_eq!(resolve_template_path(Some(vault), Some("  ")), None);
    }

    #[test]
    fn candidate_paths_prioritize_override_and_env() {
        unsafe {
            std::env::set_var("HERDR_SCRATCH_OBSIDIAN_CONFIG", "/tmp/env-obsidian.json");
        }
        let paths = candidate_registry_paths(Some("/tmp/cli-obsidian.json"));
        unsafe {
            std::env::remove_var("HERDR_SCRATCH_OBSIDIAN_CONFIG");
        }
        assert_eq!(paths[0], PathBuf::from("/tmp/cli-obsidian.json"));
        assert_eq!(paths[1], PathBuf::from("/tmp/env-obsidian.json"));
    }
}
