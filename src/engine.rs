use anyhow::{bail, Context, Result};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{ColorBackend, Config, WallpaperBackend};

// ===========================================================================
// CommandRunner trait — enables unit-testing without spawning real processes
// ===========================================================================

/// Abstraction over `std::process::Command` so callers can be tested without
/// actually invoking external binaries.
pub trait CommandRunner: Send + Sync {
    /// Run a program with the given arguments and return the captured output.
    fn run(&self, program: &str, args: &[&str]) -> Result<Output>;

    /// Run a shell snippet via `sh -c "<cmd>"` and return the captured output.
    fn run_shell(&self, cmd: &str) -> Result<Output>;

    /// Check whether a program is currently running (e.g. `pgrep -x <name>`).
    fn is_running(&self, process_name: &str) -> bool;
}

// ---------------------------------------------------------------------------
// Real (production) implementation
// ---------------------------------------------------------------------------

/// The default runner that actually spawns subprocesses.
pub struct RealCommandRunner {
    timeout: Duration,
}

pub struct DryRunCommandRunner;

pub enum AppCommandRunner {
    Real(RealCommandRunner),
    DryRun(DryRunCommandRunner),
}

impl RealCommandRunner {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    fn run_command(&self, mut command: Command, label: &str) -> Result<Output> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to execute: {label}"))?;

        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    return child
                        .wait_with_output()
                        .with_context(|| format!("Failed to collect output: {label}"));
                }
                Ok(None) if started.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("{label} timed out after {} seconds", self.timeout.as_secs());
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error)
                        .with_context(|| format!("Failed while waiting for: {label}"));
                }
            }
        }
    }
}

impl Default for RealCommandRunner {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl CommandRunner for DryRunCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        tracing::info!(program, ?args, "Dry-run exec");
        Ok(Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn run_shell(&self, cmd: &str) -> Result<Output> {
        tracing::info!(cmd, "Dry-run shell command");
        Ok(Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    fn is_running(&self, process_name: &str) -> bool {
        tracing::info!(process_name, "Dry-run process probe");
        false
    }
}

impl CommandRunner for AppCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        match self {
            AppCommandRunner::Real(runner) => runner.run(program, args),
            AppCommandRunner::DryRun(runner) => runner.run(program, args),
        }
    }

    fn run_shell(&self, cmd: &str) -> Result<Output> {
        match self {
            AppCommandRunner::Real(runner) => runner.run_shell(cmd),
            AppCommandRunner::DryRun(runner) => runner.run_shell(cmd),
        }
    }

    fn is_running(&self, process_name: &str) -> bool {
        match self {
            AppCommandRunner::Real(runner) => runner.is_running(process_name),
            AppCommandRunner::DryRun(runner) => runner.is_running(process_name),
        }
    }
}

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        tracing::debug!(program, ?args, "exec");
        let mut command = Command::new(program);
        command.args(args);
        self.run_command(command, program)
    }

    fn run_shell(&self, cmd: &str) -> Result<Output> {
        tracing::debug!(cmd, "sh -c");
        let mut command = Command::new("sh");
        command.arg("-c").arg(cmd);
        self.run_command(command, cmd)
    }

    fn is_running(&self, process_name: &str) -> bool {
        self.run("pgrep", &["-x", process_name])
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ===========================================================================
// Engine — orchestrates color extraction, wallpaper setting, and hooks
// ===========================================================================

pub struct Engine<'a, R: CommandRunner> {
    pub config: &'a Config,
    pub runner: &'a R,
}

impl<'a, R: CommandRunner> Engine<'a, R> {
    pub fn new(config: &'a Config, runner: &'a R) -> Self {
        Self { config, runner }
    }

    /// Full pipeline: colors → wallpaper → hooks.
    pub fn apply(&self, image_path: &Path) -> Result<()> {
        let path_str = image_path
            .to_str()
            .context("Image path is not valid UTF-8")?;

        self.apply_colors(path_str)?;
        self.apply_wallpaper(path_str)?;
        self.run_hooks()?;

        tracing::info!(path = path_str, "Theme applied successfully");
        Ok(())
    }

    // -- Color backend --------------------------------------------------------

