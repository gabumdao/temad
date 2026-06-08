use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config::{load_config, ColorBackend, WallpaperBackend};
use crate::engine::CommandRunner;
use crate::slideshow::discover_images;

#[derive(Debug, Clone, PartialEq, Eq)]
enum CheckLevel {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Check {
    label: &'static str,
    level: CheckLevel,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    config_path: PathBuf,
    checks: Vec<Check>,
}

impl DoctorReport {
    fn is_healthy(&self) -> bool {
        self.checks
            .iter()
            .all(|check| check.level != CheckLevel::Fail)
    }
}

pub fn run_doctor<R: CommandRunner>(config_path: &Path, runner: &R) -> Result<bool> {
    let report = inspect_doctor(config_path, runner);
    print!("{}", render_report(&report));
    Ok(report.is_healthy())
}

fn inspect_doctor<R: CommandRunner>(config_path: &Path, runner: &R) -> DoctorReport {
    let mut checks = Vec::new();

    match load_config(config_path) {
        Ok(config) => {
            let config_detail = if config_path.exists() {
                format!("loaded {}", config_path.display())
            } else {
                format!("missing {}; using defaults", config_path.display())
            };
            checks.push(Check {
                label: "Config",
                level: if config_path.exists() {
                    CheckLevel::Ok
                } else {
                    CheckLevel::Warn
                },
                detail: config_detail,
            });

            let wall_dir = config.wallpaper_dir();
            checks.push(check_wallpaper_directory(
                &wall_dir,
                config.slideshow.recursive,
            ));
            checks.push(check_color_backend(&config.backends.colors, runner));
            checks.extend(check_wallpaper_backend(&config.backends.wallpaper, runner));
            checks.push(Check {
                label: "Hooks",
                level: CheckLevel::Ok,
                detail: format!("{} configured", config.hooks.post_update.len()),
            });
        }
        Err(error) => {
            checks.push(Check {
                label: "Config",
                level: CheckLevel::Fail,
                detail: format!("{} ({error})", config_path.display()),
            });
        }
    }

    DoctorReport {
        config_path: config_path.to_path_buf(),
        checks,
    }
}

fn check_wallpaper_directory(path: &Path, recursive: bool) -> Check {
    match discover_images(path, recursive) {
        Ok(images) if images.is_empty() => Check {
            label: "Wallpaper directory",
            level: CheckLevel::Fail,
            detail: format!("{} has no supported images", path.display()),
        },
        Ok(images) => Check {
            label: "Wallpaper directory",
            level: CheckLevel::Ok,
            detail: format!("{} supported image(s) in {}", images.len(), path.display()),
        },
        Err(error) => Check {
            label: "Wallpaper directory",
            level: CheckLevel::Fail,
            detail: format!("{} ({error})", path.display()),
        },
    }
}

fn check_color_backend<R: CommandRunner>(backend: &ColorBackend, runner: &R) -> Check {
    match backend {
        ColorBackend::Wallust => check_binary("Color backend", "wallust", runner),
        ColorBackend::Matugen => check_binary("Color backend", "matugen", runner),
        ColorBackend::None => Check {
            label: "Color backend",
            level: CheckLevel::Ok,
            detail: "disabled".to_string(),
        },
    }
}

fn check_wallpaper_backend<R: CommandRunner>(backend: &WallpaperBackend, runner: &R) -> Vec<Check> {
    match backend {
        WallpaperBackend::Swww => vec![
            check_binary("Wallpaper backend", "swww", runner),
            check_binary("swww-daemon binary", "swww-daemon", runner),
            Check {
                label: "swww-daemon process",
                level: if runner.is_running("swww-daemon") {
                    CheckLevel::Ok
                } else {
                    CheckLevel::Warn
                },
                detail: if runner.is_running("swww-daemon") {
                    "running".to_string()
                } else {
                    "not running; temad can start it on demand".to_string()
                },
            },
        ],
        WallpaperBackend::Hyprpaper => {
            vec![check_binary("Wallpaper backend", "hyprctl", runner)]
        }
        WallpaperBackend::Wbg => vec![check_binary("Wallpaper backend", "wbg", runner)],
    }
}

fn check_binary<R: CommandRunner>(label: &'static str, program: &str, runner: &R) -> Check {
    match runner.run_shell(&format!("command -v {program} >/dev/null 2>&1")) {
        Ok(output) if output.status.success() => Check {
            label,
            level: CheckLevel::Ok,
            detail: format!("{program} available"),
        },
        Ok(_) => Check {
            label,
            level: CheckLevel::Fail,
            detail: format!("{program} not found"),
        },
        Err(error) => Check {
            label,
            level: CheckLevel::Fail,
            detail: format!("{program} check failed ({error})"),
        },
    }
}

fn render_report(report: &DoctorReport) -> String {
    let mut output = String::new();
    output.push_str("temad doctor\n");
    output.push_str(&format!("Config path: {}\n", report.config_path.display()));
    for check in &report.checks {
        output.push_str(&format!(
            "[{}] {}: {}\n",
            level_label(&check.level),
            check.label,
            check.detail
        ));
    }
    output.push_str(&format!(
        "Overall: {}\n",
        if report.is_healthy() {
            "healthy"
        } else {
            "attention needed"
        }
    ));
    output
}

fn level_label(level: &CheckLevel) -> &'static str {
    match level {
        CheckLevel::Ok => "OK",
        CheckLevel::Warn => "WARN",
        CheckLevel::Fail => "FAIL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::collections::HashSet;
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    struct DoctorRunner {
        available: HashSet<String>,
        running: HashSet<String>,
        fail_shell: bool,
    }

    impl DoctorRunner {
        fn new(available: &[&str], running: &[&str]) -> Self {
            Self {
                available: available.iter().map(|item| item.to_string()).collect(),
                running: running.iter().map(|item| item.to_string()).collect(),
                fail_shell: false,
            }
        }
    }

    impl CommandRunner for DoctorRunner {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<Output> {
            Err(anyhow!("doctor should not execute direct commands"))
        }

        fn run_shell(&self, cmd: &str) -> Result<Output> {
            if self.fail_shell {
                return Err(anyhow!("shell unavailable"));
            }

            let program = cmd
                .strip_prefix("command -v ")
                .and_then(|rest| rest.strip_suffix(" >/dev/null 2>&1"))
                .expect("doctor binary probe format");

            Ok(Output {
                status: if self.available.contains(program) {
                    ExitStatus::from_raw(0)
                } else {
                    ExitStatus::from_raw(256)
                },
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }

        fn is_running(&self, process_name: &str) -> bool {
            self.running.contains(process_name)
        }
    }

    #[test]
    fn report_is_healthy_when_required_checks_pass() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "[slideshow]\ndirectory = \"{}\"\n\n[backends]\ncolors = \"wallust\"\nwallpaper = \"swww\"\n",
                dir.path().display()
            ),
        )
        .expect("write config");
        fs::write(dir.path().join("wall.jpg"), b"img").expect("write image");

        let report = inspect_doctor(
            &config_path,
            &DoctorRunner::new(&["wallust", "swww", "swww-daemon"], &["swww-daemon"]),
        );

        assert!(report.is_healthy());
        let rendered = render_report(&report);
        assert!(rendered.contains("[OK] Config"));
        assert!(rendered.contains("[OK] Wallpaper directory"));
        assert!(rendered.contains("[OK] Color backend: wallust available"));
        assert!(rendered.contains("[OK] swww-daemon process: running"));
        assert!(rendered.contains("Overall: healthy"));
    }

