mod cli;
mod config;
mod herdr;
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
    let paths = config::Paths::discover().context("failed to discover plugin paths")?;
    let config = config::Config::load(&paths.config_file).context("failed to load config")?;
    let registry = registry::RegistryStore::new(paths.registry_file.clone())
        .load()
        .context("failed to load registry")?;
    let herdr = herdr::HerdrCli::discover();

    let mut app = scratchpad::ScratchApp::new(config, registry, paths, herdr);
    let result = app.handle(args.command)?;
    output::print(result)
}
