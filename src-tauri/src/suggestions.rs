//! Feature suggestions. VetCord has no backend server, so suggestions
//! are appended to a local JSON-lines file the maintainer can review,
//! and the UI also links to GitHub Issues.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

pub const GITHUB_ISSUES_URL: &str = "https://github.com/VetTV-Studios/VetCord/issues";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Suggestion {
    /// Unix epoch seconds.
    pub ts: u64,
    pub title: String,
    pub details: String,
}

pub fn suggestions_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("suggestions.jsonl")
}

pub fn append(path: &PathBuf, title: &str, details: &str) -> Result<Suggestion, String> {
    let title = title.trim();
    let details = details.trim();
    if title.is_empty() {
        return Err("suggestion title must not be empty".into());
    }
    if title.len() > 200 || details.len() > 5000 {
        return Err("suggestion is too long".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let entry = Suggestion {
        ts: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        title: title.to_string(),
        details: details.to_string(),
    };
    let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())?;
    Ok(entry)
}

pub fn list(path: &PathBuf) -> Vec<Suggestion> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_list_round_trip() {
        let dir = std::env::temp_dir().join("vetcord-test-suggestions");
        let path = dir.join("suggestions.jsonl");
        let _ = std::fs::remove_file(&path);
        append(&path, "Dark mode", "Please add it").unwrap();
        append(&path, "Themes", "").unwrap();
        let items = list(&path);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Dark mode");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_title_is_rejected() {
        let path = std::env::temp_dir().join("vetcord-test-sug2.jsonl");
        assert!(append(&path, "  ", "details").is_err());
    }
}
