use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const VALID_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

pub(crate) fn is_supported_image_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| VALID_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
}

pub(crate) fn discover_images(directory: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        bail!(
            "Wallpaper directory does not exist or is not a directory: {}",
            directory.display()
        );
    }

    let mut images = Vec::new();
    collect_images(directory, recursive, &mut images)?;
    images.sort();

    tracing::debug!(
        dir = %directory.display(),
        count = images.len(),
        "Discovered wallpaper images"
    );

    Ok(images)
}

fn collect_images(directory: &Path, recursive: bool, images: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory: {}", directory.display()))?
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_images(&path, recursive, images)?;
            }
            continue;
        }

        if is_supported_image_path(&path) {
            images.push(path);
        }
    }

    Ok(())
}
