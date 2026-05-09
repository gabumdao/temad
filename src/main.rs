mod cli;
mod config;
mod engine;
mod slideshow;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};
use crate::config::{load_config, resolve_config_path};
use crate::engine::{Engine, RealCommandRunner};

fn init_logging(verbosity: u8) {
    let filter = match verbosity {
        0 => "temad=info",
        1 => "temad=debug",
        2 => "temad=trace",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    init_logging(cli.verbose);

    // Resolve and load configuration
    let config_path = resolve_config_path(cli.config.as_deref());
    let config = load_config(&config_path)?;

    tracing::debug!(?config, "Effective configuration");

    let runner = RealCommandRunner;

    match cli.command {
        Command::Daemon => {
            tracing::info!("Starting slideshow daemon");
            tracing::info!(
                dir = %config.wallpaper_dir().display(),
                interval = config.slideshow.interval_seconds,
                randomize = config.slideshow.randomize,
                "Slideshow configuration"
            );
            slideshow::run_daemon(&config, &runner).await?;
        }
        Command::Set { ref path } => {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()?.join(path)
            };

            tracing::info!(path = %abs.display(), "Setting wallpaper");
            let engine = Engine::new(&config, &runner);
            engine.apply(&abs)?;
        }
    }

    Ok(())
}