    fn apply_colors(&self, path: &str) -> Result<()> {
        match self.config.backends.colors {
            ColorBackend::Wallust => {
                tracing::info!("Running wallust color extraction");
                let output = self.runner.run("wallust", &["run", "-q", path])?;
                check_output("wallust", &output)?;
            }
            ColorBackend::Matugen => {
                tracing::info!("Running matugen color extraction");
                let output = self.runner.run("matugen", &["image", path])?;
                check_output("matugen", &output)?;
            }
            ColorBackend::None => {
                tracing::info!("Color extraction disabled");
            }
        }
        Ok(())
    }

    // -- Wallpaper backend ----------------------------------------------------

    fn apply_wallpaper(&self, path: &str) -> Result<()> {
        match self.config.backends.wallpaper {
            WallpaperBackend::Swww => self.apply_swww(path),
            WallpaperBackend::Hyprpaper => self.apply_hyprpaper(path),
            WallpaperBackend::Wbg => self.apply_wbg(path),
        }
    }

    fn apply_swww(&self, path: &str) -> Result<()> {
        if !self.runner.is_running("swww-daemon") {
            self.start_swww_daemon()?;
        }

        let opts = &self.config.swww_options;
        let mut args = vec![
            "img".to_string(),
            path.to_string(),
            "--transition-type".to_string(),
            opts.transition_type.clone(),
            "--transition-step".to_string(),
            opts.transition_step.to_string(),
            "--transition-fps".to_string(),
            opts.transition_fps.to_string(),
        ];

        if let Some(duration) = opts.transition_duration {
            args.push("--transition-duration".to_string());
            args.push(duration.to_string());
        }

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        tracing::info!(backend = "swww", "Setting wallpaper");
        let output = self.runner.run("swww", &arg_refs)?;
        if !output.status.success() && is_recoverable_swww_failure(&output) {
            tracing::warn!("Recovering swww-daemon after wallpaper apply failure");
            self.recover_swww_daemon()?;
            let retry = self.runner.run("swww", &arg_refs)?;
            check_output("swww img", &retry)?;
            return Ok(());
        }
        check_output("swww img", &output)?;
        Ok(())
    }

    fn start_swww_daemon(&self) -> Result<()> {
        tracing::info!("swww-daemon not running — starting it");
        let output = self
            .runner
            .run_shell("nohup swww-daemon >/dev/null 2>&1 &")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(stderr = %stderr, "swww-daemon start returned non-zero");
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        Ok(())
    }

    fn recover_swww_daemon(&self) -> Result<()> {
        let _ = self
            .runner
            .run_shell("pkill -x swww-daemon >/dev/null 2>&1 || true");

        if let Some(socket_path) = swww_socket_path() {
            let cleanup = format!("rm -f -- {}", shell_quote(&socket_path));
            let _ = self.runner.run_shell(&cleanup);
        }

        self.start_swww_daemon()
    }

    fn apply_hyprpaper(&self, path: &str) -> Result<()> {
        tracing::info!(backend = "hyprpaper", "Setting wallpaper");

        // Preload the image
        let output = self
            .runner
            .run("hyprctl", &["hyprpaper", "preload", path])?;
        check_output("hyprpaper preload", &output)?;

        // Set the wallpaper on all monitors
        let wallpaper_arg = format!(",{path}");
        let output = self
            .runner
            .run("hyprctl", &["hyprpaper", "wallpaper", &wallpaper_arg])?;
        check_output("hyprpaper wallpaper", &output)?;

        Ok(())
    }

    fn apply_wbg(&self, path: &str) -> Result<()> {
        tracing::info!(backend = "wbg", "Setting wallpaper");
        let output = self.runner.run("wbg", &[path])?;
        check_output("wbg", &output)?;
        Ok(())
    }

    // -- Hooks ----------------------------------------------------------------

