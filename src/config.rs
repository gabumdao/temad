use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Top-level Config
// ---------------------------------------------------------------------------

/// Root configuration structure mapping to `~/.config/temad/config.toml`.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    pub slideshow: SlideshowConfig,
    pub backends: BackendsConfig,
    pub swww_options: SwwwOptions,
    pub commands: CommandsConfig,
    pub hooks: HooksConfig,
}

// ---------------------------------------------------------------------------
// Slideshow
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct SlideshowConfig {
    /// Directory containing wallpaper images.
    /// Supports `~` expansion.
    pub directory: String,

    /// Seconds between wallpaper changes.
    pub interval_seconds: u64,

    /// Whether to randomize image selection order.
    pub randomize: bool,

    pub history_limit: usize,

    pub recursive: bool,
}

impl Default for SlideshowConfig {
    fn default() -> Self {
        Self {
            directory: "~/Pictures/Wallpapers".to_string(),
            interval_seconds: 300,
            randomize: true,
            history_limit: 32,
            recursive: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct CommandsConfig {
    pub timeout_seconds: u64,
}

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// Which color and wallpaper engines to use.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct BackendsConfig {
    /// Color extraction backend: "wallust", "matugen", or "none".
    pub colors: ColorBackend,

    /// Wallpaper setting backend: "swww", "hyprpaper", or "wbg".
    pub wallpaper: WallpaperBackend,
}

impl Default for BackendsConfig {
    fn default() -> Self {
        Self {
            colors: ColorBackend::Wallust,
            wallpaper: WallpaperBackend::Swww,
        }
    }
}

/// Supported color extraction backends.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ColorBackend {
    Wallust,
    Matugen,
    None,
}

/// Supported wallpaper setting backends.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperBackend {
    Swww,
    Hyprpaper,
    Wbg,
}

// ---------------------------------------------------------------------------
// swww options
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct SwwwOptions {
    pub transition_type: String,
    pub transition_step: u32,
    pub transition_fps: u32,
    pub transition_duration: Option<u32>,
}

impl Default for SwwwOptions {
    fn default() -> Self {
        Self {
            transition_type: "random".to_string(),
            transition_step: 100,
            transition_fps: 60,
            transition_duration: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct HooksConfig {
    /// Shell commands executed AFTER colors and wallpaper are applied.
    pub post_update: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Expand a leading `~` or `~user` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Resolve the config file path.
///
/// Priority:
///   1. Explicit `--config <path>` flag.
///   2. `$XDG_CONFIG_HOME/temad/config.toml`  (usually `~/.config/temad/config.toml`).
///   3. `~/.config/temad/config.toml` as a hard fallback.
pub fn resolve_config_path(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }

    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("temad").join("config.toml");
    }

    expand_tilde("~/.config/temad/config.toml")
}

/// Load, parse, and validate the TOML configuration file.
///
/// If the file does not exist, returns `Config::default()` so the daemon
/// can still run with sensible defaults.
pub fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        tracing::warn!(
            path = %path.display(),
            "Config file not found — falling back to defaults"
        );
        return Ok(Config::default());
    }

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&raw)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

    if config.slideshow.interval_seconds == 0 {
        bail!("slideshow.interval_seconds must be greater than 0");
    }

    if config.slideshow.history_limit == 0 {
        bail!("slideshow.history_limit must be greater than 0");
    }

    if config.commands.timeout_seconds == 0 {
        bail!("commands.timeout_seconds must be greater than 0");
    }

    tracing::info!(path = %path.display(), "Loaded configuration");
    Ok(config)
}

