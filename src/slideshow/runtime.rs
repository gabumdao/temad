use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use tokio::net::UnixStream;
use tokio::time::{sleep_until, Duration, Instant};

use crate::config::{load_config, Config};
use crate::control::{
    read_command_with_timeout, respond_error, respond_ok, ControlCommand, ControlSocket,
};
use crate::engine::{CommandRunner, Engine};

use super::discovery::discover_images;
use super::history_state::{last_image_state_path, load_history, with_locked_history_from};
use super::selector::ImageSelector;

struct DaemonState {
    config: Config,
    wall_dir: PathBuf,
    selector: ImageSelector,
    interval: Duration,
}

impl DaemonState {
    fn sync_last_from_disk(&mut self) {
        match load_history(&self.wall_dir) {
            Ok(history) => self.selector.sync_last(history.current_cloned()),
            Err(error) => tracing::warn!(error = %error, "Failed to reload wallpaper history"),
        }
    }

    fn from_config(config: Config) -> Result<Self> {
        let wall_dir = config.wallpaper_dir();
        let images = discover_images(&wall_dir, config.slideshow.recursive)?;

        tracing::info!(
            count = images.len(),
            dir = %wall_dir.display(),
            "Image pool ready"
        );

        let history = match load_history(&wall_dir) {
            Ok(history) => history,
            Err(error) => {
                tracing::warn!(error = %error, dir = %wall_dir.display(), "Failed to load wallpaper history state");
                Default::default()
            }
        };

        let selector = match history.current_cloned() {
            Some(last) => ImageSelector::with_last(images, config.slideshow.randomize, Some(last))?,
            None => ImageSelector::new(images, config.slideshow.randomize)?,
        };

        let interval = Duration::from_secs(config.slideshow.interval_seconds);

        Ok(Self {
            config,
            wall_dir,
            selector,
            interval,
        })
    }
}

pub(crate) fn run_daemon_cycle<R: CommandRunner>(
    wall_dir: &Path,
    selector: &mut ImageSelector,
    engine: &Engine<R>,
) -> Option<PathBuf> {
    match discover_images(wall_dir, engine.config.slideshow.recursive) {
        Ok(images) => {
            if let Err(e) = selector.update_images(images) {
                tracing::warn!(error = %e, dir = %wall_dir.display(), "No wallpapers available; skipping cycle");
                return None;
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, dir = %wall_dir.display(), "Failed to refresh wallpaper directory; skipping cycle");
            return None;
        }
    }

    let image = selector.next().to_path_buf();
    tracing::info!(image = %image.display(), "Applying wallpaper");

    if let Err(e) = engine.apply(&image) {
        tracing::error!(error = %e, image = %image.display(), "Failed to apply theme");
        return None;
    }

    Some(image)
}

pub(crate) fn run_next<R: CommandRunner>(
    config: &Config,
    runner: &R,
    dry_run: bool,
) -> Result<PathBuf> {
    let wall_dir = config.wallpaper_dir();
    let images = discover_images(&wall_dir, config.slideshow.recursive)?;
    let state_path = last_image_state_path();

    let engine = Engine::new(config, runner);

    if dry_run {
        let history = load_history(&wall_dir).unwrap_or_default();
        if let Some(image) = history.next().cloned() {
            tracing::info!(image = %image.display(), "Dry-run next wallpaper from history");
            engine.apply(&image)?;
            return Ok(image);
        }

        let mut selector = match history.current_cloned() {
            Some(last) => {
                ImageSelector::with_last(images.clone(), config.slideshow.randomize, Some(last))?
            }
            None => ImageSelector::new(images.clone(), config.slideshow.randomize)?,
        };
        let image = selector.next().to_path_buf();
        tracing::info!(image = %image.display(), "Dry-run next wallpaper");
        engine.apply(&image)?;
        return Ok(image);
    }

    with_locked_history_from(&state_path, &wall_dir, |history| {
        if let Some(image) = history.next().cloned() {
            tracing::info!(image = %image.display(), "Applying next wallpaper from history");
            engine.apply(&image)?;
            history.step_next();
            return Ok(image);
        }

        let mut selector = match history.current_cloned() {
            Some(last) => {
                ImageSelector::with_last(images.clone(), config.slideshow.randomize, Some(last))?
            }
            None => ImageSelector::new(images.clone(), config.slideshow.randomize)?,
        };
        let image = selector.next().to_path_buf();

        tracing::info!(image = %image.display(), "Applying next wallpaper");
        engine.apply(&image)?;
        history.record(image.clone(), config.slideshow.history_limit);
        Ok(image)
    })
}

