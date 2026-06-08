use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::slideshow::last_image_state_path;

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistorySnapshot {
    current: Option<usize>,
    entries: Vec<PathBuf>,
}

pub fn render_history(config: &Config) -> String {
    let wall_dir = config.wallpaper_dir();
    let history_path = last_image_state_path();
    let snapshot = read_history_snapshot(&history_path, &wall_dir);
    build_history_report(&history_path, &snapshot)
}

fn build_history_report(history_path: &Path, snapshot: &HistorySnapshot) -> String {
    let mut output = String::new();
    output.push_str("temad history\n");
    output.push_str(&format!("State file: {}\n", history_path.display()));
    output.push_str(&format!("Entries: {}\n", snapshot.entries.len()));

    match snapshot.current {
        Some(index) => output.push_str(&format!("Current index: {}\n", index + 1)),
        None => output.push_str("Current index: none\n"),
    }

    if snapshot.entries.is_empty() {
        output.push_str("History: none\n");
        return output;
    }

    output.push_str("History:\n");
    for (index, entry) in snapshot.entries.iter().enumerate() {
        let marker = if snapshot.current == Some(index) {
            "*"
        } else {
            "-"
        };
        output.push_str(&format!("{} {}\n", marker, entry.display()));
    }

    output
}

fn read_history_snapshot(history_path: &Path, wall_dir: &Path) -> HistorySnapshot {
    let raw = match fs::read_to_string(history_path) {
        Ok(raw) => raw,
        Err(_) => {
            return HistorySnapshot {
                current: None,
                entries: Vec::new(),
            }
        }
    };

    parse_history_snapshot(wall_dir, &raw)
}

fn parse_history_snapshot(wall_dir: &Path, raw: &str) -> HistorySnapshot {
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.is_empty() {
        return HistorySnapshot {
            current: None,
            entries: Vec::new(),
        };
    }

    if let Ok(cursor) = lines[0].parse::<usize>() {
        let entries: Vec<PathBuf> = lines[1..]
            .iter()
            .filter_map(|line| decode_history_entry(wall_dir, line))
            .collect();

        if entries.is_empty() {
            return HistorySnapshot {
                current: None,
                entries: Vec::new(),
            };
        }

        return HistorySnapshot {
            current: Some(cursor.min(entries.len() - 1)),
            entries,
        };
    }

    match decode_history_entry(wall_dir, lines[0]) {
        Some(entry) => HistorySnapshot {
            current: Some(0),
            entries: vec![entry],
        },
        None => HistorySnapshot {
            current: None,
            entries: Vec::new(),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_history_snapshot_supports_bounded_format() {
        let wall_dir = Path::new("/walls");
        let snapshot = parse_history_snapshot(wall_dir, "1\na.jpg\nb.jpg\n");

        assert_eq!(snapshot.current, Some(1));
        assert_eq!(
            snapshot.entries,
            vec![wall_dir.join("a.jpg"), wall_dir.join("b.jpg")]
        );
    }

    #[test]
    fn parse_history_snapshot_supports_legacy_format() {
        let wall_dir = Path::new("/walls");
        let snapshot = parse_history_snapshot(wall_dir, "legacy.png\n");

        assert_eq!(snapshot.current, Some(0));
        assert_eq!(snapshot.entries, vec![wall_dir.join("legacy.png")]);
    }

    #[test]
    fn build_history_report_handles_empty_history() {
        let report = build_history_report(
            Path::new("/tmp/last_image"),
            &HistorySnapshot {
                current: None,
                entries: Vec::new(),
            },
        );

        assert!(report.contains("Entries: 0"));
        assert!(report.contains("History: none"));
    }

    #[test]
    fn render_history_marks_current_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let report = build_history_report(
            Path::new("/tmp/last_image"),
            &HistorySnapshot {
                current: Some(1),
                entries: vec![temp.path().join("a.jpg"), temp.path().join("b.jpg")],
            },
        );

        assert!(report.contains("Current index: 2"));
        assert!(report.contains(&format!("* {}", temp.path().join("b.jpg").display())));
        assert!(report.contains(&format!("- {}", temp.path().join("a.jpg").display())));
    }

    #[test]
    fn read_history_snapshot_returns_empty_for_missing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let snapshot = read_history_snapshot(&temp.path().join("missing"), temp.path());

        assert_eq!(snapshot.current, None);
        assert!(snapshot.entries.is_empty());
    }
}
