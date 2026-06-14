use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{ColorBackend, Config, WallpaperBackend};
use crate::control::{SchedulerState, SchedulerStatus};
use crate::engine::CommandRunner;
use crate::slideshow::{discover_images, last_image_state_path};

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistorySummary {
    current: Option<PathBuf>,
    entries: usize,
}

pub fn render_status_with_remaining<R: CommandRunner>(
    config_path: &Path,
    config: &Config,
    runner: &R,
    scheduler_status: Option<SchedulerStatus>,
) -> String {
    let wall_dir = config.wallpaper_dir();
    let history_path = history_state_path();
    let history = read_history_summary(&history_path, &wall_dir);

    build_status_report(
        config_path,
        config,
        runner,
        &wall_dir,
        &history_path,
        history,
        scheduler_status,
    )
}

fn build_status_report<R: CommandRunner>(
    config_path: &Path,
    config: &Config,
    runner: &R,
    wall_dir: &Path,
    history_path: &Path,
    history: HistorySummary,
    scheduler_status: Option<SchedulerStatus>,
) -> String {
    let config_source = if config_path.exists() {
        "file"
    } else {
        "defaults"
    };

    let directory_status = match discover_images(wall_dir, config.slideshow.recursive) {
        Ok(images) => format!("ok ({})", pluralize(images.len(), "supported image")),
        Err(_) if !wall_dir.exists() => "missing".to_string(),
        Err(_) if !wall_dir.is_dir() => "not a directory".to_string(),
        Err(error) => format!("error ({error})"),
    };

    let daemon_status = match config.backends.wallpaper {
        WallpaperBackend::Swww => yes_no(runner.is_running("swww-daemon")).to_string(),
        _ => "n/a".to_string(),
    };

    let current_wallpaper = history
        .current
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "none".to_string());

    let slideshow_state = scheduler_status
        .map(|status| match status.state {
            SchedulerState::Running => "running".to_string(),
            SchedulerState::Paused => "paused".to_string(),
        })
        .unwrap_or_else(|| "n/a".to_string());

    let remaining = scheduler_status
        .map(|status| format_remaining_seconds(status.remaining_seconds))
        .unwrap_or_else(|| "n/a".to_string());

    format!(
        concat!(
            "temad status\n",
            "Config path: {}\n",
            "Config source: {}\n",
            "Wallpaper directory: {}\n",
            "Wallpaper directory status: {}\n",
            "Interval seconds: {}\n",
            "Randomize: {}\n",
            "Color backend: {}\n",
            "Wallpaper backend: {}\n",
            "Slideshow state: {}\n",
            "swww-daemon running: {}\n",
            "Time until next wallpaper: {}\n",
            "Post-update hooks: {}\n",
            "State file: {}\n",
            "History entries: {}\n",
            "Current wallpaper: {}\n"
        ),
        config_path.display(),
        config_source,
        wall_dir.display(),
        directory_status,
        config.slideshow.interval_seconds,
        yes_no(config.slideshow.randomize),
        color_backend_label(&config.backends.colors),
        wallpaper_backend_label(&config.backends.wallpaper),
        slideshow_state,
        daemon_status,
        remaining,
        config.hooks.post_update.len(),
        history_path.display(),
        history.entries,
        current_wallpaper,
    )
}

fn format_remaining_seconds(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

fn history_state_path() -> PathBuf {
    last_image_state_path()
}

fn read_history_summary(state_path: &Path, wall_dir: &Path) -> HistorySummary {
    let raw = match fs::read_to_string(state_path) {
        Ok(raw) => raw,
        Err(_) => {
            return HistorySummary {
                current: None,
                entries: 0,
            }
        }
    };

    parse_history_summary(wall_dir, &raw)
}

fn parse_history_summary(wall_dir: &Path, raw: &str) -> HistorySummary {
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.is_empty() {
        return HistorySummary {
            current: None,
            entries: 0,
        };
    }

    if let Ok(cursor) = lines[0].parse::<usize>() {
        let entries: Vec<PathBuf> = lines[1..]
            .iter()
            .filter_map(|line| decode_history_entry(wall_dir, line))
            .collect();

        if entries.is_empty() {
            return HistorySummary {
                current: None,
                entries: 0,
            };
        }

        let current = entries.get(cursor.min(entries.len() - 1)).cloned();
        return HistorySummary {
            current,
            entries: entries.len(),
        };
    }

    HistorySummary {
        current: decode_history_entry(wall_dir, lines[0]),
        entries: 1,
    }
}

fn decode_history_entry(wall_dir: &Path, raw: &str) -> Option<PathBuf> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }

    let path = PathBuf::from(value);
    Some(if path.is_absolute() {
        path
    } else {
        wall_dir.join(path)
    })
}

fn color_backend_label(backend: &ColorBackend) -> &'static str {
    match backend {
        ColorBackend::Wallust => "wallust",
        ColorBackend::Matugen => "matugen",
        ColorBackend::None => "none",
    }
}