pub(crate) fn run_prev<R: CommandRunner>(
    config: &Config,
    runner: &R,
    dry_run: bool,
) -> Result<PathBuf> {
    let wall_dir = config.wallpaper_dir();
    let images = discover_images(&wall_dir, config.slideshow.recursive)?;
    let state_path = last_image_state_path();

    let engine = Engine::new(config, runner);

    if dry_run {
        let history = load_history(&wall_dir).unwrap_or_default();
        if let Some(image) = history.previous().cloned() {
            tracing::info!(image = %image.display(), "Dry-run previous wallpaper from history");
            engine.apply(&image)?;
            return Ok(image);
        }

        if config.slideshow.randomize {
            bail!("No previous wallpaper available in history");
        }

        let mut selector = match history.current_cloned() {
            Some(last) => {
                ImageSelector::with_last(images.clone(), config.slideshow.randomize, Some(last))?
            }
            None => ImageSelector::new(images.clone(), config.slideshow.randomize)?,
        };
        let image = selector.prev().to_path_buf();
        tracing::info!(image = %image.display(), "Dry-run previous wallpaper");
        engine.apply(&image)?;
        return Ok(image);
    }

    with_locked_history_from(&state_path, &wall_dir, |history| {
        if let Some(image) = history.previous().cloned() {
            tracing::info!(image = %image.display(), "Applying previous wallpaper from history");
            engine.apply(&image)?;
            history.step_prev();
            return Ok(image);
        }

        if config.slideshow.randomize {
            bail!("No previous wallpaper available in history");
        }

        let mut selector = match history.current_cloned() {
            Some(last) => {
                ImageSelector::with_last(images.clone(), config.slideshow.randomize, Some(last))?
            }
            None => ImageSelector::new(images.clone(), config.slideshow.randomize)?,
        };
        let image = selector.prev().to_path_buf();

        tracing::info!(image = %image.display(), "Applying previous wallpaper");
        engine.apply(&image)?;
        history.prepend_previous(image.clone(), config.slideshow.history_limit);
        Ok(image)
    })
}

// A disconnected IPC client must not terminate the daemon.
async fn reply_ok(stream: &mut UnixStream, message: &str) -> Result<()> {
    if let Err(error) = respond_ok(stream, message).await {
        tracing::warn!(error = %error, "Control client disconnected");
    }
    Ok(())
}

async fn reply_error(stream: &mut UnixStream, message: &str) -> Result<()> {
    if let Err(error) = respond_error(stream, message).await {
        tracing::warn!(error = %error, "Control client disconnected");
    }
    Ok(())
}