    fn run_hooks(&self) -> Result<()> {
        let hooks = &self.config.hooks.post_update;
        if hooks.is_empty() {
            tracing::debug!("No post-update hooks configured");
            return Ok(());
        }

        tracing::info!(count = hooks.len(), "Executing post-update hooks");

        for (i, hook) in hooks.iter().enumerate() {
            let cmd = self.build_hook_command(hook);
            tracing::debug!(index = i, cmd = %cmd, "Running hook");

            let output = self.runner.run_shell(&cmd)?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    index = i,
                    stderr = %stderr,
                    "Hook returned non-zero exit code"
                );
            }
        }

        Ok(())
    }

    /// Build the shell command for a hook.
    ///
    /// When wallust is the color backend, we source `~/.cache/wallust/colors.sh`
    /// *before* executing the hook so that shell variables like `$color2`,
    /// `$background`, etc. are available for expansion.
    fn build_hook_command(&self, hook: &str) -> String {
        match self.config.backends.colors {
            ColorBackend::Wallust => {
                let colors_sh = crate::config::expand_tilde("~/.cache/wallust/colors.sh");
                let colors_path = colors_sh.display();
                format!("if [ -f \"{colors_path}\" ]; then . \"{colors_path}\"; fi; {hook}")
            }
            _ => hook.to_string(),
        }
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Check the output of a subprocess and log/error on failure.
fn check_output(label: &str, output: &Output) -> Result<()> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "{label} failed (exit {})\nstdout: {stdout}\nstderr: {stderr}",
            output.status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn is_recoverable_swww_failure(output: &Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();

    [
        "received malformed answer from daemon",
        "failed to connect to socket",
        "connection refused",
        "address already in use",
        "socket file",
    ]
    .iter()
    .any(|needle| stdout.contains(needle) || stderr.contains(needle))
}

fn swww_socket_path() -> Option<String> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok()?;
    Some(format!("{runtime_dir}/{wayland_display}-awww-daemon.sock"))
}

fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BackendsConfig, Config, HooksConfig, SwwwOptions};
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::sync::Mutex;

    // -- Mock CommandRunner ---------------------------------------------------

    /// Records every command invocation for later assertion.
    #[derive(Debug)]
    struct Invocation {
        program: String,
        args: Vec<String>,
        is_shell: bool,
    }

    struct MockRunner {
        log: Mutex<Vec<Invocation>>,
        /// If true, `is_running` returns true (daemon already running).
        daemon_running: bool,
    }

    impl MockRunner {
        fn new(daemon_running: bool) -> Self {
            Self {
                log: Mutex::new(Vec::new()),
                daemon_running,
            }
        }

        fn invocations(&self) -> Vec<Invocation> {
            let guard = self.log.lock().unwrap();
            // We need to move data out; rebuild from the guard
            guard
                .iter()
                .map(|inv| Invocation {
                    program: inv.program.clone(),
                    args: inv.args.clone(),
                    is_shell: inv.is_shell,
                })
                .collect()
        }
    }

    fn mock_success() -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
            self.log.lock().unwrap().push(Invocation {
                program: program.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                is_shell: false,
            });
            Ok(mock_success())
        }

        fn run_shell(&self, cmd: &str) -> Result<Output> {
            self.log.lock().unwrap().push(Invocation {
                program: "sh".to_string(),
                args: vec!["-c".to_string(), cmd.to_string()],
                is_shell: true,
            });
            Ok(mock_success())
        }

        fn is_running(&self, _process_name: &str) -> bool {
            self.daemon_running
        }
    }

    // -- Color backend tests --------------------------------------------------

    #[test]
    fn wallust_generates_correct_command() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_colors("/tmp/test.jpg")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "wallust");
        assert_eq!(invocations[0].args, vec!["run", "-q", "/tmp/test.jpg"]);
    }

    #[test]
    fn matugen_generates_correct_command() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Matugen,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_colors("/tmp/test.jpg")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "matugen");
        assert_eq!(invocations[0].args, vec!["image", "/tmp/test.jpg"]);
    }

    #[test]
    fn color_none_runs_nothing() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_colors("/tmp/test.jpg")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert!(invocations.is_empty());
    }

    // -- Wallpaper backend tests ----------------------------------------------

    #[test]
    fn swww_with_daemon_running_generates_correct_command() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Swww,
                ..Default::default()
            },
            swww_options: SwwwOptions {
                transition_type: "fade".to_string(),
                transition_step: 50,
                transition_fps: 30,
                transition_duration: None,
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true); // daemon already running
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should succeed");

        let invocations = runner.invocations();
        // Should NOT have started the daemon
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "swww");
        assert_eq!(
            invocations[0].args,
            vec![
                "img",
                "/tmp/wall.png",
                "--transition-type",
                "fade",
                "--transition-step",
                "50",
                "--transition-fps",
                "30"
            ]
        );
    }

    #[test]
    fn swww_starts_daemon_when_not_running() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Swww,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(false); // daemon NOT running
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].program, "sh");
        assert!(invocations[0].args[1].contains("nohup swww-daemon >/dev/null 2>&1 &"));
        assert_eq!(invocations[1].program, "swww");
        assert_eq!(invocations[1].args[0], "img");
    }

    #[test]
    fn hyprpaper_generates_correct_commands() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Hyprpaper,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].program, "hyprctl");
        assert!(!invocations[0].is_shell);
        assert_eq!(
            invocations[0].args,
            vec!["hyprpaper", "preload", "/tmp/wall.png"]
        );
        assert_eq!(invocations[1].program, "hyprctl");
        assert!(!invocations[1].is_shell);
        assert_eq!(
            invocations[1].args,
            vec!["hyprpaper", "wallpaper", ",/tmp/wall.png"]
        );
    }

    #[test]
    fn wbg_generates_correct_command() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Wbg,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "wbg");
        assert_eq!(invocations[0].args, vec!["/tmp/wall.png"]);
    }

    // -- Hooks tests ----------------------------------------------------------

    #[test]
    fn hooks_with_wallust_source_colors_sh() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                ..Default::default()
            },
            hooks: HooksConfig {
                post_update: vec!["echo $color2".to_string()],
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine.run_hooks().expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert!(invocations[0].is_shell);

        let shell_cmd = &invocations[0].args[1];
        // Must source colors.sh before the hook
        assert!(
            shell_cmd.contains("colors.sh"),
            "hook must source colors.sh, got: {shell_cmd}"
        );
        assert!(
            shell_cmd.contains("echo $color2"),
            "hook body must be present, got: {shell_cmd}"
        );
        // The source must come BEFORE the hook body
        let source_pos = shell_cmd.find("colors.sh").unwrap();
        let hook_pos = shell_cmd.find("echo $color2").unwrap();
        assert!(source_pos < hook_pos, "source must precede hook body");
    }

    #[test]
    fn hooks_without_wallust_do_not_source_colors() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Matugen,
                ..Default::default()
            },
            hooks: HooksConfig {
                post_update: vec!["swaync-client -rs".to_string()],
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine.run_hooks().expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        let shell_cmd = &invocations[0].args[1];
        assert!(
            !shell_cmd.contains("colors.sh"),
            "non-wallust hooks must not source colors.sh"
        );
        assert_eq!(shell_cmd, "swaync-client -rs");
    }

    #[test]
    fn empty_hooks_run_nothing() {
        let config = Config::default();
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine.run_hooks().expect("should succeed");
        assert!(runner.invocations().is_empty());
    }

    #[test]
    fn multiple_hooks_execute_in_order() {
        let config = Config {
            hooks: HooksConfig {
                post_update: vec![
                    "echo first".to_string(),
                    "echo second".to_string(),
                    "echo third".to_string(),
                ],
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine.run_hooks().expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 3);
        // With wallust (default), each hook is prefixed with the source command
        assert!(invocations[0].args[1].contains("echo first"));
        assert!(invocations[1].args[1].contains("echo second"));
        assert!(invocations[2].args[1].contains("echo third"));
    }

    // -- Full pipeline test ---------------------------------------------------

    #[test]
    fn full_apply_pipeline_executes_in_order() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                wallpaper: WallpaperBackend::Swww,
            },
            hooks: HooksConfig {
                post_update: vec!["echo done".to_string()],
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true); // swww-daemon running
        let engine = Engine::new(&config, &runner);

        engine
            .apply(Path::new("/tmp/wall.jpg"))
            .expect("full pipeline should succeed");

        let invocations = runner.invocations();
        // wallust → swww img → hook
        assert_eq!(invocations.len(), 3);
        assert_eq!(invocations[0].program, "wallust");
        assert_eq!(invocations[1].program, "swww");
        assert!(invocations[2].is_shell);
        assert!(invocations[2].args[1].contains("echo done"));
    }

    // =========================================================================
    // Phase 5 — Extended rigorous integration / mock tests
    // =========================================================================

    // -- FailingMockRunner for error propagation tests ------------------------

    /// A runner that always returns a non-zero exit code.
    struct FailingMockRunner;

    struct RecoveringSwwwRunner {
        log: Mutex<Vec<Invocation>>,
        img_calls: Mutex<usize>,
    }

    struct TimeoutOnlyRunner {
        timeout: Duration,
    }

    fn mock_failure(stderr_msg: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(256), // exit code 1
            stdout: Vec::new(),
            stderr: stderr_msg.as_bytes().to_vec(),
        }
    }

    impl CommandRunner for FailingMockRunner {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<Output> {
            Ok(mock_failure("mock failure"))
        }
        fn run_shell(&self, _cmd: &str) -> Result<Output> {
            Ok(mock_failure("mock shell failure"))
        }
        fn is_running(&self, _name: &str) -> bool {
            true
        }
    }

    impl RecoveringSwwwRunner {
        fn new() -> Self {
            Self {
                log: Mutex::new(Vec::new()),
                img_calls: Mutex::new(0),
            }
        }

        fn invocations(&self) -> Vec<Invocation> {
            self.log
                .lock()
                .unwrap()
                .iter()
                .map(|inv| Invocation {
                    program: inv.program.clone(),
                    args: inv.args.clone(),
                    is_shell: inv.is_shell,
                })
                .collect()
        }
    }

    impl TimeoutOnlyRunner {
        fn new(timeout: Duration) -> Self {
            Self { timeout }
        }

        fn real(&self) -> RealCommandRunner {
            RealCommandRunner::new(self.timeout)
        }
    }

    impl CommandRunner for RecoveringSwwwRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
            self.log.lock().unwrap().push(Invocation {
                program: program.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                is_shell: false,
            });

            if program == "swww" {
                let mut calls = self.img_calls.lock().unwrap();
                *calls += 1;
                if *calls == 1 {
                    return Ok(mock_failure("Received malformed answer from daemon"));
                }
            }

            Ok(mock_success())
        }

        fn run_shell(&self, cmd: &str) -> Result<Output> {
            self.log.lock().unwrap().push(Invocation {
                program: "sh".to_string(),
                args: vec!["-c".to_string(), cmd.to_string()],
                is_shell: true,
            });
            Ok(mock_success())
        }

        fn is_running(&self, _process_name: &str) -> bool {
            true
        }
    }

    impl CommandRunner for TimeoutOnlyRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
            self.real().run(program, args)
        }

        fn run_shell(&self, cmd: &str) -> Result<Output> {
            self.real().run_shell(cmd)
        }

        fn is_running(&self, process_name: &str) -> bool {
            self.real().is_running(process_name)
        }
    }

    #[test]
    fn color_backend_failure_propagates_error() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = FailingMockRunner;
        let engine = Engine::new(&config, &runner);

        let result = engine.apply_colors("/tmp/test.jpg");
        assert!(result.is_err(), "color failure must propagate");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("wallust"),
            "error should mention wallust: {err_msg}"
        );
    }

    #[test]
    fn wallpaper_backend_failure_propagates_error() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Wbg,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = FailingMockRunner;
        let engine = Engine::new(&config, &runner);

        let result = engine.apply_wallpaper("/tmp/wall.jpg");
        assert!(result.is_err(), "wallpaper failure must propagate");
    }

    #[test]
    fn full_pipeline_stops_on_color_failure() {
        // If color extraction fails, wallpaper and hooks should NOT execute.
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                wallpaper: WallpaperBackend::Swww,
            },
            hooks: HooksConfig {
                post_update: vec!["echo should-not-run".to_string()],
            },
            ..Default::default()
        };
        let runner = FailingMockRunner;
        let engine = Engine::new(&config, &runner);

        let result = engine.apply(Path::new("/tmp/wall.jpg"));
        assert!(result.is_err(), "pipeline must fail on color error");
    }

    // -- Cross-backend combination tests --------------------------------------

    #[test]
    fn pipeline_matugen_with_hyprpaper() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Matugen,
                wallpaper: WallpaperBackend::Hyprpaper,
            },
            hooks: HooksConfig {
                post_update: vec!["swaync-client -rs".to_string()],
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply(Path::new("/tmp/wall.png"))
            .expect("should succeed");

        let invocations = runner.invocations();
        // matugen → hyprpaper preload → hyprpaper wallpaper → hook
        assert_eq!(invocations.len(), 4);
        assert_eq!(invocations[0].program, "matugen");
        assert_eq!(invocations[0].args, vec!["image", "/tmp/wall.png"]);
        assert_eq!(invocations[1].program, "hyprctl");
        assert!(!invocations[1].is_shell);
        assert_eq!(
            invocations[1].args,
            vec!["hyprpaper", "preload", "/tmp/wall.png"]
        );
        assert_eq!(invocations[2].program, "hyprctl");
        assert!(!invocations[2].is_shell);
        assert_eq!(
            invocations[2].args,
            vec!["hyprpaper", "wallpaper", ",/tmp/wall.png"]
        );
        // Hook should NOT source colors.sh (matugen, not wallust)
        assert_eq!(invocations[3].args[1], "swaync-client -rs");
    }

    #[test]
    fn pipeline_none_colors_with_wbg() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None,
                wallpaper: WallpaperBackend::Wbg,
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply(Path::new("/tmp/wall.jpg"))
            .expect("should succeed");

        let invocations = runner.invocations();
        // Only wbg, no color extraction, no hooks
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "wbg");
    }

    #[test]
    fn pipeline_wallust_with_wbg_and_hooks() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                wallpaper: WallpaperBackend::Wbg,
            },
            hooks: HooksConfig {
                post_update: vec!["echo $foreground".to_string()],
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply(Path::new("/img/test.png"))
            .expect("should succeed");

        let invocations = runner.invocations();
        // wallust → wbg → hook (with source)
        assert_eq!(invocations.len(), 3);
        assert_eq!(invocations[0].program, "wallust");
        assert_eq!(invocations[1].program, "wbg");
        assert!(invocations[2].args[1].contains("colors.sh"));
        assert!(invocations[2].args[1].contains("echo $foreground"));
    }

    // -- Hook command structure validation ------------------------------------

    #[test]
    fn hook_command_with_wallust_has_correct_structure() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        let cmd = engine
            .build_hook_command("hyprctl keyword general:col.active_border 'rgba(${color2:1}ff)'");

        // Must contain the conditional source
        assert!(cmd.contains("if [ -f"));
        assert!(cmd.contains("then ."));
        assert!(cmd.contains("colors.sh"));
        assert!(cmd.contains("fi;"));
        // Must preserve the hook body exactly
        assert!(cmd.contains("hyprctl keyword general:col.active_border 'rgba(${color2:1}ff)'"));
    }

    #[test]
    fn hook_command_with_matugen_is_passthrough() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Matugen,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        let cmd = engine.build_hook_command("echo test");
        assert_eq!(
            cmd, "echo test",
            "matugen hooks must pass through unchanged"
        );
    }

    #[test]
    fn hook_command_with_none_is_passthrough() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        let cmd = engine.build_hook_command("pkill dunst && dunst &");
        assert_eq!(cmd, "pkill dunst && dunst &");
    }

    #[test]
    fn hyprpaper_preload_and_wallpaper_exact_format() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Hyprpaper,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/home/user/Pictures/sunset.jpg")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations[0].program, "hyprctl");
        assert!(!invocations[0].is_shell);
        assert_eq!(
            invocations[0].args,
            vec!["hyprpaper", "preload", "/home/user/Pictures/sunset.jpg"]
        );
        assert_eq!(invocations[1].program, "hyprctl");
        assert!(!invocations[1].is_shell);
        assert_eq!(
            invocations[1].args,
            vec!["hyprpaper", "wallpaper", ",/home/user/Pictures/sunset.jpg"]
        );
    }

    #[test]
    fn hyprpaper_path_with_spaces_is_preserved_without_shell() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Hyprpaper,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/home/user/My Pictures/wall paper.jpg")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(invocations[0].program, "hyprctl");
        assert!(!invocations[0].is_shell);
        assert_eq!(
            invocations[0].args,
            vec![
                "hyprpaper",
                "preload",
                "/home/user/My Pictures/wall paper.jpg"
            ]
        );
        assert_eq!(invocations[1].program, "hyprctl");
        assert!(!invocations[1].is_shell);
        assert_eq!(
            invocations[1].args,
            vec![
                "hyprpaper",
                "wallpaper",
                ",/home/user/My Pictures/wall paper.jpg"
            ]
        );
    }

    // -- swww with default options --------------------------------------------

    #[test]
    fn swww_includes_transition_duration_when_configured() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Swww,
                ..Default::default()
            },
            swww_options: SwwwOptions {
                transition_type: "fade".to_string(),
                transition_step: 50,
                transition_fps: 30,
                transition_duration: Some(2),
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(
            invocations[0].args,
            vec![
                "img",
                "/tmp/wall.png",
                "--transition-type",
                "fade",
                "--transition-step",
                "50",
                "--transition-fps",
                "30",
                "--transition-duration",
                "2"
            ]
        );
    }

    #[test]
    fn swww_with_default_options() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Swww,
                ..Default::default()
            },
            // swww_options uses Default::default() → random, 100, 60
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should succeed");

        let invocations = runner.invocations();
        assert_eq!(
            invocations[0].args,
            vec![
                "img",
                "/tmp/wall.png",
                "--transition-type",
                "random",
                "--transition-step",
                "100",
                "--transition-fps",
                "60"
            ]
        );
    }

    #[test]
    fn swww_recovers_from_malformed_daemon_answer() {
        let config = Config {
            backends: BackendsConfig {
                wallpaper: WallpaperBackend::Swww,
                ..Default::default()
            },
            ..Default::default()
        };
        let runner = RecoveringSwwwRunner::new();
        let engine = Engine::new(&config, &runner);

        engine
            .apply_wallpaper("/tmp/wall.png")
            .expect("should recover and succeed");

        let invocations = runner.invocations();
        let swww_calls = invocations
            .iter()
            .filter(|inv| inv.program == "swww")
            .count();
        assert_eq!(swww_calls, 2);
        assert!(invocations
            .iter()
            .any(|inv| inv.program == "sh" && inv.args[1].contains("pkill -x swww-daemon")));
        assert!(invocations.iter().any(|inv| {
            inv.program == "sh" && inv.args[1].contains("nohup swww-daemon >/dev/null 2>&1 &")
        }));
        assert_eq!(
            invocations.last().map(|inv| inv.program.as_str()),
            Some("swww")
        );
    }

    #[test]
    fn real_runner_times_out_shell_command() {
        let runner = TimeoutOnlyRunner::new(Duration::from_millis(50));

        let result = runner.run_shell("sleep 1");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[test]
    fn real_runner_allows_short_shell_command() {
        let runner = TimeoutOnlyRunner::new(Duration::from_secs(1));

        let output = runner
            .run_shell("printf ok")
            .expect("short command should succeed");

        assert_eq!(String::from_utf8_lossy(&output.stdout), "ok");
    }

    #[test]
    fn real_runner_times_out_direct_command() {
        let runner = TimeoutOnlyRunner::new(Duration::from_millis(50));

        let result = runner.run("sh", &["-c", "sleep 1"]);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    // -- Path with spaces -----------------------------------------------------

    #[test]
    fn wallpaper_path_with_spaces_is_preserved() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::Wallust,
                wallpaper: WallpaperBackend::Swww,
            },
            ..Default::default()
        };
        let runner = MockRunner::new(true);
        let engine = Engine::new(&config, &runner);

        engine
            .apply(Path::new("/home/user/My Pictures/wall paper.jpg"))
            .expect("should succeed");

        let invocations = runner.invocations();
        // wallust
        assert_eq!(
            invocations[0].args[2],
            "/home/user/My Pictures/wall paper.jpg"
        );
        // swww img
        assert_eq!(
            invocations[1].args[1],
            "/home/user/My Pictures/wall paper.jpg"
        );
    }

    // -- All hooks execute even when one returns non-zero ---------------------

    /// A runner where run_shell alternates success/failure.
    struct AlternatingMockRunner {
        call_count: Mutex<usize>,
    }

    impl AlternatingMockRunner {
        fn new() -> Self {
            Self {
                call_count: Mutex::new(0),
            }
        }
    }

    impl CommandRunner for AlternatingMockRunner {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<Output> {
            Ok(mock_success())
        }
        fn run_shell(&self, _cmd: &str) -> Result<Output> {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            if (*count).is_multiple_of(2) {
                Ok(mock_failure("intentional failure"))
            } else {
                Ok(mock_success())
            }
        }
        fn is_running(&self, _name: &str) -> bool {
            true
        }
    }

    #[test]
    fn hooks_continue_after_non_zero_exit() {
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None, // skip color source prefix
                ..Default::default()
            },
            hooks: HooksConfig {
                post_update: vec![
                    "echo first".to_string(),  // succeeds (call 1)
                    "echo second".to_string(), // fails (call 2)
                    "echo third".to_string(),  // succeeds (call 3)
                ],
            },
            ..Default::default()
        };
        let runner = AlternatingMockRunner::new();
        let engine = Engine::new(&config, &runner);

        // Should NOT error — non-zero hooks are warned but don't abort
        engine
            .run_hooks()
            .expect("hooks should not abort on non-zero exit");

        let count = *runner.call_count.lock().unwrap();
        assert_eq!(count, 3, "all 3 hooks must execute even if one fails");
    }
}
