use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User settings persisted as JSON in Tauri's app-config directory.
/// No Discord credentials or tokens are ever stored here: login happens
/// entirely inside Discord's own web login page, like Vesktop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Hide Nitro upsells, gift/shop buttons and other promotional UI.
    pub debloat: bool,
    /// Start VetCord minimised to the tray area (close = hide, not quit, is separate).
    pub start_minimised: bool,
    /// Automatic gameplay clipping (rolling replay buffer).
    pub clipping_enabled: bool,
    /// Seconds of gameplay kept in the replay buffer.
    pub clip_seconds: u32,
    /// Frames per second for clip capture.
    pub clip_fps: u32,
    /// Capture backend: "auto" (GPU first, CPU fallback), "gpu", or "cpu".
    pub clip_backend: String,
    /// Whether the global save-clip hotkey is registered.
    pub hotkey_enabled: bool,
    /// Global shortcut that saves a clip, e.g. "Ctrl+Alt+C".
    pub hotkey: String,
    /// Capture target: "desktop" (whole screen) or "window" (one app window).
    pub capture_target: String,
    /// Exact window title to capture when capture_target is "window".
    pub capture_window: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            debloat: true,
            start_minimised: false,
            clipping_enabled: false,
            clip_seconds: 60,
            clip_fps: 60,
            clip_backend: "auto".to_string(),
            hotkey_enabled: true,
            hotkey: "Ctrl+Alt+C".to_string(),
            capture_target: "desktop".to_string(),
            capture_window: String::new(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(15..=600).contains(&self.clip_seconds) {
            return Err("clip_seconds must be between 15 and 600".into());
        }
        if !(15..=240).contains(&self.clip_fps) {
            return Err("clip_fps must be between 15 and 240".into());
        }
        if !matches!(self.clip_backend.as_str(), "auto" | "gpu" | "cpu") {
            return Err("clip_backend must be auto, gpu, or cpu".into());
        }
        if self.hotkey_enabled && self.hotkey.trim().is_empty() {
            return Err("hotkey must not be empty when enabled".into());
        }
        if !matches!(self.capture_target.as_str(), "desktop" | "window") {
            return Err("capture_target must be desktop or window".into());
        }
        if self.capture_target == "window" && self.capture_window.trim().is_empty() {
            return Err("choose a window to capture".into());
        }
        Ok(())
    }

    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert!(s.debloat);
        assert_eq!(s.clip_seconds, 60);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rejects_bad_backend_and_empty_hotkey() {
        let mut s = Settings::default();
        s.clip_backend = "quantum".into();
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.hotkey = "  ".into();
        assert!(s.validate().is_err());
        s.hotkey_enabled = false;
        assert!(s.validate().is_ok());
    }

    #[test]
    fn old_settings_json_without_new_fields_still_loads() {
        let dir = std::env::temp_dir().join("vetcord-test-settings-old");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"debloat":false,"clip_seconds":90}"#).unwrap();
        let s = Settings::load(&path);
        assert!(!s.debloat);
        assert_eq!(s.clip_seconds, 90);
        assert_eq!(s.clip_backend, "auto");
        assert_eq!(s.hotkey, "Ctrl+Alt+C");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn window_target_requires_a_window_title() {
        let mut s = Settings::default();
        s.capture_target = "window".into();
        assert!(s.validate().is_err());
        s.capture_window = "My Game".into();
        assert!(s.validate().is_ok());
        s.capture_target = "lid".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn rejects_out_of_range_clip_values() {
        let mut s = Settings::default();
        s.clip_seconds = 5;
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.clip_fps = 999;
        assert!(s.validate().is_err());
    }

    #[test]
    fn round_trips_through_json_file() {
        let dir = std::env::temp_dir().join("vetcord-test-settings");
        let path = dir.join("settings.json");
        let s = Settings { clip_seconds: 120, ..Settings::default() };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        let _ = std::fs::remove_dir_all(dir);
    }
}
