use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::engine::{CommandRunner, Engine};

use super::discovery::discover_images;
use super::history_state::{last_image_state_path, load_history, with_locked_history_from};
use super::selector::ImageSelector;

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

pub(crate) async fn run_daemon<R: CommandRunner>(
    config: &Config,
    runner: &R,
    dry_run: bool,
) -> Result<()> {
    let wall_dir = config.wallpaper_dir();
    let images = discover_images(&wall_dir, config.slideshow.recursive)?;

    tracing::info!(
        count = images.len(),
        dir = %wall_dir.display(),
        "Image pool ready"
    );

    let mut history = match load_history(&wall_dir) {
        Ok(history) => history,
        Err(error) => {
            tracing::warn!(error = %error, dir = %wall_dir.display(), "Failed to load wallpaper history state");
            Default::default()
        }
    };

    let mut selector = match history.current_cloned() {
        Some(last) => ImageSelector::with_last(images, config.slideshow.randomize, Some(last))?,
        None => ImageSelector::new(images, config.slideshow.randomize)?,
    };
    let engine = Engine::new(config, runner);
    let interval = std::time::Duration::from_secs(config.slideshow.interval_seconds);

    if dry_run {
        let _ = run_daemon_cycle(&wall_dir, &mut selector, &engine);
        return Ok(());
    }

    loop {
        if let Some(image) = run_daemon_cycle(&wall_dir, &mut selector, &engine) {
            history.record(image.clone(), config.slideshow.history_limit);
            let latest_image = image.clone();
            if let Err(error) =
                with_locked_history_from(&last_image_state_path(), &wall_dir, |state| {
                    state.record(latest_image.clone(), config.slideshow.history_limit);
                    Ok(())
                })
            {
                tracing::warn!(error = %error, path = %image.display(), "Failed to persist wallpaper history state");
            }
        }

        tracing::debug!(seconds = config.slideshow.interval_seconds, "Sleeping");
        tokio::time::sleep(interval).await;
    }
}
