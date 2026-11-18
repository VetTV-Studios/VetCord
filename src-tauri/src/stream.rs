//! Live multistreaming: one ffmpeg process pushes the gameplay capture
//! to several RTMP ingests at once via the tee muxer. Each output uses
//! `onfail=ignore`, so one dead ingest does not kill the others.
//!
//! Note: Discord has no RTMP ingest for user accounts, so streaming
//! "to a Discord server" still happens via Go Live inside the client.
//! This engine targets X, Rumble, and any custom RTMP server.
//!
//! Verification status: the ffmpeg argument construction is unit-tested
//! and the tee syntax exercised against file outputs and a local RTMP
//! sink, but live ingestion against real X/Rumble accounts requires the
//! user's own stream keys and is not covered here.

use crate::settings::Settings;
use crate::stream_keys::{full_rtmp_url, mask_url, KeyStore};
use serde::Serialize;
use std::process::{Child, Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize)]
pub struct StreamStatus {
    pub streaming: bool,
    /// Masked destination URLs (stream keys are never exposed).
    pub destinations: Vec<String>,
    pub bitrate_kbps: u32,
    pub message: String,
}

pub struct Streamer {
    child: Child,
    pub destinations: Vec<String>,
    pub bitrate_kbps: u32,
}

impl Streamer {
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// Pure argument builder, unit-tested without spawning ffmpeg.
/// `urls` are full RTMP URLs including stream keys; they must never be
/// logged or returned to the UI.
pub fn build_stream_args(settings: &Settings, urls: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec!["-y".into()];
    let fps = settings.clip_fps.to_string();
    let gpu_input = matches!(settings.clip_backend.as_str(), "gpu" | "auto")
        || settings.capture_target == "fullscreen";
    match settings.capture_target.as_str() {
        "window" => {
            args.extend([
                "-f".into(),
                "gdigrab".into(),
                "-framerate".into(),
                fps,
                "-i".into(),
                format!("title={}", settings.capture_window),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "veryfast".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]);
        }
        _ if gpu_input => {
            args.extend([
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                format!("ddagrab=framerate={fps},hwdownload,format=bgra,format=nv12"),
                "-c:v".into(),
                "h264_nvenc".into(),
                "-preset".into(),
                "p4".into(),
            ]);
        }
        _ => {
            args.extend([
                "-f".into(),
                "gdigrab".into(),
                "-framerate".into(),
                fps,
                "-i".into(),
                "desktop".into(),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "veryfast".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]);
        }
    }
    // The tee muxer does not auto-select streams; map video explicitly.
    args.extend(["-map".into(), "0:v".into()]);
    let bitrate = format!("{}k", settings.stream_bitrate_kbps);
    args.extend([
        "-b:v".into(),
        bitrate.clone(),
        "-maxrate".into(),
        bitrate,
        "-bufsize".into(),
        format!("{}k", settings.stream_bitrate_kbps * 2),
        "-g".into(),
        (settings.clip_fps * 2).to_string(),
    ]);
    if urls.len() == 1 {
        args.extend(["-f".into(), "flv".into(), urls[0].clone()]);
    } else {
        let tee = urls
            .iter()
            .map(|u| format!("[f=flv:onfail=ignore]{u}"))
            .collect::<Vec<_>>()
            .join("|");
        args.extend(["-f".into(), "tee".into(), tee]);
    }
    args
}

pub fn start_stream(settings: &Settings, keys: &KeyStore) -> Result<Streamer, String> {
    let ffmpeg = crate::clip::find_ffmpeg().ok_or(
        "ffmpeg not found. Install it with: winget install -e --id Gyan.FFmpeg",
    )?;
    let mut urls = Vec::new();
    let mut masked = Vec::new();
    for dest in &settings.stream_destinations {
        if !dest.enabled {
            continue;
        }
        let key = keys.get(&dest.name).ok_or_else(|| {
            format!("no stream key saved for '{}' - add it in Settings", dest.name)
        })?;
        let url = full_rtmp_url(&dest.server, &key);
        masked.push(mask_url(&url));
        urls.push(url);
    }
    if urls.is_empty() {
        return Err("no streaming destinations enabled".into());
    }
    if settings.capture_target == "window" && settings.capture_window.trim().is_empty() {
        return Err("choose a window to stream first".into());
    }
    let args = build_stream_args(settings, &urls);
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd.spawn().map_err(|e| format!("failed to start ffmpeg: {e}"))?;
    Ok(Streamer {
        child,
        destinations: masked,
        bitrate_kbps: settings.stream_bitrate_kbps,
    })
}

pub fn stop_stream(streamer: &mut Streamer) {
    let _ = streamer.child.kill();
    let _ = streamer.child.wait();
}

pub fn status(streamer: Option<&Streamer>, settings: &Settings) -> StreamStatus {
    match streamer {
        Some(s) => StreamStatus {
            streaming: true,
            destinations: s.destinations.clone(),
            bitrate_kbps: s.bitrate_kbps,
            message: "Live - pushing to all enabled destinations".into(),
        },
        None => StreamStatus {
            streaming: false,
            destinations: settings
                .stream_destinations
                .iter()
                .filter(|d| d.enabled)
                .map(|d| mask_url(&full_rtmp_url(&d.server, "key")))
                .collect(),
            bitrate_kbps: settings.stream_bitrate_kbps,
            message: "Not streaming".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_destination_uses_plain_flv_output() {
        let s = Settings::default();
        let args = build_stream_args(&s, &["rtmp://example.com/live/KEY".into()]);
        let joined = args.join(" ");
        assert!(joined.contains("-f flv rtmp://example.com/live/KEY"));
        assert!(!joined.contains("tee"));
        assert!(joined.contains("6000k"));
    }

    #[test]
    fn multistream_uses_tee_with_onfail_ignore() {
        let s = Settings::default();
        let args = build_stream_args(
            &s,
            &[
                "rtmps://a.example.com/x/K1".into(),
                "rtmp://b.example.com/live/K2".into(),
            ],
        );
        let joined = args.join(" ");
        assert!(joined.contains("-f tee"));
        assert!(joined.contains("-map 0:v"), "tee needs explicit mapping: {joined}");
        assert_eq!(joined.matches("onfail=ignore").count(), 2);
    }

    #[test]
    fn window_target_streams_the_chosen_window() {
        let mut s = Settings::default();
        s.capture_target = "window".into();
        s.capture_window = "My Game".into();
        let args = build_stream_args(&s, &["rtmp://example.com/live/K".into()]);
        assert!(args.join(" ").contains("title=My Game"));
    }

    #[test]
    fn fullscreen_target_forces_gpu_input() {
        let mut s = Settings::default();
        s.clip_backend = "cpu".into();
        s.capture_target = "fullscreen".into();
        let args = build_stream_args(&s, &["rtmp://example.com/live/K".into()]);
        let joined = args.join(" ");
        assert!(joined.contains("ddagrab"));
        assert!(joined.contains("h264_nvenc"));
    }
}
