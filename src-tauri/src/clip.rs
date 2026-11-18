use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Automatic gameplay clipping: a rolling replay buffer.
///
/// ffmpeg records the desktop (gdigrab) as 5-second segments in a temp
/// buffer directory. A janitor thread deletes segments older than the
/// configured buffer length, so disk use stays bounded at roughly
/// `clip_seconds` of video. Saving a clip concatenates the newest closed
/// segments and trims them to exactly the requested length.
///
/// Note: this captures the whole desktop while running, like OBS replay
/// buffer. It only runs after the user enables/starts it.

pub const SEGMENT_SECONDS: u32 = 5;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize)]
pub struct ClipStatus {
    pub enabled: bool,
    pub capturing: bool,
    pub backend: String,
    pub buffer_seconds: u32,
    pub fps: u32,
    pub buffered_seconds: u32,
    pub output_dir: String,
    pub ffmpeg_available: bool,
    pub message: String,
    pub last_clip: Option<String>,
}

pub struct Recorder {
    child: Child,
    running: Arc<AtomicBool>,
    pub buffer_seconds: u32,
    pub fps: u32,
    /// Backend that actually started: "gpu" or "cpu".
    pub backend_used: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Gpu,
    Cpu,
    Window,
}

impl Backend {
    fn name(&self) -> &'static str {
        match self {
            Backend::Gpu => "gpu",
            Backend::Cpu => "cpu",
            Backend::Window => "window",
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Backend::Gpu => "GPU (ddagrab + NVENC)",
            Backend::Cpu => "CPU (gdigrab + x264)",
            Backend::Window => "Window (gdigrab window capture)",
        }
    }
    fn from_name(name: &str) -> Backend {
        match name {
            "gpu" => Backend::Gpu,
            "window" => Backend::Window,
            _ => Backend::Cpu,
        }
    }
}

/// Ordered backends to try. Window targets always use gdigrab title
/// capture (ddagrab only grabs whole displays). Unit-tested.
fn backend_candidates(preference: &str, capture_target: &str) -> Vec<Backend> {
    if capture_target == "window" {
        return vec![Backend::Window];
    }
    match preference {
        "gpu" => vec![Backend::Gpu],
        "cpu" => vec![Backend::Cpu],
        _ => vec![Backend::Gpu, Backend::Cpu],
    }
}

impl Recorder {
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

pub fn output_dir() -> PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Videos").join("VetCord").join("Clips")
}

pub fn buffer_dir() -> PathBuf {
    std::env::temp_dir().join("vetcord-buffer")
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    // PATH first, then the winget install location used on dev machines.
    if Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("ffmpeg"));
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    let packages = PathBuf::from(local).join("Microsoft").join("WinGet").join("Packages");
    for entry in std::fs::read_dir(packages).ok()?.flatten() {
        if entry.file_name().to_string_lossy().starts_with("Gyan.FFmpeg") {
            for file in walk_files(&entry.path(), 4) {
                if file.file_name().map(|n| n == "ffmpeg.exe").unwrap_or(false) {
                    return Some(file);
                }
            }
        }
    }
    None
}

