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
    /// Video bitrate for live streaming, in kbps.
    pub stream_bitrate_kbps: u32,
    /// Live-streaming destinations. Stream keys are NOT stored here;
    /// they live in the DPAPI-encrypted key file (see stream_keys.rs).
    pub stream_destinations: Vec<StreamDestination>,
}

/// One RTMP ingest target for multistreaming.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StreamDestination {
    /// Display name, also the key used to look up the stored stream key.
    pub name: String,
    /// "x", "rumble", or "custom".
    pub kind: String,
    /// Ingest server URL (rtmp:// or rtmps://), without the stream key.
    pub server: String,
    pub enabled: bool,
}

impl StreamDestination {
    /// The built-in destinations shown in streaming setup.
    pub fn presets() -> Vec<Self> {
        vec![
            Self {
                name: "X".to_string(),
                kind: "x".to_string(),
                server: Self::preset_server("x").to_string(),
                enabled: false,
            },
            Self {
                name: "Rumble".to_string(),
                kind: "rumble".to_string(),
                server: Self::preset_server("rumble").to_string(),
                enabled: false,
            },
        ]
    }

    pub fn preset_server(kind: &str) -> &'static str {
        match kind {
            // X (Twitter) Media Studio producer ingest.
            "x" => "rtmps://rtmp-ams.pscp.tv:443/x",
            // Rumble Studio / Rumble live RTMP ingest.
            "rumble" => "rtmp://rtmp.rumble.com/live",
            _ => "",
        }
    }
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
            stream_bitrate_kbps: 6000,
            stream_destinations: StreamDestination::presets(),
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
        if !matches!(self.capture_target.as_str(), "desktop" | "window" | "fullscreen") {
            return Err("capture_target must be desktop, window, or fullscreen".into());
        }
        if self.capture_target == "window" && self.capture_window.trim().is_empty() {
            return Err("choose a window to capture".into());
        }
        if !(1000..=20000).contains(&self.stream_bitrate_kbps) {
            return Err("stream_bitrate_kbps must be between 1000 and 20000".into());
        }
        for dest in &self.stream_destinations {
            if dest.name.trim().is_empty() {
                return Err("stream destination name must not be empty".into());
            }
            if dest.enabled
                && !dest.server.starts_with("rtmp://")
                && !dest.server.starts_with("rtmps://")
            {
                return Err(format!(
                    "stream destination '{}' has an invalid server URL",
                    dest.name
                ));
            }
        }
        Ok(())
    }

    pub fn load(path: &PathBuf) -> Self {
        let mut settings = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        // A saved empty list would make the X/Rumble setup rows vanish;
        // serde defaults only cover a *missing* field, not an empty one.
        if settings.stream_destinations.is_empty() {
            settings.stream_destinations = StreamDestination::presets();
        }
        settings
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
        // Streaming fields added in v0.3 must appear for pre-streaming
        // settings files, or the X/Rumble setup rows never render.
        assert_eq!(s.stream_bitrate_kbps, 6000);
        assert_eq!(s.stream_destinations.len(), 2);
        assert_eq!(s.stream_destinations[0].name, "X");
        assert_eq!(
            s.stream_destinations[0].server,
            StreamDestination::preset_server("x")
        );
        assert_eq!(s.stream_destinations[1].name, "Rumble");
        assert_eq!(
            s.stream_destinations[1].server,
            StreamDestination::preset_server("rumble")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_saved_stream_destinations_are_backfilled_with_presets() {
        // A settings file saved with an empty list must not leave the
        // streaming setup permanently blank in the UI.
        let dir = std::env::temp_dir().join("vetcord-test-settings-empty-dests");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"stream_destinations":[]}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.stream_destinations.len(), 2);
        assert_eq!(s.stream_destinations[0].name, "X");
        assert_eq!(s.stream_destinations[1].name, "Rumble");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stream_defaults_ship_x_and_rumble_disabled() {
        let s = Settings::default();
        assert_eq!(s.stream_bitrate_kbps, 6000);
        assert_eq!(s.stream_destinations.len(), 2);
        assert!(s.stream_destinations.iter().all(|d| !d.enabled));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rejects_bad_stream_settings() {
        let mut s = Settings::default();
        s.stream_bitrate_kbps = 50;
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.stream_destinations[0].enabled = true;
        s.stream_destinations[0].server = "https://example.com".into();
        assert!(s.validate().is_err());
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
