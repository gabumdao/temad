mod cli;
mod config;
mod engine;
mod slideshow;
mod status;

use anyhow::{bail, Result};
use clap::Parser;
use std::io;
use std::path::{Path, PathBuf};
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

fn validate_set_path(path: &Path) -> Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    if !abs.exists() {
        bail!("Wallpaper file does not exist: {}", abs.display());
    }

    if !abs.is_file() {
        bail!("Wallpaper path is not a file: {}", abs.display());
    }

    if !slideshow::is_supported_image_path(&abs) {
        bail!(
            "Wallpaper file must use a supported image extension (.jpg, .jpeg, .png, .webp, .bmp, .gif): {}",
            abs.display()
        );
    }

    Ok(abs)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    init_logging(cli.verbose);

    if let Command::Completions { shell } = cli.command {
        cli::generate_completions(shell, &mut io::stdout());
        return Ok(());
    }

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
        Command::Status => {
            let report = status::render_status(&config_path, &config, &runner);
            print!("{report}");
        }
        Command::Set { ref path } => {
            let abs = validate_set_path(path)?;

            tracing::info!(path = %abs.display(), "Setting wallpaper");
            let engine = Engine::new(&config, &runner);
            engine.apply(&abs)?;
            if let Err(error) = slideshow::persist_last_image(&config.wallpaper_dir(), &abs) {
                tracing::warn!(error = %error, path = %abs.display(), "Failed to persist last wallpaper state");
            }
        }
        Command::Next => {
            let image = slideshow::run_next(&config, &runner)?;
            tracing::info!(path = %image.display(), "Applied next wallpaper");
        }
        Command::Prev => {
            let image = slideshow::run_prev(&config, &runner)?;
            tracing::info!(path = %image.display(), "Applied previous wallpaper");
        }
        Command::Completions { .. } => unreachable!("handled before config load"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_set_path;
    use std::fs;
    use std::path::Path;

    #[test]
    fn validate_set_path_accepts_supported_image_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallpaper.png");
        fs::write(&path, b"image").expect("write image");

        let resolved = validate_set_path(&path).expect("supported file should validate");
        assert_eq!(resolved, path);
    }

    #[test]
    fn validate_set_path_rejects_missing_file() {
        let path = Path::new("/tmp/temad_missing_wallpaper.png");
        let err = validate_set_path(path).expect_err("missing file must error");
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn validate_set_path_rejects_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = validate_set_path(dir.path()).expect_err("directory must error");
        assert!(err.to_string().contains("not a file"));
    }

    #[test]
    fn validate_set_path_rejects_unsupported_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, b"not an image").expect("write text file");

        let err = validate_set_path(&path).expect_err("unsupported extension must error");
        assert!(err.to_string().contains("supported image extension"));
    }

    #[test]
    fn validate_set_path_accepts_mixed_case_supported_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wallpaper.JPG");
        fs::write(&path, b"image").expect("write image");

        let resolved =
            validate_set_path(&path).expect("mixed-case supported extension should validate");
        assert_eq!(resolved, path);
    }
}
