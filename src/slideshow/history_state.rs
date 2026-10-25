use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static HISTORY_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn last_image_state_path() -> PathBuf {
    let state_root = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));

    state_root.join("temad").join("last_image")
}

/// A wallpaper and the time it was originally recorded.
/// Plain-path legacy entries may have an unknown timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryEntry {
    pub(crate) path: PathBuf,
    pub(crate) applied_at: Option<u64>,
}

impl HistoryEntry {
    fn applied_now(path: PathBuf) -> Self {
        let applied_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            path,
            applied_at: Some(applied_at),
        }
    }
}

// Fixtures for the historical plain-path format; production records the clock.
#[cfg(test)]
impl From<PathBuf> for HistoryEntry {
    fn from(path: PathBuf) -> Self {
        Self {
            path,
            applied_at: None,
        }
    }
}

#[cfg(test)]
impl PartialEq<PathBuf> for HistoryEntry {
    fn eq(&self, other: &PathBuf) -> bool {
        &self.path == other
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistoryState {
    pub(crate) cursor: usize,
    pub(crate) entries: Vec<HistoryEntry>,
}

impl HistoryState {
    pub(crate) fn current(&self) -> Option<&PathBuf> {
        self.entries.get(self.cursor).map(|entry| &entry.path)
    }

    pub(crate) fn previous(&self) -> Option<&PathBuf> {
        self.cursor
            .checked_sub(1)
            .and_then(|idx| self.entries.get(idx))
            .map(|entry| &entry.path)
    }

    pub(crate) fn next(&self) -> Option<&PathBuf> {
        self.entries.get(self.cursor + 1).map(|entry| &entry.path)
    }

    pub(crate) fn current_cloned(&self) -> Option<PathBuf> {
        self.current().cloned()
    }

    pub(crate) fn step_prev(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub(crate) fn step_next(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        }
    }

    pub(crate) fn record(&mut self, image: PathBuf, history_limit: usize) {
        if self.current() == Some(&image) {
            return;
        }

        if self.cursor + 1 < self.entries.len() {
            self.entries.truncate(self.cursor + 1);
        }

        self.entries.push(HistoryEntry::applied_now(image));
        if self.entries.len() > history_limit {
            let excess = self.entries.len() - history_limit;
            self.entries.drain(0..excess);
            self.cursor = self.cursor.saturating_sub(excess);
        }
        self.cursor = self.entries.len() - 1;
    }

    pub(crate) fn prepend_previous(&mut self, image: PathBuf, history_limit: usize) {
        if self.current() == Some(&image) {
            return;
        }

        if self.entries.is_empty() {
            self.entries.push(HistoryEntry::applied_now(image));
            self.cursor = 0;
            return;
        }

        self.entries
            .insert(self.cursor, HistoryEntry::applied_now(image));
        if self.entries.len() > history_limit {
            self.entries.pop();
        }
    }
}

fn encode_history_entry(wall_dir: &Path, entry: &HistoryEntry) -> String {
    let path = entry
        .path
        .strip_prefix(wall_dir)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| entry.path.clone());
    match entry.applied_at {
        Some(timestamp) => format!("@{timestamp}\t{}", path.display()),
        None => path.display().to_string(),
    }
}

/// Parse historical plain paths and `@<unix-seconds>\\t<path>` records,
/// preserving their timestamps. Unknown historical times remain `None`.
fn parse_history_entry(wall_dir: &Path, raw: &str) -> Option<HistoryEntry> {
    let value = raw.trim();
    let (applied_at, filename) = match value.split_once('\t') {
        Some((prefix, file)) => match prefix
            .strip_prefix('@')
            .and_then(|timestamp| timestamp.parse::<u64>().ok())
        {
            Some(timestamp) => (Some(timestamp), file),
            None => (None, value),
        },
        None => (None, value),
    };
    if filename.is_empty() {
        return None;
    }

    let path = PathBuf::from(filename);
    let path = if path.is_absolute() {
        path
    } else {
        wall_dir.join(path)
    };
    Some(HistoryEntry { path, applied_at })
}

/// The status/history CLI only needs paths; the state store retains metadata.
pub(crate) fn decode_history_entry(wall_dir: &Path, raw: &str) -> Option<PathBuf> {
    parse_history_entry(wall_dir, raw).map(|entry| entry.path)
}

fn encode_history_state(wall_dir: &Path, state: &HistoryState) -> String {
    let mut lines = vec![state.cursor.to_string()];
    lines.extend(
        state
            .entries
            .iter()
            .map(|entry| encode_history_entry(wall_dir, entry)),
    );
    lines.join("\n") + "\n"
}

pub(crate) fn decode_history_state(wall_dir: &Path, raw: &str) -> HistoryState {
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.is_empty() {
        return HistoryState::default();
    }

    if let Ok(cursor) = lines[0].parse::<usize>() {
        let entries: Vec<HistoryEntry> = lines[1..]
            .iter()
            .filter_map(|line| parse_history_entry(wall_dir, line))
            .collect();
        if entries.is_empty() {
            return HistoryState::default();
        }
        return HistoryState {
            cursor: cursor.min(entries.len() - 1),
            entries,
        };
    }

    let entry = parse_history_entry(wall_dir, lines[0]);
    match entry {
        Some(entry) => HistoryState {
            cursor: 0,
            entries: vec![entry],
        },
        None => HistoryState::default(),
    }
}

pub(crate) fn load_history_from(state_path: &Path, wall_dir: &Path) -> Result<HistoryState> {
    if !state_path.exists() {
        return Ok(HistoryState::default());
    }

    let raw = fs::read_to_string(state_path).with_context(|| {
        format!(
            "Failed to read wallpaper history state: {}",
            state_path.display()
        )
    })?;
    Ok(decode_history_state(wall_dir, &raw))
}

pub(crate) fn history_lock_path(state_path: &Path) -> PathBuf {
    let file_name = state_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("last_image");
    state_path.with_file_name(format!("{file_name}.lock"))
}

pub(crate) fn acquire_history_lock(state_path: &Path) -> Result<File> {
    let lock_path = history_lock_path(state_path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create history lock directory: {}",
                parent.display()
            )
        })?;
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Failed to open history lock file: {}", lock_path.display()))?;

    file.lock()
        .with_context(|| format!("Failed to lock history state: {}", lock_path.display()))?;
    Ok(file)
}