/// Return the expanded wallpaper directory as a `PathBuf`.
impl Config {
    /// Convenience: get the slideshow directory with `~` expanded.
    pub fn wallpaper_dir(&self) -> PathBuf {
        expand_tilde(&self.slideshow.directory)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Path expansion -------------------------------------------------------

    #[test]
    fn expand_tilde_home() {
        let expanded = expand_tilde("~/Pictures/Wallpapers");
        let home = dirs::home_dir().expect("could not determine home dir");
        assert_eq!(expanded, home.join("Pictures/Wallpapers"));
    }

    #[test]
    fn expand_tilde_bare() {
        let expanded = expand_tilde("~");
        let home = dirs::home_dir().expect("could not determine home dir");
        assert_eq!(expanded, home);
    }

    #[test]
    fn expand_tilde_absolute_passthrough() {
        let expanded = expand_tilde("/absolute/path");
        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn expand_tilde_relative_passthrough() {
        let expanded = expand_tilde("relative/path");
        assert_eq!(expanded, PathBuf::from("relative/path"));
    }

    // -- Full TOML parsing ----------------------------------------------------

    #[test]
    fn parse_full_config() {
        let toml_str = r#"
[slideshow]
directory = "~/Pictures/Walls"
interval_seconds = 600
randomize = false
history_limit = 16
recursive = true

[backends]
colors = "matugen"
wallpaper = "hyprpaper"

[swww_options]
transition_type = "fade"
transition_step = 50
transition_fps = 30
transition_duration = 2

[hooks]
post_update = [
    "echo hello",
    "notify-send 'wallpaper changed'"
]

[commands]
timeout_seconds = 12
"#;

        let config: Config = toml::from_str(toml_str).expect("valid TOML");

        assert_eq!(config.slideshow.directory, "~/Pictures/Walls");
        assert_eq!(config.slideshow.interval_seconds, 600);
        assert!(!config.slideshow.randomize);
        assert_eq!(config.slideshow.history_limit, 16);
        assert!(config.slideshow.recursive);

        assert_eq!(config.backends.colors, ColorBackend::Matugen);
        assert_eq!(config.backends.wallpaper, WallpaperBackend::Hyprpaper);

        assert_eq!(config.swww_options.transition_type, "fade");
        assert_eq!(config.swww_options.transition_step, 50);
        assert_eq!(config.swww_options.transition_fps, 30);
        assert_eq!(config.swww_options.transition_duration, Some(2));

        assert_eq!(config.hooks.post_update.len(), 2);
        assert_eq!(config.hooks.post_update[0], "echo hello");
        assert_eq!(config.commands.timeout_seconds, 12);
    }

    // -- Default fallbacks ----------------------------------------------------

    #[test]
    fn parse_empty_config_uses_defaults() {
        let config: Config = toml::from_str("").expect("empty TOML should parse");

        assert_eq!(config.slideshow.directory, "~/Pictures/Wallpapers");
        assert_eq!(config.slideshow.interval_seconds, 300);
        assert!(config.slideshow.randomize);
        assert_eq!(config.slideshow.history_limit, 32);
        assert!(!config.slideshow.recursive);
        assert_eq!(config.backends.colors, ColorBackend::Wallust);
        assert_eq!(config.backends.wallpaper, WallpaperBackend::Swww);
        assert_eq!(config.commands.timeout_seconds, 30);
        assert!(config.hooks.post_update.is_empty());
    }

    #[test]
    fn parse_partial_config_uses_defaults_for_missing_fields() {
        let toml_str = r#"
[slideshow]
interval_seconds = 120
"#;
        let config: Config = toml::from_str(toml_str).expect("partial TOML should parse");

        // Provided value
        assert_eq!(config.slideshow.interval_seconds, 120);

        // Defaults
        assert_eq!(config.slideshow.directory, "~/Pictures/Wallpapers");
        assert!(config.slideshow.randomize);
        assert_eq!(config.slideshow.history_limit, 32);
        assert!(!config.slideshow.recursive);
        assert_eq!(config.backends.colors, ColorBackend::Wallust);
        assert_eq!(config.backends.wallpaper, WallpaperBackend::Swww);
        assert_eq!(config.commands.timeout_seconds, 30);
    }

    // -- Backend deserialization -----------------------------------------------

    #[test]
    fn parse_wbg_backend() {
        let toml_str = r#"
[backends]
colors = "none"
wallpaper = "wbg"
"#;
        let config: Config = toml::from_str(toml_str).expect("valid TOML");
        assert_eq!(config.backends.colors, ColorBackend::None);
        assert_eq!(config.backends.wallpaper, WallpaperBackend::Wbg);
    }

    #[test]
    fn parse_invalid_backend_fails() {
        let toml_str = r#"
[backends]
colors = "doesnotexist"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "unknown backend should fail to parse");
    }

    // -- Config convenience methods -------------------------------------------

    #[test]
    fn wallpaper_dir_expands_tilde() {
        let config = Config::default();
        let dir = config.wallpaper_dir();
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(dir, home.join("Pictures/Wallpapers"));
    }

    // =========================================================================
    // Phase 5 — Extended rigorous tests
    // =========================================================================

    // -- Unknown / extra fields are tolerated ---------------------------------

    #[test]
    fn unknown_top_level_fields_are_ignored() {
        let toml_str = r#"
some_random_key = "hello"

[slideshow]
directory = "~/Walls"
interval_seconds = 60

[unknown_section]
foo = "bar"
"#;
        let config: Config = toml::from_str(toml_str).expect("should ignore unknown fields");
        assert_eq!(config.slideshow.directory, "~/Walls");
        assert_eq!(config.slideshow.interval_seconds, 60);
    }

    // -- load_config from a real temp file ------------------------------------

    #[test]
    fn load_config_from_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[slideshow]
directory = "/custom/walls"
interval_seconds = 42
randomize = false
history_limit = 7
recursive = true

[backends]
colors = "matugen"
wallpaper = "wbg"

[hooks]
post_update = ["echo loaded"]

[commands]
timeout_seconds = 9
"#,
        )
        .expect("write");

