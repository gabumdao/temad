use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{ColorBackend, Config, WallpaperBackend};
use crate::engine::CommandRunner;
use crate::slideshow::discover_images;

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistorySummary {
    current: Option<PathBuf>,
    entries: usize,
}

pub fn render_status<R: CommandRunner>(config_path: &Path, config: &Config, runner: &R) -> String {
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
    )
}

fn build_status_report<R: CommandRunner>(
    config_path: &Path,
    config: &Config,
    runner: &R,
    wall_dir: &Path,
    history_path: &Path,
    history: HistorySummary,
) -> String {
    let config_source = if config_path.exists() {
        "file"
    } else {
        "defaults"
    };

    let directory_status = match discover_images(wall_dir) {
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
            "swww-daemon running: {}\n",
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
        daemon_status,
        config.hooks.post_update.len(),
        history_path.display(),
        history.entries,
        current_wallpaper,
    )
}

fn history_state_path() -> PathBuf {
    let state_root = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));

    state_root.join("temad").join("last_image")
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
        );

        assert!(report.contains("Config source: defaults"));
        assert!(report.contains("Wallpaper directory status: ok (2 supported images)"));
        assert!(report.contains("Randomize: yes"));
        assert!(report.contains("Color backend: wallust"));
        assert!(report.contains("Wallpaper backend: swww"));
        assert!(report.contains("swww-daemon running: yes"));
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
        );

        assert!(report.contains("Wallpaper directory status: missing"));
        assert!(report.contains("Wallpaper backend: wbg"));
        assert!(report.contains("swww-daemon running: n/a"));
        assert!(report.contains("Current wallpaper: none"));
    }
}
