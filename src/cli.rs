use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "herdr-scratch")]
#[command(about = "Persistent named scratchpads for Herdr")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the interactive Scratch quick-start guide.
    Guide,
    /// Show the scratchpad, or return to the previous context when it is active.
    Toggle(OpenArgs),
    /// Create or show a scratchpad.
    Open(OpenArgs),
    /// Open today's daily note (Obsidian vault with auto-detect, else local file).
    Daily(DailyArgs),
    /// Focus an existing scratchpad.
    Focus(NameArg),
    /// Leave a scratchpad without destroying it when possible.
    Hide(NameArg),
    /// Terminate and forget a scratchpad runtime.
    Close(NameArg),
    /// List known scratchpads.
    List(JsonArg),
    /// Show one scratchpad's status.
    Status(StatusArgs),
    /// Rename a scratchpad identity.
    Rename(RenameArgs),
    /// Send text to a scratchpad without pressing Enter.
    Send(SendArgs),
    /// Send a command to a scratchpad and press Enter.
    Run(RunArgs),
    /// Resize a popup scratchpad.
    Resize(ResizeArgs),
    /// Toggle a popup scratchpad to fullscreen (or back to its previous size).
    Fullscreen(NameArg),
    /// Reset a popup scratchpad to its configured size.
    Reset(NameArg),
    /// Write recommended Herdr keybindings for the scratchpad actions.
    Setup,
    /// Validate Herdr Scratch configuration and runtime connectivity.
    Doctor(JsonArg),
    /// Print config paths.
    Config(ConfigArgs),
    /// Print state paths.
    State(PathArgs),
    /// Internal pane entrypoint.
    #[command(hide = true)]
    Session,
    /// Internal popup attachment entrypoint.
    #[command(hide = true)]
    Attach,
    /// Internal quick-start popup entrypoint.
    #[command(hide = true)]
    GuidePane,
}

#[derive(Debug, Args)]
pub struct NameArg {
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct DailyArgs {
    /// Override the vault path for this invocation only.
    #[arg(long)]
    pub vault: Option<String>,
    /// Print the resolved daily-note path instead of opening it.
    #[arg(long)]
    pub print_path: bool,
    /// Open the note for a specific date (YYYY-MM-DD), e.g. for backfill.
    #[arg(long)]
    pub date: Option<String>,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    pub name: Option<String>,
    #[arg(last = true, allow_hyphen_values = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct JsonArg {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RenameArgs {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Args)]
pub struct SendArgs {
    pub name: String,
    pub text: Vec<String>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    pub name: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ResizeDirection {
    /// Increase the popup size.
    Up,
    /// Decrease the popup size.
    Down,
}

#[derive(Debug, Args)]
pub struct ResizeArgs {
    #[arg(value_enum)]
    pub direction: ResizeDirection,
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Print the primary path.
    Path,
    /// Write a default config file.
    Init(ConfigInitArgs),
    /// Add a named scratchpad/profile to the config file.
    Add(ConfigAddArgs),
}

#[derive(Debug, Args)]
pub struct ConfigInitArgs {
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ConfigAddArgs {
    pub name: String,
    #[arg(long)]
    pub scope: Option<String>,
    #[arg(long)]
    pub cwd: Option<String>,
    #[arg(
        last = true,
        required = true,
        allow_hyphen_values = true,
        value_name = "COMMAND"
    )]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct PathArgs {
    #[command(subcommand)]
    pub command: PathSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum PathSubcommand {
    /// Print the primary path.
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_open_with_trailing_command() {
        let cli = Cli::parse_from(["herdr-scratch", "open", "lazygit", "--", "lazygit"]);
        let Command::Open(args) = cli.command else {
            panic!("expected open command");
        };
        assert_eq!(args.name.as_deref(), Some("lazygit"));
        assert_eq!(args.command, vec!["lazygit"]);
    }

    #[test]
    fn parses_toggle_with_multi_word_trailing_command() {
        let cli = Cli::parse_from([
            "herdr-scratch",
            "toggle",
            "server",
            "--",
            "npm",
            "run",
            "dev",
        ]);
        let Command::Toggle(args) = cli.command else {
            panic!("expected toggle command");
        };
        assert_eq!(args.name.as_deref(), Some("server"));
        assert_eq!(args.command, vec!["npm", "run", "dev"]);
    }

    #[test]
    fn parses_config_add() {
        let cli = Cli::parse_from([
            "herdr-scratch",
            "config",
            "add",
            "lazygit",
            "--scope",
            "cwd",
            "--",
            "lazygit",
        ]);
        let Command::Config(config) = cli.command else {
            panic!("expected config command");
        };
        let ConfigSubcommand::Add(args) = config.command else {
            panic!("expected config add");
        };
        assert_eq!(args.name, "lazygit");
        assert_eq!(args.scope.as_deref(), Some("cwd"));
        assert_eq!(args.command, vec!["lazygit"]);
    }

    #[test]
    fn parses_daily_flags() {
        let cli = Cli::parse_from([
            "herdr-scratch",
            "daily",
            "--vault",
            "/tmp/vault",
            "--print-path",
            "--date",
            "2026-09-19",
        ]);
        let Command::Daily(args) = cli.command else {
            panic!("expected daily command");
        };
        assert_eq!(args.vault.as_deref(), Some("/tmp/vault"));
        assert!(args.print_path);
        assert_eq!(args.date.as_deref(), Some("2026-09-19"));
    }

    #[test]
    fn parses_guide() {
        let cli = Cli::parse_from(["herdr-scratch", "guide"]);
        assert!(matches!(cli.command, Command::Guide));
    }

    #[test]
    fn parses_setup_without_arguments() {
        let cli = Cli::parse_from(["herdr-scratch", "setup"]);
        assert!(matches!(cli.command, Command::Setup));
    }

    #[test]
    fn parses_resize_direction_and_fullscreen() {
        let up = Cli::parse_from(["herdr-scratch", "resize", "up"]);
        assert!(matches!(
            up.command,
            Command::Resize(ResizeArgs {
                direction: ResizeDirection::Up,
                ..
            })
        ));
        let named_down = Cli::parse_from(["herdr-scratch", "resize", "down", "notes"]);
        if let Command::Resize(ResizeArgs { direction, name }) = named_down.command {
            assert_eq!(direction, ResizeDirection::Down);
            assert_eq!(name.as_deref(), Some("notes"));
        } else {
            panic!("expected resize command");
        }
        let fullscreen = Cli::parse_from(["herdr-scratch", "fullscreen"]);
        assert!(matches!(
            fullscreen.command,
            Command::Fullscreen(NameArg { name: None })
        ));
        let reset = Cli::parse_from(["herdr-scratch", "reset", "scratch"]);
        if let Command::Reset(NameArg { name }) = reset.command {
            assert_eq!(name.as_deref(), Some("scratch"));
        } else {
            panic!("expected reset command");
        }
    }
}