fn history_temp_path(state_path: &Path) -> PathBuf {
    let file_name = state_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("last_image");
    let counter = HISTORY_TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    state_path.with_file_name(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        counter
    ))
}

fn atomic_write_history(state_path: &Path, encoded: &str) -> Result<()> {
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create wallpaper state directory: {}",
                parent.display()
            )
        })?;
    }

    let temp_path = history_temp_path(state_path);
    let write_result = (|| -> Result<()> {
        let mut temp_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| {
                format!(
                    "Failed to create history temp file: {}",
                    temp_path.display()
                )
            })?;
        temp_file.write_all(encoded.as_bytes()).with_context(|| {
            format!("Failed to write history temp file: {}", temp_path.display())
        })?;
        temp_file.sync_all().with_context(|| {
            format!("Failed to sync history temp file: {}", temp_path.display())
        })?;
        fs::rename(&temp_path, state_path).with_context(|| {
            format!(
                "Failed to atomically replace wallpaper history state: {}",
                state_path.display()
            )
        })?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result
}

fn persist_history_unlocked_to(
    state_path: &Path,
    wall_dir: &Path,
    state: &HistoryState,
) -> Result<()> {
    let encoded = encode_history_state(wall_dir, state);

    if fs::read_to_string(state_path).ok().as_deref() == Some(encoded.as_str()) {
        return Ok(());
    }

    atomic_write_history(state_path, &encoded)
}

#[cfg(test)]
pub(crate) fn persist_history_to(
    state_path: &Path,
    wall_dir: &Path,
    state: &HistoryState,
) -> Result<()> {
    let _lock = acquire_history_lock(state_path)?;
    persist_history_unlocked_to(state_path, wall_dir, state)
}

pub(crate) fn with_locked_history_from<T, F>(
    state_path: &Path,
    wall_dir: &Path,
    operation: F,
) -> Result<T>
where
    F: FnOnce(&mut HistoryState) -> Result<T>,
{
    let _lock = acquire_history_lock(state_path)?;
    let mut history = load_history_from(state_path, wall_dir)?;
    let result = operation(&mut history)?;
    persist_history_unlocked_to(state_path, wall_dir, &history)?;
    Ok(result)
}

pub(crate) fn persist_last_image(
    wall_dir: &Path,
    image: &Path,
    history_limit: usize,
) -> Result<()> {
    let state_path = last_image_state_path();
    with_locked_history_from(&state_path, wall_dir, |state| {
        state.record(image.to_path_buf(), history_limit);
        Ok(())
    })
}

pub(crate) fn load_history(wall_dir: &Path) -> Result<HistoryState> {
    load_history_from(&last_image_state_path(), wall_dir)
}

