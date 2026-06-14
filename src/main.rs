mod cli;
mod config;
mod control;
mod doctor;
mod engine;
mod history;
mod init;
mod slideshow;
mod status;

use anyhow::{bail, Result};
use clap::Parser;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Command};
use crate::config::{load_config, resolve_config_path};
use crate::control::{
    request_pause, request_play, request_scheduler_status, request_set_interval,
    resolve_control_socket_path,
};
use crate::engine::{AppCommandRunner, DryRunCommandRunner, Engine, RealCommandRunner};

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

    let config_path = resolve_config_path(cli.config.as_deref());
    let doctor_runner = if cli.dry_run {
        AppCommandRunner::DryRun(DryRunCommandRunner)
    } else {
        AppCommandRunner::Real(RealCommandRunner::default())
    };

    if let Command::Init = cli.command {
        print!("{}", init::init_config(&config_path)?);
        return Ok(());
    }

    if let Command::Doctor = cli.command {
        let healthy = doctor::run_doctor(&config_path, &doctor_runner)?;
        if !healthy {
            std::process::exit(1);
        }
        return Ok(());
    }

    let config = load_config(&config_path)?;
    let runner = if cli.dry_run {
        AppCommandRunner::DryRun(DryRunCommandRunner)
    } else {
        AppCommandRunner::Real(RealCommandRunner::new(Duration::from_secs(
            config.commands.timeout_seconds,
        )))
    };

    tracing::debug!(?config, "Effective configuration");

    match cli.command {
        Command::Daemon => {
            tracing::info!("Starting slideshow daemon");
            tracing::info!(
                dir = %config.wallpaper_dir().display(),
                interval = config.slideshow.interval_seconds,
                randomize = config.slideshow.randomize,
                "Slideshow configuration"
            );
            slideshow::run_daemon(&config_path, config, &runner, cli.dry_run).await?;
        }
        Command::Reload => {
            let socket_path = resolve_control_socket_path(&config_path)?;
            if cli.dry_run {
                print!(
                    "temad reload\nDry-run: config valid; would request reload via {}\n",
                    socket_path.display()
                );
            } else {
                let response = control::request_reload(&config_path).await?;
                print!(
                    "temad reload\nRequested daemon reload via {}\nResult: {}\n",
                    socket_path.display(),
                    response
                );
            }
        }
        Command::Pause => {
            let socket_path = resolve_control_socket_path(&config_path)?;
            if cli.dry_run {
                print!(
                    "temad pause\nDry-run: would request pause via {}\n",
                    socket_path.display()
                );
            } else {
                let response = request_pause(&config_path).await?;
                print!(
                    "temad pause\nRequested daemon pause via {}\nResult: {}\n",
                    socket_path.display(),
                    response
                );
            }
        }
        Command::Play => {
            let socket_path = resolve_control_socket_path(&config_path)?;
            if cli.dry_run {
                print!(
                    "temad play\nDry-run: would request play via {}\n",
                    socket_path.display()
                );
            } else {
                let response = request_play(&config_path).await?;
                print!(
                    "temad play\nRequested daemon play via {}\nResult: {}\n",
                    socket_path.display(),
                    response
                );
            }
        }
        Command::Interval { seconds } => {
            if seconds == 0 {
                bail!("Interval seconds must be greater than 0");
            }

            let socket_path = resolve_control_socket_path(&config_path)?;
            if cli.dry_run {
                print!(
                    "temad interval\nDry-run: would request interval {} via {}\n",
                    seconds,
                    socket_path.display()
                );
            } else {
                let response = request_set_interval(&config_path, seconds).await?;
                print!(
                    "temad interval\nRequested daemon interval {} via {}\nResult: {}\n",
                    seconds,
                    socket_path.display(),
                    response
                );
            }
        }
        Command::Status => {
            let scheduler_status = request_scheduler_status(&config_path).await.ok();
            let report = status::render_status_with_remaining(
                &config_path,
                &config,
                &runner,
                scheduler_status,
            );
            print!("{report}");
        }
        Command::Doctor => unreachable!("handled before config load"),
        Command::History => {
            print!("{}", history::render_history(&config));
        }
        Command::Init => unreachable!("handled before config load"),
        Command::Set { ref path } => {
            let abs = validate_set_path(path)?;

            tracing::info!(path = %abs.display(), "Setting wallpaper");
            let engine = Engine::new(&config, &runner);
            engine.apply(&abs)?;
            if !cli.dry_run {
                if let Err(error) = slideshow::persist_last_image(
                    &config.wallpaper_dir(),
                    &abs,
                    config.slideshow.history_limit,
                ) {
                    tracing::warn!(error = %error, path = %abs.display(), "Failed to persist last wallpaper state");
                }
            }
        }
        Command::Next => {
            let image = slideshow::run_next(&config, &runner, cli.dry_run)?;
            tracing::info!(path = %image.display(), "Applied next wallpaper");
        }
        Command::Prev => {
            let image = slideshow::run_prev(&config, &runner, cli.dry_run)?;
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