        let config = load_config(&path).expect("should load");
        assert_eq!(config.slideshow.directory, "/custom/walls");
        assert_eq!(config.slideshow.interval_seconds, 42);
        assert!(!config.slideshow.randomize);
        assert_eq!(config.slideshow.history_limit, 7);
        assert!(config.slideshow.recursive);
        assert_eq!(config.backends.colors, ColorBackend::Matugen);
        assert_eq!(config.backends.wallpaper, WallpaperBackend::Wbg);
        assert_eq!(config.hooks.post_update, vec!["echo loaded"]);
        assert_eq!(config.commands.timeout_seconds, 9);
    }

    #[test]
    fn load_config_missing_file_returns_defaults() {
        let path = PathBuf::from("/tmp/temad_nonexistent_config_test.toml");
        let config = load_config(&path).expect("should return defaults");
        assert_eq!(config.slideshow.interval_seconds, 300);
        assert!(config.slideshow.randomize);
        assert_eq!(config.slideshow.history_limit, 32);
        assert!(!config.slideshow.recursive);
        assert_eq!(config.backends.colors, ColorBackend::Wallust);
        assert_eq!(config.commands.timeout_seconds, 30);
    }

    #[test]
    fn load_config_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid [[[ toml !!!").expect("write");

        let result = load_config(&path);
        assert!(result.is_err(), "malformed TOML should return Err");
    }

    #[test]
    fn load_config_rejects_zero_slideshow_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("zero_interval.toml");
        std::fs::write(
            &path,
            r#"
[slideshow]
interval_seconds = 0
"#,
        )
        .expect("write");

        let result = load_config(&path);
        assert!(result.is_err(), "zero interval should return Err");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("slideshow.interval_seconds"));
        assert!(error.contains("greater than 0"));
    }

    #[test]
    fn load_config_accepts_positive_slideshow_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("positive_interval.toml");
        std::fs::write(
            &path,
            r#"
[slideshow]
interval_seconds = 1
"#,
        )
        .expect("write");

        let config = load_config(&path).expect("positive interval should load");
        assert_eq!(config.slideshow.interval_seconds, 1);
    }

    #[test]
    fn load_config_rejects_zero_history_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("zero_history_limit.toml");
        std::fs::write(
            &path,
            r#"
[slideshow]
history_limit = 0
"#,
        )
        .expect("write");

        let result = load_config(&path);
        assert!(result.is_err(), "zero history limit should return Err");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("slideshow.history_limit"));
    }

    #[test]
    fn load_config_rejects_zero_command_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("zero_timeout.toml");
        std::fs::write(
            &path,
            r#"
[commands]
timeout_seconds = 0
"#,
        )
        .expect("write");

        let result = load_config(&path);
        assert!(result.is_err(), "zero command timeout should return Err");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("commands.timeout_seconds"));
    }

    // -- resolve_config_path --------------------------------------------------

    #[test]
    fn resolve_config_path_explicit_takes_priority() {
        let explicit = PathBuf::from("/my/custom/config.toml");
        let resolved = resolve_config_path(Some(&explicit));
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_config_path_default_ends_with_temad() {
        let resolved = resolve_config_path(None);
        assert!(
            resolved.ends_with("temad/config.toml"),
            "default path should end with temad/config.toml, got: {}",
            resolved.display()
        );
    }

    // -- swww_options defaults ------------------------------------------------

    #[test]
    fn swww_options_defaults_are_sensible() {
        let opts = SwwwOptions::default();
        assert_eq!(opts.transition_type, "random");
        assert_eq!(opts.transition_step, 100);
        assert_eq!(opts.transition_fps, 60);
        assert_eq!(opts.transition_duration, None);
    }

    #[test]
    fn swww_options_partial_override() {
        let toml_str = r#"
[swww_options]
transition_type = "wipe"
"#;
        let config: Config = toml::from_str(toml_str).expect("valid TOML");
        assert_eq!(config.swww_options.transition_type, "wipe");
        // Defaults preserved for the rest
        assert_eq!(config.swww_options.transition_step, 100);
        assert_eq!(config.swww_options.transition_fps, 60);
        assert_eq!(config.swww_options.transition_duration, None);
    }

    #[test]
    fn swww_options_transition_duration_override() {
        let toml_str = r#"
[swww_options]
transition_duration = 2
"#;
        let config: Config = toml::from_str(toml_str).expect("valid TOML");
        assert_eq!(config.swww_options.transition_duration, Some(2));
    }

    // -- Hooks with special characters ----------------------------------------

    #[test]
    fn hooks_preserve_shell_variables_and_special_chars() {
        let toml_str = r#"
[hooks]
post_update = [
    "hyprctl keyword general:col.active_border 'rgba(${color2:1}ff) rgba(${color4:1}ff) 45deg'",
    "hyprctl keyword general:col.inactive_border 'rgba(${background:1}aa)'",
    "swaync-client -rs"
]
"#;
        let config: Config = toml::from_str(toml_str).expect("valid TOML");
        assert_eq!(config.hooks.post_update.len(), 3);
        assert!(config.hooks.post_update[0].contains("${color2:1}"));
        assert!(config.hooks.post_update[1].contains("${background:1}"));
        assert_eq!(config.hooks.post_update[2], "swaync-client -rs");
    }

    // -- Tilde expansion edge cases -------------------------------------------

    #[test]
    fn expand_tilde_deeply_nested_path() {
        let expanded = expand_tilde("~/a/b/c/d/e/f");
        let home = dirs::home_dir().expect("home");
        assert_eq!(expanded, home.join("a/b/c/d/e/f"));
    }

    #[test]
    fn expand_tilde_does_not_expand_mid_path() {
        // Only a leading ~ should be expanded
        let expanded = expand_tilde("/some/~/path");
        assert_eq!(expanded, PathBuf::from("/some/~/path"));
    }

    // -- Invalid wallpaper backend --------------------------------------------

    #[test]
    fn parse_invalid_wallpaper_backend_fails() {
        let toml_str = r#"
[backends]
wallpaper = "sway-bg"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "unknown wallpaper backend should fail");
    }

    // -- All backend combos parse correctly -----------------------------------

    #[test]
    fn all_valid_backend_combinations_parse() {
        let color_backends = ["wallust", "matugen", "none"];
        let wallpaper_backends = ["swww", "hyprpaper", "wbg"];

        for cb in &color_backends {
            for wb in &wallpaper_backends {
                let toml_str = format!(
                    r#"
[backends]
colors = "{cb}"
wallpaper = "{wb}"
"#
                );
                let config: Config = toml::from_str(&toml_str)
                    .unwrap_or_else(|e| panic!("combo {cb}/{wb} should parse: {e}"));
                // Just verify they parsed without panicking
                let _ = config.backends.colors;
                let _ = config.backends.wallpaper;
            }
        }
    }
}
