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
    Toggle(NameArg),
    /// Create or show a scratchpad.
    Open(NameArg),
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
    Config(PathArgs),
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
pub struct PathArgs {
    #[command(subcommand)]
    pub command: PathSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum PathSubcommand {
    /// Print the primary path.
    Path,
}
