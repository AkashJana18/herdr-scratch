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
    /// Show the scratchpad, or return to the previous context when it is active.
    Toggle(OpenArgs),
    /// Create or show a scratchpad.
    Open(OpenArgs),
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
    /// Validate Herdr Scratch configuration and runtime connectivity.
    Doctor(JsonArg),
    /// Print config paths.
    Config(ConfigArgs),
    /// Print state paths.
    State(PathArgs),
    /// Internal pane entrypoint.
    #[command(hide = true)]
    Session,
}

#[derive(Debug, Args)]
pub struct NameArg {
    pub name: Option<String>,
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
}