pub(crate) async fn run_daemon<R: CommandRunner>(
    config_path: &Path,
    config: Config,
    runner: &R,
    dry_run: bool,
) -> Result<()> {
    let mut state = DaemonState::from_config(config)?;

    if dry_run {
        let engine = Engine::new(&state.config, runner);
        let _ = run_daemon_cycle(&state.wall_dir, &mut state.selector, &engine);
        return Ok(());
    }

    let control_socket = ControlSocket::bind(config_path)?;
    tracing::info!(path = %control_socket.socket_path().display(), "temad control socket ready");

    let mut next_tick = Instant::now();
    let mut paused_remaining: Option<Duration> = None;

    loop {
        if let Some(remaining) = paused_remaining {
            let (mut stream, _) = control_socket.accept().await?;
            match read_command_with_timeout(&mut stream).await {
                Ok(ControlCommand::Reload) => {
                    match load_config(config_path).and_then(DaemonState::from_config) {
                        Ok(reloaded_state) => {
                            state = reloaded_state;
                            paused_remaining = Some(remaining.min(state.interval));
                            tracing::info!(interval = state.config.slideshow.interval_seconds, dir = %state.wall_dir.display(), "Reloaded daemon config and state while paused");
                            reply_ok(&mut stream, "reloaded").await?;
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "Failed to reload daemon config and state");
                            reply_error(&mut stream, &error.to_string()).await?;
                        }
                    }
                }
                Ok(ControlCommand::Pause) => {
                    reply_ok(&mut stream, "already paused").await?;
                }
                Ok(ControlCommand::Play) => {
                    next_tick = Instant::now() + remaining;
                    paused_remaining = None;
                    reply_ok(&mut stream, "playing").await?;
                }
                Ok(ControlCommand::SetInterval(seconds)) => {
                    state.config.slideshow.interval_seconds = seconds;
                    state.interval = Duration::from_secs(seconds);
                    paused_remaining = Some(remaining.min(state.interval));
                    reply_ok(&mut stream, "interval updated").await?;
                }
                Ok(ControlCommand::ResetCountdown) => {
                    state.sync_last_from_disk();
                    paused_remaining = Some(state.interval);
                    reply_ok(&mut stream, "countdown reset").await?;
                }
                Ok(ControlCommand::RemainingSeconds | ControlCommand::SchedulerStatus) => {
                    reply_ok(
                        &mut stream,
                        &format!(
                            "paused {} {}",
                            remaining.as_secs(),
                            state.config.slideshow.interval_seconds
                        ),
                    )
                    .await?;
                }
                Err(error) => {
                    tracing::warn!(error = %error, "Failed to parse temad control command");
                    reply_error(&mut stream, &error.to_string()).await?;
                }
            }

            continue;
        }

        let sleep = sleep_until(next_tick);
        tokio::pin!(sleep);

        tokio::select! {
            _ = &mut sleep => {
                state.sync_last_from_disk();
                let engine = Engine::new(&state.config, runner);
                if let Some(image) = run_daemon_cycle(&state.wall_dir, &mut state.selector, &engine) {
                    let latest_image = image.clone();
                    if let Err(error) = with_locked_history_from(
                        &last_image_state_path(),
                        &state.wall_dir,
                        |history| {
                            history.record(latest_image.clone(), state.config.slideshow.history_limit);
                            Ok(())
                        },
                    ) {
                        tracing::warn!(error = %error, path = %image.display(), "Failed to persist wallpaper history state");
                    }
                }

                tracing::debug!(seconds = state.config.slideshow.interval_seconds, "Sleeping");
                next_tick = Instant::now() + state.interval;
            }
            accept_result = control_socket.accept() => {
                let (mut stream, _) = accept_result?;
                match read_command_with_timeout(&mut stream).await {
                    Ok(ControlCommand::Reload) => {
                        match load_config(config_path).and_then(DaemonState::from_config) {
                            Ok(reloaded_state) => {
                                state = reloaded_state;
                                next_tick = Instant::now() + state.interval;
                                tracing::info!(interval = state.config.slideshow.interval_seconds, dir = %state.wall_dir.display(), "Reloaded daemon config and state");
                                reply_ok(&mut stream, "reloaded").await?;
                            }
                            Err(error) => {
                                tracing::warn!(error = %error, "Failed to reload daemon config and state");
                                reply_error(&mut stream, &error.to_string()).await?;
                            }
                        }
                    }
                    Ok(ControlCommand::Pause) => {
                        paused_remaining = Some(next_tick.saturating_duration_since(Instant::now()));
                        reply_ok(&mut stream, "paused").await?;
                    }
                    Ok(ControlCommand::Play) => {
                        reply_ok(&mut stream, "already running").await?;
                    }
                    Ok(ControlCommand::SetInterval(seconds)) => {
                        state.config.slideshow.interval_seconds = seconds;
                        state.interval = Duration::from_secs(seconds);
                        next_tick = Instant::now() + state.interval;
                        reply_ok(&mut stream, "interval updated").await?;
                    }
                    Ok(ControlCommand::ResetCountdown) => {
                        state.sync_last_from_disk();
                        next_tick = Instant::now() + state.interval;
                        reply_ok(&mut stream, "countdown reset").await?;
                    }
                    Ok(ControlCommand::RemainingSeconds | ControlCommand::SchedulerStatus) => {
                        let remaining = next_tick.saturating_duration_since(Instant::now()).as_secs();
                        reply_ok(&mut stream, &format!("running {remaining} {}", state.config.slideshow.interval_seconds)).await?;
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Failed to parse temad control command");
                        reply_error(&mut stream, &error.to_string()).await?;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonState;
    use crate::config::load_config;
    use std::fs;
    use tokio::time::Duration;

    fn write_config(path: &std::path::Path, wall_dir: &std::path::Path, interval_seconds: u64) {
        let config = format!(
            "[slideshow]\ndirectory = \"{}\"\ninterval_seconds = {}\nrandomize = true\nhistory_limit = 32\nrecursive = false\n",
            wall_dir.display(),
            interval_seconds
        );
        fs::write(path, config).expect("write config");
    }

    #[test]
    fn daemon_state_from_config_uses_current_wallpaper_dir_and_interval() {
        let config_dir = tempfile::tempdir().expect("tempdir");
        let first_wall_dir = tempfile::tempdir().expect("tempdir");
        let second_wall_dir = tempfile::tempdir().expect("tempdir");
        let config_path = config_dir.path().join("config.toml");

        fs::write(first_wall_dir.path().join("one.jpg"), b"img").expect("write image");
        fs::write(second_wall_dir.path().join("two.jpg"), b"img").expect("write image");

        write_config(&config_path, first_wall_dir.path(), 15);
        let initial = DaemonState::from_config(load_config(&config_path).expect("load config"))
            .expect("build daemon state");

        assert_eq!(initial.wall_dir, first_wall_dir.path());
        assert_eq!(initial.interval, Duration::from_secs(15));

        write_config(&config_path, second_wall_dir.path(), 45);
        let reloaded = DaemonState::from_config(load_config(&config_path).expect("load config"))
            .expect("rebuild daemon state");

        assert_eq!(reloaded.wall_dir, second_wall_dir.path());
        assert_eq!(reloaded.interval, Duration::from_secs(45));
    }

    #[test]
    fn daemon_state_reload_rejects_invalid_config() {
        let config_dir = tempfile::tempdir().expect("tempdir");
        let config_path = config_dir.path().join("config.toml");
        fs::write(
            &config_path,
            "[slideshow]\ndirectory = \"/tmp\"\ninterval_seconds = 0\n",
        )
        .expect("write config");

        let err = load_config(&config_path).expect_err("invalid config should fail");
        assert!(err.to_string().contains("slideshow.interval_seconds"));
    }
}
