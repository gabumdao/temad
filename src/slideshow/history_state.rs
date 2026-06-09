use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static HISTORY_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn last_image_state_path() -> PathBuf {
    let state_root = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));

    state_root.join("temad").join("last_image")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistoryState {
    pub(crate) cursor: usize,
    pub(crate) entries: Vec<PathBuf>,
}

impl HistoryState {
    pub(crate) fn current(&self) -> Option<&PathBuf> {
        self.entries.get(self.cursor)
    }

    pub(crate) fn previous(&self) -> Option<&PathBuf> {
        self.cursor
            .checked_sub(1)
            .and_then(|idx| self.entries.get(idx))
    }

    pub(crate) fn next(&self) -> Option<&PathBuf> {
        self.entries.get(self.cursor + 1)
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

        self.entries.push(image);
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
            self.entries.push(image);
            self.cursor = 0;
            return;
        }

        self.entries.insert(self.cursor, image);
        if self.entries.len() > history_limit {
            self.entries.pop();
        }
    }
}

fn encode_history_entry(wall_dir: &Path, image: &Path) -> String {
    image
        .strip_prefix(wall_dir)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| image.to_path_buf())
        .display()
        .to_string()
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
        let entries: Vec<PathBuf> = lines[1..]
            .iter()
            .filter_map(|line| decode_history_entry(wall_dir, line))
            .collect();
        if entries.is_empty() {
            return HistoryState::default();
        }
        return HistoryState {
            cursor: cursor.min(entries.len() - 1),
            entries,
        };
    }

    let entry = decode_history_entry(wall_dir, lines[0]);
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