fn walk_files(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if depth == 0 {
        return out;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_files(&path, depth - 1));
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// Segment file names, oldest first, excluding the segment being written.
pub fn closed_segments(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walk_files(dir, 1)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let name = n.to_string_lossy();
                    name.starts_with("seg-") && name.ends_with(".mp4")
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    // The newest file is still being written by ffmpeg; never read it.
    files.pop();
    files
}

pub fn buffered_seconds(dir: &Path) -> u32 {
    closed_segments(dir).len() as u32 * SEGMENT_SECONDS
}

/// Pure prune rule, unit-tested: keep the newest `keep` segments.
pub fn expired_segments(sorted: &[PathBuf], keep: usize) -> Vec<PathBuf> {
    if sorted.len() <= keep {
        return Vec::new();
    }
    sorted[..sorted.len() - keep].to_vec()
}

fn spawn_backend(
    ffmpeg: &Path,
    dir: &Path,
    backend: Backend,
    fps: u32,
    window_title: &str,
) -> Result<Child, String> {
    let log = std::fs::File::create(dir.join(format!("ffmpeg-{}.log", backend.name())))
        .map_err(|e| e.to_string())?;
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-y");
    match backend {
        // Desktop Duplication API capture on the GPU, NVENC encode.
        Backend::Gpu => {
            cmd.args([
                "-f", "lavfi",
                "-i", &format!(
                    "ddagrab=framerate={fps},hwdownload,format=bgra,format=nv12"
                ),
                "-c:v", "h264_nvenc",
                "-preset", "p4",
            ]);
        }
        Backend::Cpu => {
            cmd.args([
                "-f", "gdigrab",
                "-framerate", &fps.to_string(),
                "-i", "desktop",
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-pix_fmt", "yuv420p",
            ]);
        }
        // One application window by exact title (e.g. a game).
        Backend::Window => {
            cmd.args(["-f", "gdigrab", "-framerate", &fps.to_string(), "-i"])
                .arg(format!("title={window_title}"))
                .args(["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"]);
        }
    }
    cmd.args([
        "-f", "segment",
        "-segment_time", &SEGMENT_SECONDS.to_string(),
        "-reset_timestamps", "1",
        "-strftime", "1",
    ])
    .arg(dir.join("seg-%Y%m%d-%H%M%S.mp4"))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map_err(|e| format!("failed to start ffmpeg: {e}"))
}

pub fn start_recorder(
    buffer_seconds: u32,
    fps: u32,
    backend_preference: &str,
    capture_target: &str,
    capture_window: &str,
) -> Result<Recorder, String> {
    if capture_target == "window" && capture_window.trim().is_empty() {
        return Err("choose a window to capture first".into());
    }
    let ffmpeg = find_ffmpeg().ok_or(
        "ffmpeg not found. Install it with: winget install -e --id Gyan.FFmpeg",
    )?;
    let dir = buffer_dir();
    // Start from a clean buffer so stale segments are never clipped.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Try each candidate; a backend that exits during warm-up falls
    // through to the next one (e.g. no NVENC GPU -> CPU capture).
    let mut errors = Vec::new();
    let mut started = None;
    for backend in backend_candidates(backend_preference, capture_target) {
        match spawn_backend(&ffmpeg, &dir, backend, fps, capture_window) {
            Ok(mut child) => {
                std::thread::sleep(Duration::from_millis(1200));
                if matches!(child.try_wait(), Ok(None)) {
                    started = Some((child, backend));
                    break;
                }
                errors.push(format!("{} exited during warm-up", backend.name()));
            }
            Err(e) => errors.push(format!("{}: {e}", backend.name())),
        }
        // Clear any partial segments before the next candidate.
        for segment in closed_segments(&dir) {
            let _ = std::fs::remove_file(segment);
        }
    }
    let (child, backend) = started.ok_or_else(|| {
        format!("no capture backend started ({})", errors.join("; "))
    })?;

    let running = Arc::new(AtomicBool::new(true));
    let janitor_flag = running.clone();
    let keep = ((buffer_seconds + SEGMENT_SECONDS - 1) / SEGMENT_SECONDS) as usize;
    std::thread::spawn(move || {
        while janitor_flag.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(2));
            let mut all = closed_segments(&buffer_dir());
            // Keep one extra so `closed_segments`' in-progress pop stays valid.
            all.push(PathBuf::new());
            for expired in expired_segments(&all, keep + 1) {
                if !expired.as_os_str().is_empty() {
                    let _ = std::fs::remove_file(expired);
                }
            }
        }
    });

    Ok(Recorder {
        child,
        running,
        buffer_seconds,
        fps,
        backend_used: backend.name().to_string(),
    })
}

pub fn stop_recorder(recorder: &mut Recorder) {
    recorder.running.store(false, Ordering::Relaxed);
    let _ = recorder.child.kill();
    let _ = recorder.child.wait();
    let _ = std::fs::remove_dir_all(buffer_dir());
}

/// Concatenate the newest segments covering `seconds` and trim exactly.
pub fn save_clip(seconds: u32) -> Result<String, String> {
    let ffmpeg = find_ffmpeg().ok_or("ffmpeg not found")?;
    let dir = buffer_dir();
    let segments = closed_segments(&dir);
    if segments.is_empty() {
        return Err(
            "No buffered footage yet - the replay buffer is still warming up (wait a few seconds)"
                .into(),
        );
    }
    let needed = ((seconds + SEGMENT_SECONDS - 1) / SEGMENT_SECONDS) as usize;
    let take = needed.min(segments.len());
    let chosen = &segments[segments.len() - take..];
    let available = take as u32 * SEGMENT_SECONDS;
    let trim = available.saturating_sub(seconds);

    let out_dir = output_dir();
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let output = out_dir.join(format!("vetcord-clip-{epoch}.mp4"));

    let list_path = dir.join("concat.txt");
    let list = chosen
        .iter()
        .map(|p| format!("file '{}'", p.to_string_lossy().replace('\\', "/")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_path, list).map_err(|e| e.to_string())?;

    let status = Command::new(&ffmpeg)
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args([
            "-ss", &trim.to_string(),
            "-t", &seconds.min(available).to_string(),
            "-c:v", "libx264",
            "-preset", "veryfast",
            "-pix_fmt", "yuv420p",
        ])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&list_path);
    if !status.success() || !output.exists() {
        return Err("ffmpeg failed to write the clip".into());
    }
    Ok(output.to_string_lossy().into_owned())
}