fn wallpaper_backend_label(backend: &WallpaperBackend) -> &'static str {
    match backend {
        WallpaperBackend::Swww => "swww",
        WallpaperBackend::Hyprpaper => "hyprpaper",
        WallpaperBackend::Wbg => "wbg",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn pluralize(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BackendsConfig, HooksConfig, SlideshowConfig};
    use anyhow::Result;
    use std::process::Output;

    struct StatusRunner {
        running: bool,
    }

    impl CommandRunner for StatusRunner {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<Output> {
            unreachable!("status should not execute external commands")
        }

        fn run_shell(&self, _cmd: &str) -> Result<Output> {
            unreachable!("status should not execute shell commands")
        }

        fn is_running(&self, _process_name: &str) -> bool {
            self.running
        }
    }

    #[test]
    fn parse_history_summary_supports_bounded_history_format() {
        let wall_dir = Path::new("/walls");
        let summary = parse_history_summary(wall_dir, "1\na.jpg\nb.jpg\n");

        assert_eq!(summary.entries, 2);
        assert_eq!(summary.current, Some(wall_dir.join("b.jpg")));
    }

    #[test]
    fn parse_history_summary_supports_legacy_single_line_format() {
        let wall_dir = Path::new("/walls");
        let summary = parse_history_summary(wall_dir, "legacy.png\n");

        assert_eq!(summary.entries, 1);
        assert_eq!(summary.current, Some(wall_dir.join("legacy.png")));
    }

    #[test]
    fn build_status_report_describes_available_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join("one.jpg"), b"image").expect("write image");
        fs::write(temp.path().join("two.png"), b"image").expect("write image");

        let config = Config {
            slideshow: SlideshowConfig {
                directory: temp.path().display().to_string(),
                interval_seconds: 120,
                randomize: true,
                history_limit: 32,
                recursive: false,
            },
            backends: BackendsConfig::default(),
            hooks: HooksConfig {
                post_update: vec!["echo reload".to_string(), "echo sync".to_string()],
            },
            ..Default::default()
        };

        let report = build_status_report(
            Path::new("/tmp/temad.toml"),
            &config,
            &StatusRunner { running: true },
            temp.path(),
            Path::new("/tmp/state/last_image"),
            HistorySummary {
                current: Some(temp.path().join("two.png")),
                entries: 3,
            },
            Some(SchedulerStatus {
                state: SchedulerState::Running,
                remaining_seconds: 125,
            }),
        );

        assert!(report.contains("Config source: defaults"));
        assert!(report.contains("Wallpaper directory status: ok (2 supported images)"));
        assert!(report.contains("Randomize: yes"));
        assert!(report.contains("Color backend: wallust"));
        assert!(report.contains("Wallpaper backend: swww"));
        assert!(report.contains("Slideshow state: running"));
        assert!(report.contains("swww-daemon running: yes"));
        assert!(report.contains("Time until next wallpaper: 2m 5s"));
        assert!(report.contains("Post-update hooks: 2"));
        assert!(report.contains("History entries: 3"));
        assert!(report.contains("Current wallpaper: "));
        assert!(report.contains("two.png"));
    }

    #[test]
    fn build_status_report_handles_missing_directory() {
        let config = Config {
            slideshow: SlideshowConfig {
                directory: "/tmp/temad-missing-dir".to_string(),
                ..Default::default()
            },
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Wbg,
                ..Default::default()
            },
            ..Default::default()
        };

        let report = build_status_report(
            Path::new("/tmp/present.toml"),
            &config,
            &StatusRunner { running: false },
            Path::new("/tmp/temad-missing-dir"),
            Path::new("/tmp/state/last_image"),
            HistorySummary {
                current: None,
                entries: 0,
            },
            None,
        );

        assert!(report.contains("Wallpaper directory status: missing"));
        assert!(report.contains("Wallpaper backend: wbg"));
        assert!(report.contains("Slideshow state: n/a"));
        assert!(report.contains("swww-daemon running: n/a"));
        assert!(report.contains("Time until next wallpaper: n/a"));
        assert!(report.contains("Current wallpaper: none"));
    }

    #[test]
    fn build_status_report_shows_paused_state() {
        let config = Config::default();
        let report = build_status_report(
            Path::new("/tmp/temad.toml"),
            &config,
            &StatusRunner { running: false },
            Path::new("/tmp/walls"),
            Path::new("/tmp/state/last_image"),
            HistorySummary {
                current: None,
                entries: 0,
            },
            Some(SchedulerStatus {
                state: SchedulerState::Paused,
                remaining_seconds: 7,
            }),
        );

        assert!(report.contains("Slideshow state: paused"));
        assert!(report.contains("Time until next wallpaper: 7s"));
    }

    #[test]
    fn format_remaining_seconds_formats_hours_minutes_and_seconds() {
        assert_eq!(format_remaining_seconds(5), "5s");
        assert_eq!(format_remaining_seconds(125), "2m 5s");
        assert_eq!(format_remaining_seconds(3723), "1h 2m 3s");
    }
}
