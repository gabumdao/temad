use anyhow::{bail, Context, Result};
use rand::seq::SliceRandom;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::engine::{CommandRunner, Engine};

// ===========================================================================
// Image discovery
// ===========================================================================

/// Valid wallpaper file extensions (lowercase comparison).
const VALID_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

pub(crate) fn is_supported_image_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| VALID_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
            .unwrap_or(false)
}

fn last_image_state_path() -> PathBuf {
    let state_root = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));

    state_root.join("temad").join("last_image")
}

fn encode_last_image(wall_dir: &Path, image: &Path) -> String {
    let value = image
        .strip_prefix(wall_dir)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| image.to_path_buf());

    format!("{}\n", value.display())
}

fn decode_last_image(wall_dir: &Path, raw: &str) -> Option<PathBuf> {
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

fn load_last_image_from(state_path: &Path, wall_dir: &Path) -> Result<Option<PathBuf>> {
    if !state_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(state_path).with_context(|| {
        format!(
            "Failed to read last wallpaper state: {}",
            state_path.display()
        )
    })?;
    Ok(decode_last_image(wall_dir, &raw))
}

fn persist_last_image_to(state_path: &Path, wall_dir: &Path, image: &Path) -> Result<()> {
    let encoded = encode_last_image(wall_dir, image);

    if fs::read_to_string(state_path).ok().as_deref() == Some(encoded.as_str()) {
        return Ok(());
    }

    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create wallpaper state directory: {}",
                parent.display()
            )
        })?;
    }

    fs::write(state_path, encoded).with_context(|| {
        format!(
            "Failed to write last wallpaper state: {}",
            state_path.display()
        )
    })
}

pub fn load_last_image(wall_dir: &Path) -> Result<Option<PathBuf>> {
    load_last_image_from(&last_image_state_path(), wall_dir)
}

pub fn persist_last_image(wall_dir: &Path, image: &Path) -> Result<()> {
    persist_last_image_to(&last_image_state_path(), wall_dir, image)
}