#[cfg(test)]
mod timestamp_compat_tests {
    use super::*;

    #[test]
    fn parses_timestamped_history_without_treating_timestamp_as_filename() {
        let state = decode_history_state(
            Path::new("/wallpapers"),
            "1\n@1792937864\twallhaven-wqlxpx.jpg\nwallhaven-m9lp89.jpg\n",
        );
        assert_eq!(state.cursor, 1);
        assert_eq!(
            state.entries,
            vec![
                PathBuf::from("/wallpapers/wallhaven-wqlxpx.jpg"),
                PathBuf::from("/wallpapers/wallhaven-m9lp89.jpg"),
            ]
        );
        assert_eq!(
            state.previous(),
            Some(&PathBuf::from("/wallpapers/wallhaven-wqlxpx.jpg"))
        );
    }

    #[test]
    fn timestamped_absolute_path_is_preserved() {
        assert_eq!(
            decode_history_entry(Path::new("/wallpapers"), "@1792937864\t/elsewhere/a.png"),
            Some(PathBuf::from("/elsewhere/a.png"))
        );
    }

    #[test]
    fn non_timestamp_prefix_is_treated_as_literal_filename() {
        assert_eq!(
            decode_history_entry(Path::new("/wallpapers"), "@abc\timage.jpg"),
            Some(PathBuf::from("/wallpapers/@abc\timage.jpg"))
        );
    }
}

#[cfg(test)]
mod timestamp_persistence_tests {
    use super::*;

    #[test]
    fn timestamped_entries_survive_decode_encode_and_navigation() {
        let dir = Path::new("/wallpapers");
        let raw = "1\n@1792937864\tone.jpg\n@1792937877\ttwo.jpg\n";
        let mut state = decode_history_state(dir, raw);
        assert_eq!(encode_history_state(dir, &state), raw);
        assert_eq!(state.entries[0].applied_at, Some(1792937864));
        assert_eq!(state.entries[1].applied_at, Some(1792937877));

        state.step_prev();
        assert_eq!(encode_history_state(dir, &state), raw.replace("1\n", "0\n"));
        state.step_next();
        assert_eq!(encode_history_state(dir, &state), raw);
    }

    #[test]
    fn newly_recorded_wallpaper_has_real_unix_timestamp() {
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut state = HistoryState::default();
        state.record(PathBuf::from("/wallpapers/new.jpg"), 32);
        let end = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let timestamp = state.entries[0].applied_at.expect("timestamp");
        assert!(timestamp >= start && timestamp <= end);
        assert_eq!(
            encode_history_state(Path::new("/wallpapers"), &state),
            format!("0\n@{timestamp}\tnew.jpg\n")
        );
    }

    #[test]
    fn unknown_legacy_timestamp_is_not_fabricated() {
        let dir = Path::new("/walls");
        let mut state = decode_history_state(dir, "0\nlegacy.jpg\n");
        assert_eq!(state.entries[0].applied_at, None);
        state.record(dir.join("new.jpg"), 32);
        assert!(state.entries[1].applied_at.is_some());
        let saved = encode_history_state(dir, &state);
        assert!(saved.starts_with("1\nlegacy.jpg\n@"));
        let recovered = decode_history_state(dir, &saved);
        assert_eq!(recovered.entries[0].applied_at, None);
        assert_eq!(recovered.entries[1].applied_at, state.entries[1].applied_at);
    }

    #[test]
    fn history_truncation_preserves_timestamps_of_retained_entries() {
        let dir = Path::new("/walls");
        let mut state = decode_history_state(dir, "1\n@100\ta.jpg\n@200\tb.jpg\n@300\tc.jpg\n");
        state.record(dir.join("d.jpg"), 3);
        assert_eq!(state.entries[0].applied_at, Some(100));
        assert_eq!(state.entries[1].applied_at, Some(200));
        assert!(state.entries[2].applied_at.is_some());
        assert_eq!(state.entries[2].path, dir.join("d.jpg"));
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn persisted_history_keeps_original_timestamps_after_reload() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_path = temp.path().join("last_image");
        let dir = Path::new("/walls");
        fs::write(&state_path, "1\n@100\ta.jpg\n@200\tb.jpg\n").expect("write");
        with_locked_history_from(&state_path, dir, |state| {
            state.step_prev();
            Ok(())
        })
        .expect("navigate");
        assert_eq!(
            fs::read_to_string(&state_path).expect("read"),
            "0\n@100\ta.jpg\n@200\tb.jpg\n"
        );
    }
}
