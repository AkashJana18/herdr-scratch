mod cli;
mod config;
mod herdr;
mod obsidian;
mod output;
mod registry;
mod scratchpad;

use anyhow::Context;
use clap::Parser;

fn main() {
    let args = cli::Cli::parse();
    if let Err(err) = run(args) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run(args: cli::Cli) -> anyhow::Result<()> {
    if matches!(&args.command, cli::Command::Session) {
        return scratchpad::run_runtime_session();
    }
    if matches!(&args.command, cli::Command::GuidePane) {
        return scratchpad::run_guide_pane();
    }
    let paths = config::Paths::discover().context("failed to discover plugin paths")?;
    if matches!(&args.command, cli::Command::Attach) {
        return scratchpad::run_popup_attach(paths);
    }
    let store = registry::RegistryStore::new(paths.registry_file.clone());
    let _registry_lock = store.lock().context("failed to lock registry")?;
    let config = config::Config::load(&paths.config_file).context("failed to load config")?;
    let registry = store.load().context("failed to load registry")?;
    let herdr = herdr::HerdrCli::discover();

    let mut app = scratchpad::ScratchApp::new(config, registry, paths, herdr);
    let result = app.handle(args.command)?;
    output::print(result)
}
