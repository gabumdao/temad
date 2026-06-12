use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn init_config(config_path: &Path) -> Result<String> {
    if config_path.exists() {
        return Ok(format!(
            "temad init\nConfig already exists: {}\nNo changes made.\n",
            config_path.display()
        ));
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    fs::write(config_path, default_config_template())
        .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;

    Ok(format!(
        "temad init\nCreated config: {}\n",
        config_path.display()
    ))
}

fn default_config_template() -> &'static str {
    concat!(
        "[slideshow]\n",
        "directory = \"~/Pictures/Wallpapers\"\n",
        "interval_seconds = 300\n",
        "randomize = true\n",
        "history_limit = 32\n\n",
        "recursive = false\n\n",
        "[backends]\n",
        "colors = \"wallust\"\n",
        "wallpaper = \"swww\"\n\n",
        "[wallpaper_targets]\n",
        "outputs = []\n\n",
        "[swww_options]\n",
        "transition_type = \"random\"\n",
        "transition_step = 100\n",
        "transition_fps = 60\n\n",
        "[commands]\n",
        "timeout_seconds = 30\n\n",
        "[hooks]\n",
        "post_update = []\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;

    #[test]
    fn init_config_creates_missing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("temad/config.toml");

        let message = init_config(&path).expect("init config");

        assert!(message.contains("Created config:"));
        assert!(path.exists());

        let loaded = load_config(&path).expect("generated config should load");
        assert_eq!(loaded.slideshow.interval_seconds, 300);
        assert!(loaded.slideshow.randomize);
        assert_eq!(loaded.slideshow.history_limit, 32);
        assert!(!loaded.slideshow.recursive);
        assert_eq!(loaded.commands.timeout_seconds, 30);
    }

    #[test]
    fn init_config_does_not_overwrite_existing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.toml");
        fs::write(&path, "[slideshow]\ninterval_seconds = 42\n").expect("write existing config");

        let message = init_config(&path).expect("init config");

        assert!(message.contains("No changes made."));
        let contents = fs::read_to_string(&path).expect("read config");
        assert!(contents.contains("interval_seconds = 42"));
    }
}