    #[test]
    fn missing_config_warns_and_uses_defaults() {
        let report = inspect_doctor(
            Path::new("/tmp/temad-doctor-missing-config.toml"),
            &DoctorRunner::new(&[], &[]),
        );

        assert!(!report.is_healthy());
        let rendered = render_report(&report);
        assert!(rendered.contains(
            "[WARN] Config: missing /tmp/temad-doctor-missing-config.toml; using defaults"
        ));
        assert!(rendered.contains("Overall: attention needed"));
    }

    #[test]
    fn malformed_config_fails_without_running_other_checks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        fs::write(&config_path, "not valid toml [[[ ").expect("write config");

        let report = inspect_doctor(&config_path, &DoctorRunner::new(&[], &[]));

        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].level, CheckLevel::Fail);
        assert!(render_report(&report).contains("[FAIL] Config:"));
    }

    #[test]
    fn empty_wallpaper_directory_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "[slideshow]\ndirectory = \"{}\"\n\n[backends]\ncolors = \"none\"\nwallpaper = \"wbg\"\n",
                dir.path().display()
            ),
        )
        .expect("write config");

        let report = inspect_doctor(&config_path, &DoctorRunner::new(&["wbg"], &[]));

        assert!(!report.is_healthy());
        assert!(render_report(&report).contains("[FAIL] Wallpaper directory:"));
        assert!(render_report(&report).contains("has no supported images"));
    }

    #[test]
    fn missing_backend_dependency_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("wall.png"), b"img").expect("write image");
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "[slideshow]\ndirectory = \"{}\"\n\n[backends]\ncolors = \"matugen\"\nwallpaper = \"wbg\"\n",
                dir.path().display()
            ),
        )
        .expect("write config");

        let report = inspect_doctor(&config_path, &DoctorRunner::new(&["wbg"], &[]));

        assert!(!report.is_healthy());
        assert!(render_report(&report).contains("[FAIL] Color backend: matugen not found"));
    }

    #[test]
    fn color_none_does_not_require_binary() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("wall.png"), b"img").expect("write image");
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "[slideshow]\ndirectory = \"{}\"\n\n[backends]\ncolors = \"none\"\nwallpaper = \"wbg\"\n",
                dir.path().display()
            ),
        )
        .expect("write config");

        let report = inspect_doctor(&config_path, &DoctorRunner::new(&["wbg"], &[]));
        let rendered = render_report(&report);

        assert!(report.is_healthy());
        assert!(rendered.contains("[OK] Color backend: disabled"));
    }

    #[test]
    fn swww_daemon_not_running_is_warning_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("wall.png"), b"img").expect("write image");
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "[slideshow]\ndirectory = \"{}\"\n\n[backends]\ncolors = \"wallust\"\nwallpaper = \"swww\"\n",
                dir.path().display()
            ),
        )
        .expect("write config");

        let report = inspect_doctor(
            &config_path,
            &DoctorRunner::new(&["wallust", "swww", "swww-daemon"], &[]),
        );

        assert!(report.is_healthy());
        let rendered = render_report(&report);
        assert!(rendered
            .contains("[WARN] swww-daemon process: not running; temad can start it on demand"));
        assert!(rendered.contains("Overall: healthy"));
    }
}