pub fn status(
    recorder: Option<&Recorder>,
    enabled_setting: bool,
    buffer_seconds: u32,
    fps: u32,
    last_clip: Option<String>,
) -> ClipStatus {
    let capturing = recorder.is_some();
    let backend = recorder
        .map(|r| Backend::from_name(&r.backend_used).label())
        .unwrap_or("not capturing");
    ClipStatus {
        enabled: enabled_setting || capturing,
        capturing,
        backend: backend.into(),
        buffer_seconds,
        fps,
        buffered_seconds: if capturing { buffered_seconds(&buffer_dir()) } else { 0 },
        output_dir: output_dir().to_string_lossy().into_owned(),
        ffmpeg_available: find_ffmpeg().is_some(),
        message: if capturing {
            "Capturing the desktop into the replay buffer".into()
        } else if find_ffmpeg().is_none() {
            "ffmpeg not found - install: winget install -e --id Gyan.FFmpeg".into()
        } else {
            "Clipping is stopped".into()
        },
        last_clip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_dir_ends_with_vetcord_clips() {
        let dir = output_dir();
        assert!(dir.ends_with("Clips"));
        assert!(dir.to_string_lossy().contains("VetCord"));
    }

    #[test]
    fn backend_candidates_follow_preference() {
        assert_eq!(
            backend_candidates("auto", "desktop"),
            vec![Backend::Gpu, Backend::Cpu]
        );
        assert_eq!(backend_candidates("gpu", "desktop"), vec![Backend::Gpu]);
        assert_eq!(backend_candidates("cpu", "desktop"), vec![Backend::Cpu]);
        assert_eq!(
            backend_candidates("junk", "desktop"),
            vec![Backend::Gpu, Backend::Cpu]
        );
        // Window capture ignores the GPU/CPU preference.
        assert_eq!(backend_candidates("gpu", "window"), vec![Backend::Window]);
        assert_eq!(backend_candidates("auto", "window"), vec![Backend::Window]);
    }

    #[test]
    fn prune_keeps_only_newest_segments() {
        let files: Vec<PathBuf> =
            (0..10).map(|i| PathBuf::from(format!("seg-{i:02}.mp4"))).collect();
        let expired = expired_segments(&files, 3);
        assert_eq!(expired.len(), 7);
        assert_eq!(expired[6], PathBuf::from("seg-06.mp4"));
        assert!(expired_segments(&files[..3], 3).is_empty());
    }

    /// Real end-to-end check: record the desktop, save a clip, verify it.
    /// Run with: cargo test clip_e2e -- --ignored --nocapture
    #[test]
    #[ignore]
    fn clip_e2e_records_and_saves_a_real_mp4() {
        let mut recorder =
            start_recorder(60, 30, "auto", "desktop", "").expect("recorder starts");
        println!("backend used: {}", recorder.backend_used);
        std::thread::sleep(Duration::from_secs(19));
        assert!(recorder.is_alive(), "ffmpeg recorder still running");
        let path = save_clip(10).expect("clip saved");
        stop_recorder(&mut recorder);
        let meta = std::fs::metadata(&path).expect("clip file exists");
        assert!(meta.len() > 10_000, "clip has real content: {} bytes", meta.len());
        let output = Command::new(find_ffmpeg().unwrap())
            .args(["-i"])
            .arg(&path)
            .output()
            .unwrap();
        let info = String::from_utf8_lossy(&output.stderr);
        assert!(info.contains("Video:"), "clip contains a video stream: {info}");
        println!("saved clip: {path} ({} bytes)", meta.len());
    }

    /// Per-window capture: record a real open window, save a clip.
    /// Run with: cargo test clip_window_e2e -- --ignored --nocapture
    #[test]
    #[ignore]
    fn clip_window_e2e_captures_one_window() {
        let titles = crate::window_list::list_window_titles();
        let target = titles
            .iter()
            .find(|t| t.contains("Notepad"))
            .or_else(|| titles.first())
            .expect("at least one visible window to capture")
            .clone();
        println!("capturing window: {target}");
        let mut recorder =
            start_recorder(60, 30, "auto", "window", &target).expect("window recorder starts");
        assert_eq!(recorder.backend_used, "window");
        std::thread::sleep(Duration::from_secs(13));
        let path = save_clip(5).expect("window clip saved");
        stop_recorder(&mut recorder);
        let meta = std::fs::metadata(&path).expect("clip exists");
        assert!(meta.len() > 5_000, "clip has content: {} bytes", meta.len());
        println!("saved window clip: {path} ({} bytes)", meta.len());
    }

    #[test]
    fn save_without_buffer_errors_instead_of_faking_a_clip() {
        // With an empty/nonexistent buffer dir there can be no clip.
        let dir = buffer_dir();
        if !dir.exists() || closed_segments(&dir).is_empty() {
            assert!(save_clip(30).is_err());
        }
    }
}