/// Scan `directory` and return all files with a valid image extension.
pub fn discover_images(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        bail!(
            "Wallpaper directory does not exist or is not a directory: {}",
            directory.display()
        );
    }

    let mut images: Vec<PathBuf> = std::fs::read_dir(directory)
        .with_context(|| format!("Failed to read directory: {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_supported_image_path(path))
        .collect();

    // Sort for deterministic ordering before any shuffle
    images.sort();

    tracing::debug!(
        dir = %directory.display(),
        count = images.len(),
        "Discovered wallpaper images"
    );

    Ok(images)
}

// ===========================================================================
// Image selector (no-repeat randomizer)
// ===========================================================================

/// Picks images from a pool, ensuring no consecutive duplicates.
#[derive(Debug)]
pub struct ImageSelector {
    images: Vec<PathBuf>,
    randomize: bool,
    last: Option<PathBuf>,
}

impl ImageSelector {
    /// Create a new selector.  The image list must not be empty.
    pub fn new(images: Vec<PathBuf>, randomize: bool) -> Result<Self> {
        Self::with_last(images, randomize, None)
    }

    pub fn with_last(images: Vec<PathBuf>, randomize: bool, last: Option<PathBuf>) -> Result<Self> {
        if images.is_empty() {
            bail!("No wallpaper images found in the configured directory");
        }

        let last = last.filter(|candidate| images.iter().any(|path| path == candidate));

        Ok(Self {
            images,
            randomize,
            last,
        })
    }

    /// Pick the next image.
    ///
    /// - **Sequential mode**: cycles through the sorted list.
    /// - **Random mode**: picks a random image that differs from the previous
    ///   pick (when the pool has ≥ 2 images).
    pub fn next(&mut self) -> &Path {
        let chosen = if self.randomize {
            self.next_random()
        } else {
            self.next_sequential()
        };
        self.last = Some(chosen.clone());
        // Return a reference into `self.last` which we just set
        self.last.as_deref().unwrap()
    }

    pub fn update_images(&mut self, images: Vec<PathBuf>) -> Result<()> {
        if images.is_empty() {
            bail!("No wallpaper images found in the configured directory");
        }

        let last_still_exists = self
            .last
            .as_ref()
            .is_some_and(|last| images.iter().any(|path| path == last));

        self.images = images;

        if !last_still_exists {
            self.last = None;
        }

        Ok(())
    }

    fn next_sequential(&self) -> PathBuf {
        match &self.last {
            None => self.images[0].clone(),
            Some(prev) => {
                let idx = self
                    .images
                    .iter()
                    .position(|p| p == prev)
                    .map(|i| (i + 1) % self.images.len())
                    .unwrap_or(0);
                self.images[idx].clone()
            }
        }
    }

    fn next_random(&self) -> PathBuf {
        let mut rng = rand::thread_rng();

        if self.images.len() == 1 {
            return self.images[0].clone();
        }

        loop {
            let pick = self.images.choose(&mut rng).unwrap().clone();
            if self.last.as_ref() != Some(&pick) {
                return pick;
            }
        }
    }
}

// ===========================================================================
// Daemon loop
// ===========================================================================

pub fn run_daemon_cycle<R: CommandRunner>(
    wall_dir: &Path,
    selector: &mut ImageSelector,
    engine: &Engine<R>,
) -> Option<PathBuf> {
    match discover_images(wall_dir) {
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

/// Run the slideshow daemon loop until the process is killed.
pub fn run_next<R: CommandRunner>(config: &Config, runner: &R) -> Result<PathBuf> {
    let wall_dir = config.wallpaper_dir();
    let images = discover_images(&wall_dir)?;
    let last = load_last_image(&wall_dir).map_err(|error| {
        tracing::warn!(error = %error, dir = %wall_dir.display(), "Failed to load last wallpaper state");
        error
    }).ok().flatten();
    let mut selector = match last {
        Some(last) => ImageSelector::with_last(images, config.slideshow.randomize, Some(last))?,
        None => ImageSelector::new(images, config.slideshow.randomize)?,
    };
    let image = selector.next().to_path_buf();
    let engine = Engine::new(config, runner);

    tracing::info!(image = %image.display(), "Applying next wallpaper");
    engine.apply(&image)?;
    if let Err(error) = persist_last_image(&wall_dir, &image) {
        tracing::warn!(error = %error, path = %image.display(), "Failed to persist last wallpaper state");
    }

    Ok(image)
}

pub async fn run_daemon<R: CommandRunner>(config: &Config, runner: &R) -> Result<()> {
    let wall_dir = config.wallpaper_dir();
    let images = discover_images(&wall_dir)?;

    tracing::info!(
        count = images.len(),
        dir = %wall_dir.display(),
        "Image pool ready"
    );

    let last = match load_last_image(&wall_dir) {
        Ok(last) => last,
        Err(error) => {
            tracing::warn!(error = %error, dir = %wall_dir.display(), "Failed to load last wallpaper state");
            None
        }
    };

    let mut selector = match last {
        Some(last) => ImageSelector::with_last(images, config.slideshow.randomize, Some(last))?,
        None => ImageSelector::new(images, config.slideshow.randomize)?,
    };
    let engine = Engine::new(config, runner);
    let interval = std::time::Duration::from_secs(config.slideshow.interval_seconds);

    loop {
        if let Some(image) = run_daemon_cycle(&wall_dir, &mut selector, &engine) {
            if let Err(error) = persist_last_image(&wall_dir, &image) {
                tracing::warn!(error = %error, path = %image.display(), "Failed to persist last wallpaper state");
            }
        }

        tracing::debug!(seconds = config.slideshow.interval_seconds, "Sleeping");
        tokio::time::sleep(interval).await;
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        BackendsConfig, ColorBackend, SlideshowConfig, SwwwOptions, WallpaperBackend,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct Invocation {
        program: String,
        args: Vec<String>,
        is_shell: bool,
    }

    struct RecordingRunner {
        log: Mutex<Vec<Invocation>>,
        daemon_running: bool,
    }

    impl RecordingRunner {
        fn new(daemon_running: bool) -> Self {
            Self {
                log: Mutex::new(Vec::new()),
                daemon_running,
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

    struct FailingApplyRunner;

    fn mock_success() -> Output {
        Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn mock_failure(stderr_msg: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(256),
            stdout: Vec::new(),
            stderr: stderr_msg.as_bytes().to_vec(),
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
            self.log.lock().unwrap().push(Invocation {
                program: program.to_string(),
                args: args.iter().map(|arg| arg.to_string()).collect(),
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

    impl CommandRunner for FailingApplyRunner {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<Output> {
            Ok(mock_failure("intentional wallpaper failure"))
        }

        fn run_shell(&self, _cmd: &str) -> Result<Output> {
            Ok(mock_success())
        }

        fn is_running(&self, _process_name: &str) -> bool {
            true
        }
    }

    /// Helper: create a temp directory with some dummy image files.
    fn make_test_dir(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        for name in files {
            let path = dir.path().join(name);
            fs::write(&path, b"fake image data").expect("write test file");
        }
        dir
    }

    #[test]
    fn daemon_cycle_applies_next_image_after_refresh() {
        let dir = make_test_dir(&["a.jpg"]);
        let images = discover_images(dir.path()).expect("discover initial images");
        let mut selector = ImageSelector::new(images, false).expect("build selector");
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None,
                wallpaper: WallpaperBackend::Wbg,
            },
            ..Default::default()
        };
        let runner = RecordingRunner::new(true);
        let engine = Engine::new(&config, &runner);

        let applied = run_daemon_cycle(dir.path(), &mut selector, &engine);

        let invocations = runner.invocations();
        assert_eq!(applied, Some(dir.path().join("a.jpg")));
        let expected = dir.path().join("a.jpg").display().to_string();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "wbg");
        assert_eq!(invocations[0].args, vec![expected]);
    }

    #[test]
    fn daemon_cycle_skips_when_refresh_finds_no_images() {
        let dir = make_test_dir(&["a.jpg"]);
        let images = discover_images(dir.path()).expect("discover initial images");
        let mut selector = ImageSelector::new(images, false).expect("build selector");
        fs::remove_file(dir.path().join("a.jpg")).expect("remove image");
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None,
                wallpaper: WallpaperBackend::Wbg,
            },
            ..Default::default()
        };
        let runner = RecordingRunner::new(true);
        let engine = Engine::new(&config, &runner);

        let applied = run_daemon_cycle(dir.path(), &mut selector, &engine);

        assert!(applied.is_none());
        assert!(runner.invocations().is_empty());
    }

    #[test]
    fn daemon_cycle_continues_when_engine_apply_fails() {
        let dir = make_test_dir(&["a.jpg"]);
        let images = discover_images(dir.path()).expect("discover initial images");
        let mut selector = ImageSelector::new(images, false).expect("build selector");
        let config = Config {
            backends: BackendsConfig {
                colors: ColorBackend::None,
                wallpaper: WallpaperBackend::Wbg,
            },
            ..Default::default()
        };
        let engine = Engine::new(&config, &FailingApplyRunner);

        let applied = run_daemon_cycle(dir.path(), &mut selector, &engine);

        assert!(applied.is_none());
        let expected = dir.path().join("a.jpg");
        assert_eq!(selector.next(), expected.as_path());
    }

    #[test]
    fn daemon_cycle_uses_swww_transition_duration_when_configured() {
        let dir = make_test_dir(&["a.jpg"]);
        let images = discover_images(dir.path()).expect("discover initial images");
        let mut selector = ImageSelector::new(images, false).expect("build selector");
        let config = Config {
            slideshow: SlideshowConfig {
                directory: dir.path().display().to_string(),
                ..Default::default()
            },
            backends: BackendsConfig {
                colors: ColorBackend::None,
                wallpaper: WallpaperBackend::Swww,
            },
            swww_options: SwwwOptions {
                transition_type: "fade".to_string(),
                transition_step: 50,
                transition_fps: 30,
                transition_duration: Some(2),
            },
            ..Default::default()
        };
        let runner = RecordingRunner::new(true);
        let engine = Engine::new(&config, &runner);

        let applied = run_daemon_cycle(dir.path(), &mut selector, &engine);

        let invocations = runner.invocations();
        assert_eq!(applied, Some(dir.path().join("a.jpg")));
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program, "swww");
        assert_eq!(
            invocations[0].args,
            vec![
                "img".to_string(),
                dir.path().join("a.jpg").display().to_string(),
                "--transition-type".to_string(),
                "fade".to_string(),
                "--transition-step".to_string(),
                "50".to_string(),
                "--transition-fps".to_string(),
                "30".to_string(),
                "--transition-duration".to_string(),
                "2".to_string(),
            ]
        );
    }

    // -- discover_images ------------------------------------------------------

    #[test]
    fn discover_filters_valid_extensions() {
        let dir = make_test_dir(&[
            "a.jpg",
            "b.png",
            "c.jpeg",
            "d.webp",
            "e.bmp",
            "f.gif",
            "g.txt",
            "h.mp4",
            "i.rs",
            ".hidden.jpg",
        ]);

        let images = discover_images(dir.path()).expect("should succeed");
        let names: HashSet<String> = images
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        // Valid images
        assert!(names.contains("a.jpg"));
        assert!(names.contains("b.png"));
        assert!(names.contains("c.jpeg"));
        assert!(names.contains("d.webp"));
        assert!(names.contains("e.bmp"));
        assert!(names.contains("f.gif"));
        assert!(names.contains(".hidden.jpg")); // hidden but valid extension

        // Invalid
        assert!(!names.contains("g.txt"));
        assert!(!names.contains("h.mp4"));
        assert!(!names.contains("i.rs"));
    }

    #[test]
    fn discover_returns_error_for_missing_dir() {
        let result = discover_images(Path::new("/nonexistent/dir"));
        assert!(result.is_err());
    }

    #[test]
    fn discover_returns_empty_for_dir_with_no_images() {
        let dir = make_test_dir(&["readme.txt", "notes.md"]);
        let images = discover_images(dir.path()).expect("should succeed");
        assert!(images.is_empty());
    }

    // -- ImageSelector --------------------------------------------------------

    #[test]
    fn selector_sequential_cycles() {
        let files: Vec<PathBuf> = (0..3)
            .map(|i| PathBuf::from(format!("/img/{i}.jpg")))
            .collect();
        let mut sel = ImageSelector::new(files.clone(), false).unwrap();

        assert_eq!(sel.next(), files[0].as_path());
        assert_eq!(sel.next(), files[1].as_path());
        assert_eq!(sel.next(), files[2].as_path());
        // Wraps around
        assert_eq!(sel.next(), files[0].as_path());
    }

    #[test]
    fn selector_random_no_consecutive_duplicates() {
        let files: Vec<PathBuf> = (0..10)
            .map(|i| PathBuf::from(format!("/img/{i}.png")))
            .collect();
        let mut sel = ImageSelector::new(files, true).unwrap();

        let mut prev: Option<PathBuf> = None;
        for _ in 0..100 {
            let picked = sel.next().to_path_buf();
            if let Some(ref p) = prev {
                assert_ne!(
                    &picked, p,
                    "Random selector must not pick the same image consecutively"
                );
            }
            prev = Some(picked);
        }
    }

    #[test]
    fn selector_with_last_advances_sequentially() {
        let files = vec![
            PathBuf::from("/img/a.jpg"),
            PathBuf::from("/img/b.jpg"),
            PathBuf::from("/img/c.jpg"),
        ];
        let mut sel =
            ImageSelector::with_last(files.clone(), false, Some(files[1].clone())).unwrap();

        assert_eq!(sel.next(), files[2].as_path());
    }

    #[test]
    fn selector_with_missing_last_starts_from_first_image() {
        let files = vec![PathBuf::from("/img/a.jpg"), PathBuf::from("/img/b.jpg")];
        let mut sel = ImageSelector::with_last(
            files.clone(),
            false,
            Some(PathBuf::from("/img/missing.jpg")),
        )
        .unwrap();

        assert_eq!(sel.next(), files[0].as_path());
    }

    #[test]
    fn selector_random_with_last_avoids_repeating_persisted_image() {
        let files = vec![PathBuf::from("/img/a.jpg"), PathBuf::from("/img/b.jpg")];
        let mut sel =
            ImageSelector::with_last(files.clone(), true, Some(files[0].clone())).unwrap();

        assert_eq!(sel.next(), files[1].as_path());
    }

    #[test]
    fn persist_and_load_last_image_round_trip_relative_path() {
        let dir = make_test_dir(&["a.jpg", "b.jpg"]);
        let state_dir = tempfile::tempdir().expect("tempdir");
        let state_path = state_dir.path().join("last_image");
        let image = dir.path().join("b.jpg");

        persist_last_image_to(&state_path, dir.path(), &image).expect("persist state");
        let raw = fs::read_to_string(&state_path).expect("read state file");
        assert_eq!(
            raw,
            "b.jpg
"
        );

        let loaded = load_last_image_from(&state_path, dir.path()).expect("load state");
        assert_eq!(loaded, Some(image));
    }

    #[test]
    fn selector_single_image_always_returns_it() {
        let files = vec![PathBuf::from("/img/only.jpg")];
        let mut sel = ImageSelector::new(files, true).unwrap();
        for _ in 0..5 {
            assert_eq!(sel.next(), Path::new("/img/only.jpg"));
        }
    }

    #[test]
    fn selector_empty_pool_errors() {
        let result = ImageSelector::new(Vec::new(), true);
        assert!(result.is_err());
    }

    #[test]
    fn selector_update_picks_up_added_image_in_sequential_mode() {
        let mut sel = ImageSelector::new(vec![PathBuf::from("/img/a.jpg")], false).unwrap();

        assert_eq!(sel.next(), Path::new("/img/a.jpg"));

        sel.update_images(vec![
            PathBuf::from("/img/a.jpg"),
            PathBuf::from("/img/b.jpg"),
        ])
        .unwrap();

        assert_eq!(sel.next(), Path::new("/img/b.jpg"));
    }

    #[test]
    fn selector_update_drops_removed_image_from_rotation() {
        let mut sel = ImageSelector::new(
            vec![PathBuf::from("/img/a.jpg"), PathBuf::from("/img/b.jpg")],
            false,
        )
        .unwrap();

        assert_eq!(sel.next(), Path::new("/img/a.jpg"));

        sel.update_images(vec![PathBuf::from("/img/b.jpg")])
            .unwrap();

        assert_eq!(sel.next(), Path::new("/img/b.jpg"));
    }

    #[test]
    fn selector_update_rejects_empty_pool_without_losing_previous_images() {
        let mut sel = ImageSelector::new(vec![PathBuf::from("/img/a.jpg")], false).unwrap();

        let result = sel.update_images(Vec::new());
        assert!(result.is_err());

        assert_eq!(sel.next(), Path::new("/img/a.jpg"));
    }

    // =========================================================================
    // Phase 5 — Extended rigorous tests
    // =========================================================================

    // -- Two-image pool (randomizer edge case) --------------------------------

    #[test]
    fn selector_two_images_random_always_alternates() {
        let files = vec![PathBuf::from("/img/a.jpg"), PathBuf::from("/img/b.jpg")];
        let mut sel = ImageSelector::new(files, true).unwrap();

        let mut prev: Option<PathBuf> = None;
        for _ in 0..50 {
            let picked = sel.next().to_path_buf();
            if let Some(ref p) = prev {
                assert_ne!(&picked, p, "Two-image pool must always alternate");
            }
            prev = Some(picked);
        }
    }

    // -- Random distribution covers all images --------------------------------

    #[test]
    fn selector_random_covers_all_images_eventually() {
        let files: Vec<PathBuf> = (0..5)
            .map(|i| PathBuf::from(format!("/img/{i}.jpg")))
            .collect();
        let mut sel = ImageSelector::new(files.clone(), true).unwrap();
        let mut seen = HashSet::new();

        // After enough iterations, all 5 images should have been picked
        for _ in 0..200 {
            let picked = sel.next().to_path_buf();
            seen.insert(picked);
        }

        for f in &files {
            assert!(
                seen.contains(f),
                "Image {:?} was never picked in 200 iterations",
                f
            );
        }
    }

    // -- Sequential mode is deterministic -------------------------------------

    #[test]
    fn selector_sequential_is_deterministic() {
        let files: Vec<PathBuf> = (0..4)
            .map(|i| PathBuf::from(format!("/img/{i}.png")))
            .collect();

        // Two independent selectors with the same input
        let mut sel1 = ImageSelector::new(files.clone(), false).unwrap();
        let mut sel2 = ImageSelector::new(files, false).unwrap();

        for _ in 0..10 {
            assert_eq!(
                sel1.next(),
                sel2.next(),
                "Two sequential selectors with same input must produce same sequence"
            );
        }
    }

    // -- Mixed-case extensions ------------------------------------------------

    #[test]
    fn discover_handles_mixed_case_extensions() {
        let dir = make_test_dir(&["upper.JPG", "mixed.Png", "weird.JpEg", "normal.webp"]);

        let images = discover_images(dir.path()).expect("should succeed");
        let names: HashSet<String> = images
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.contains("upper.JPG"), "JPG should match");
        assert!(names.contains("mixed.Png"), "Png should match");
        assert!(names.contains("weird.JpEg"), "JpEg should match");
        assert!(names.contains("normal.webp"), "webp should match");
        assert_eq!(images.len(), 4);
    }

    // -- Subdirectories are not traversed -------------------------------------

    #[test]
    fn discover_ignores_subdirectories() {
        let dir = make_test_dir(&["top.jpg"]);
        // Create a subdirectory with an image inside
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).expect("mkdir");
        fs::write(sub.join("nested.png"), b"data").expect("write");

        let images = discover_images(dir.path()).expect("should succeed");
        let names: Vec<String> = images
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(images.len(), 1);
        assert_eq!(names[0], "top.jpg");
        // nested.png should not appear
        assert!(
            !names.contains(&"nested.png".to_string()),
            "subdirectory images must be excluded"
        );
    }

    // -- Files without extensions are skipped ---------------------------------

    #[test]
    fn discover_skips_files_without_extension() {
        let dir = make_test_dir(&["noext", "valid.jpg", "Makefile"]);
        let images = discover_images(dir.path()).expect("should succeed");

        assert_eq!(images.len(), 1);
        assert!(images[0].file_name().unwrap().to_str().unwrap() == "valid.jpg");
    }

    // -- Results are sorted ---------------------------------------------------

    #[test]
    fn discover_returns_sorted_results() {
        let dir = make_test_dir(&["z.jpg", "a.png", "m.webp", "b.jpeg"]);
        let images = discover_images(dir.path()).expect("should succeed");
        let names: Vec<String> = images
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        let mut sorted_names = names.clone();
        sorted_names.sort();

        assert_eq!(
            names, sorted_names,
            "discover_images must return sorted results"
        );
    }

    // -- Sequential selector wraps around multiple full cycles -----------------

    #[test]
    fn selector_sequential_wraps_multiple_full_cycles() {
        let files: Vec<PathBuf> = (0..3)
            .map(|i| PathBuf::from(format!("/img/{i}.jpg")))
            .collect();
        let mut sel = ImageSelector::new(files.clone(), false).unwrap();

        // 3 full cycles = 9 picks
        let expected: Vec<&Path> = files.iter().cycle().take(9).map(|p| p.as_path()).collect();

        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(sel.next(), *exp, "Mismatch at pick {i}");
        }
    }

    // -- Error message content for empty pool ---------------------------------

    #[test]
    fn selector_empty_pool_error_message_is_descriptive() {
        let result = ImageSelector::new(Vec::new(), true);
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.to_lowercase().contains("no wallpaper"),
            "error message should mention wallpaper: {err_msg}"
        );
    }

    // -- Error message for missing directory -----------------------------------

    #[test]
    fn discover_missing_dir_error_message_is_descriptive() {
        let result = discover_images(Path::new("/nonexistent/wallpapers"));
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("/nonexistent/wallpapers"),
            "error should contain the directory path: {err_msg}"
        );
    }
}
