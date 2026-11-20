#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clip;
mod debloat;
mod settings;
mod stream;
mod stream_keys;
mod suggestions;
mod window_list;

use settings::Settings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::webview::{PermissionKind, PermissionResponse};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

const DISCORD_APP_URL: &str = "https://discord.com/app";
// A current desktop Chrome UA, as Vesktop uses, so Discord serves its
// full desktop web client to WebView2 instead of a degraded page.
const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

struct AppState {
    settings_path: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    /// Per-launch nonce for the in-Discord VetCord panel bridge URLs.
    bridge_token: String,
    popups: AtomicU32,
    recorder: std::sync::Mutex<Option<clip::Recorder>>,
    streamer: std::sync::Mutex<Option<stream::Streamer>>,
    last_clip: std::sync::Mutex<Option<String>>,
    registered_hotkey: std::sync::Mutex<Option<String>>,
}

fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("settings.json")
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    Settings::load(&state.settings_path)
}

/// (Re)register the global save-clip hotkey from settings.
/// An invalid shortcut is an error only when the hotkey is enabled.
fn apply_hotkey(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let state = app.state::<AppState>();
    let mut registered = state.registered_hotkey.lock().unwrap();
    if let Some(old) = registered.take() {
        let _ = app.global_shortcut().unregister(old.as_str());
    }
    if settings.hotkey_enabled {
        // Registration itself validates the shortcut string.
        app.global_shortcut()
            .register(settings.hotkey.as_str())
            .map_err(|e| format!("invalid or unavailable hotkey '{}': {e}", settings.hotkey))?;
        *registered = Some(settings.hotkey.clone());
    }
    Ok(())
}

/// Save a clip from any entry point (command, tray, hotkey).
fn do_save_clip(state: &AppState) -> Result<String, String> {
    let s = Settings::load(&state.settings_path);
    {
        let guard = state.recorder.lock().unwrap();
        if guard.is_none() {
            return Err("Clipping is not running - start it first".into());
        }
    }
    let path = clip::save_clip(s.clip_seconds)?;
    *state.last_clip.lock().unwrap() = Some(path.clone());
    Ok(path)
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    settings.save(&state.settings_path)?;
    apply_hotkey(&app, &settings)
}

#[tauri::command]
fn get_clip_status(state: State<'_, AppState>) -> clip::ClipStatus {
    let s = Settings::load(&state.settings_path);
    let mut guard = state.recorder.lock().unwrap();
    // If ffmpeg died (e.g. display change), report stopped, not capturing.
    if guard.as_mut().map(|r| !r.is_alive()).unwrap_or(false) {
        *guard = None;
    }
    let (buffer_seconds, fps) = guard
        .as_ref()
        .map(|r| (r.buffer_seconds, r.fps))
        .unwrap_or((s.clip_seconds, s.clip_fps));
    let last_clip = state.last_clip.lock().unwrap().clone();
    clip::status(guard.as_ref(), s.clipping_enabled, buffer_seconds, fps, last_clip)
}

#[tauri::command]
fn list_windows() -> Vec<String> {
    window_list::list_window_titles()
}

#[tauri::command]
fn start_clipping(state: State<'_, AppState>) -> Result<(), String> {
    let s = Settings::load(&state.settings_path);
    let mut guard = state.recorder.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    *guard = Some(clip::start_recorder(
        s.clip_seconds,
        s.clip_fps,
        &s.clip_backend,
        &s.capture_target,
        &s.capture_window,
    )?);
    Ok(())
}

#[tauri::command]
fn stop_clipping(state: State<'_, AppState>) {
    let mut guard = state.recorder.lock().unwrap();
    if let Some(mut recorder) = guard.take() {
        clip::stop_recorder(&mut recorder);
    }
}

#[tauri::command]
fn save_clip(state: State<'_, AppState>) -> Result<String, String> {
    do_save_clip(&state)
}

fn key_store(state: &AppState) -> stream_keys::KeyStore {
    stream_keys::KeyStore::new(state.config_dir.clone())
}

#[tauri::command]
fn set_stream_key(
    state: State<'_, AppState>,
    destination: String,
    key: String,
) -> Result<(), String> {
    if key.trim().is_empty() {
        key_store(&state).clear(&destination)
    } else {
        key_store(&state).set(&destination, key.trim())
    }
}

/// Destination names that have a stored key. Keys themselves never
/// leave the Rust side.
#[tauri::command]
fn stream_keys_present(state: State<'_, AppState>) -> Vec<String> {
    let s = Settings::load(&state.settings_path);
    let store = key_store(&state);
    s.stream_destinations
        .iter()
        .filter(|d| store.has(&d.name))
        .map(|d| d.name.clone())
        .collect()
}

#[tauri::command]
fn start_stream(state: State<'_, AppState>) -> Result<(), String> {
    let s = Settings::load(&state.settings_path);
    let mut guard = state.streamer.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    *guard = Some(stream::start_stream(&s, &key_store(&state))?);
    Ok(())
}

#[tauri::command]
fn stop_stream(state: State<'_, AppState>) {
    let mut guard = state.streamer.lock().unwrap();
    if let Some(mut streamer) = guard.take() {
        stream::stop_stream(&mut streamer);
    }
}

#[tauri::command]
fn get_stream_status(state: State<'_, AppState>) -> stream::StreamStatus {
    let s = Settings::load(&state.settings_path);
    let mut guard = state.streamer.lock().unwrap();
    // If ffmpeg died, report stopped rather than "live".
    if guard.as_mut().map(|st| !st.is_alive()).unwrap_or(false) {
        *guard = None;
    }
    stream::status(guard.as_ref(), &s)
}

#[tauri::command]
fn submit_suggestion(
    state: State<'_, AppState>,
    title: String,
    details: String,
) -> Result<(), String> {
    suggestions::append(
        &suggestions::suggestions_path(&state.data_dir),
        &title,
        &details,
    )?;
    Ok(())
}

#[tauri::command]
fn list_suggestions(state: State<'_, AppState>) -> Vec<suggestions::Suggestion> {
    suggestions::list(&suggestions::suggestions_path(&state.data_dir))
}

#[tauri::command]
fn github_issues_url() -> &'static str {
    suggestions::GITHUB_ISSUES_URL
}

/// Open Settings, optionally scrolled to a section anchor
/// (e.g. "#streaming" for the X/Rumble streaming setup).
fn open_settings_at(app: tauri::AppHandle, anchor: &str) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_focus();
        if !anchor.is_empty() {
            let _ = window.eval(format!(
                "document.querySelector('{anchor}')?.scrollIntoView()"
            ));
        }
        return Ok(());
    }
    WebviewWindowBuilder::new(
        &app,
        "settings",
        WebviewUrl::App(format!("index.html{anchor}").into()),
    )
    .title("VetCord Settings")
    .inner_size(560.0, 760.0)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    open_settings_at(app, "")
}

#[tauri::command]
fn open_streaming_setup(app: tauri::AppHandle) -> Result<(), String> {
    open_settings_at(app, "#streaming")
}

fn is_discord_url(url: &url::Url) -> bool {
    matches!(
        url.host_str(),
        Some(
            "discord.com"
                | "www.discord.com"
                | "canary.discord.com"
                | "ptb.discord.com"
                | "discordapp.com"
                | "cdn.discordapp.com"
                | "media.discordapp.net"
                | "accounts.google.com"
                | "appleid.apple.com"
        )
    ) || url
        .host_str()
        .map(|h| h.ends_with(".discord.com") || h.ends_with(".discordapp.com"))
        .unwrap_or(false)
}

/// Decide a WebView2 permission request. WebView2 has no built-in
/// prompt UI, so without this handler Discord's getUserMedia calls are
/// refused and Chromium only exposes the "default" microphone, camera
/// and speaker in Discord's Voice & Video settings. Allowing mic/camera
/// (and notifications) for Discord origins makes enumerateDevices
/// return every real device. All other kinds and origins fail closed
/// to the platform default.
fn media_permission(kind: PermissionKind, url: &url::Url) -> PermissionResponse {
    if !is_discord_url(url) {
        return PermissionResponse::Default;
    }
    match kind {
        PermissionKind::Microphone | PermissionKind::Camera | PermissionKind::Notifications => {
            PermissionResponse::Allow
        }
        _ => PermissionResponse::Default,
    }
}

/// Origins whose mic/camera/notification permissions are pre-granted.
#[cfg(windows)]
const DISCORD_ORIGINS: [&str; 3] = [
    "https://discord.com",
    "https://canary.discord.com",
    "https://ptb.discord.com",
];

/// Pre-grant Discord's media permissions in the WebView2 profile.
/// Answering PermissionRequested alone is not enough: Discord only
/// enumerates audio/video devices after a grant exists, and it does
/// not request one just by opening Voice & Video settings, so the
/// dropdowns stay on "Default". Setting the profile permission state
/// up front makes enumerateDevices return every real device without
/// ever activating a mic or camera. Best-effort: older WebView2
/// runtimes without ICoreWebView2Profile4 simply skip this and fall
/// back to the PermissionRequested handler plus the panel script's
/// getUserMedia prime in Voice & Video settings.
#[cfg(windows)]
fn pregrant_discord_permissions(window: &tauri::WebviewWindow) {
    let _ = window.with_webview(|webview| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Profile4, ICoreWebView2_13, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
            COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS,
            COREWEBVIEW2_PERMISSION_STATE_ALLOW,
        };
        use webview2_com::SetPermissionStateCompletedHandler;
        use windows_core::{Interface, PCWSTR};

        let Ok(core) = webview.controller().CoreWebView2() else {
            return;
        };
        let Ok(core13) = core.cast::<ICoreWebView2_13>() else {
            return;
        };
        let Ok(profile) = core13.Profile() else {
            return;
        };
        let Ok(profile4) = profile.cast::<ICoreWebView2Profile4>() else {
            return;
        };
        let handler = SetPermissionStateCompletedHandler::create(Box::new(|_| Ok(())));
        for origin in DISCORD_ORIGINS {
            let wide: Vec<u16> = origin.encode_utf16().chain([0]).collect();
            for kind in [
                COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                COREWEBVIEW2_PERMISSION_KIND_CAMERA,
                COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS,
            ] {
                let _ = profile4.SetPermissionState(
                    kind,
                    PCWSTR(wide.as_ptr()),
                    COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                    &handler,
                );
            }
        }
    });
}

/// Per-launch nonce embedded only in VetCord's injected panel script.
/// Bridge URLs without it are ignored (fail closed).
fn new_bridge_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ ((std::process::id() as u64) << 32) ^ 0x9e37_79b9_7f4a_7c15;
    let mut out = String::new();
    for _ in 0..4 {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        out.push_str(&format!("{:016x}", x.wrapping_mul(0x2545_f491_4f6c_dd1d)));
    }
    out
}

/// Actions the in-Discord VetCord panel may request via bridge URLs.
/// Deliberately narrow: no stream-key access. Stream keys are entered
/// only in the native Settings window.
#[derive(Debug, PartialEq)]
enum BridgeAction {
    OpenSettings,
    OpenStreamingSetup,
    ToggleClipping,
    SaveClip,
    ToggleStream,
    SetDebloat(bool),
    SetStreamEnabled { index: usize, enabled: bool },
}

fn parse_bridge_action(url: &url::Url, token: &str) -> Option<BridgeAction> {
    if token.is_empty() || url.host_str() != Some("vetcord.bridge") {
        return None;
    }
    let mut segments = url.path_segments()?;
    if segments.next()? != token {
        return None;
    }
    let query = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    match segments.next()? {
        "open-settings" => Some(BridgeAction::OpenSettings),
        "open-streaming" => Some(BridgeAction::OpenStreamingSetup),
        "toggle-clip" => Some(BridgeAction::ToggleClipping),
        "save-clip" => Some(BridgeAction::SaveClip),
        "toggle-stream" => Some(BridgeAction::ToggleStream),
        "set-debloat" => match query("v")?.as_str() {
            "1" => Some(BridgeAction::SetDebloat(true)),
            "0" => Some(BridgeAction::SetDebloat(false)),
            _ => None,
        },
        "set-stream-enabled" => {
            let index: usize = query("i")?.parse().ok()?;
            let enabled = match query("v")?.as_str() {
                "1" => true,
                "0" => false,
                _ => return None,
            };
            Some(BridgeAction::SetStreamEnabled { index, enabled })
        }
        _ => None,
    }
}

fn perform_bridge_action(app: &tauri::AppHandle, action: BridgeAction) {
    match action {
        BridgeAction::OpenSettings => {
            let _ = open_settings(app.clone());
        }
        BridgeAction::OpenStreamingSetup => {
            let _ = open_streaming_setup(app.clone());
        }
        BridgeAction::ToggleClipping => {
            if get_clip_status(app.state::<AppState>()).capturing {
                stop_clipping(app.state::<AppState>());
            } else {
                let _ = start_clipping(app.state::<AppState>());
            }
        }
        BridgeAction::SaveClip => {
            let _ = save_clip(app.state::<AppState>());
        }
        BridgeAction::ToggleStream => {
            if get_stream_status(app.state::<AppState>()).streaming {
                stop_stream(app.state::<AppState>());
            } else {
                let _ = start_stream(app.state::<AppState>());
            }
        }
        BridgeAction::SetDebloat(enabled) => {
            let state = app.state::<AppState>();
            let mut s = Settings::load(&state.settings_path);
            s.debloat = enabled;
            let _ = s.save(&state.settings_path);
        }
        BridgeAction::SetStreamEnabled { index, enabled } => {
            let state = app.state::<AppState>();
            let mut s = Settings::load(&state.settings_path);
            if let Some(dest) = s.stream_destinations.get_mut(index) {
                dest.enabled = enabled;
                let _ = s.save(&state.settings_path);
            }
        }
    }
}

fn open_main_window(app: &tauri::AppHandle, settings: &Settings) -> tauri::Result<()> {
    let handle = app.clone();
    let bridge_token = app.state::<AppState>().bridge_token.clone();
    let script = debloat::initialization_script(settings, &bridge_token);
    WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External(DISCORD_APP_URL.parse().expect("static URL")),
    )
    .title("VetCord")
    .inner_size(1280.0, 800.0)
    .min_inner_size(940.0, 500.0)
    .user_agent(CHROME_USER_AGENT)
    .initialization_script(&script)
    .on_permission_request(|webview, kind| match webview.url() {
        Ok(url) => media_permission(kind, &url),
        Err(_) => PermissionResponse::Default,
    })
    .on_navigation(|url| is_discord_url(url))
    .on_new_window(move |url, _features| {
        // VetCord panel bridge: our injected script opens
        // https://vetcord.bridge/<token>/<action> as a new window.
        // Perform the whitelisted action and never open a window for it.
        if url.host_str() == Some("vetcord.bridge") {
            let token = handle.state::<AppState>().bridge_token.clone();
            if let Some(action) = parse_bridge_action(&url, &token) {
                perform_bridge_action(&handle, action);
            }
            return tauri::webview::NewWindowResponse::Deny;
        }
        // OAuth / login popups open as VetCord windows, not system browser tabs.
        let n = handle
            .state::<AppState>()
            .popups
            .fetch_add(1, Ordering::Relaxed);
        match WebviewWindowBuilder::new(
            &handle,
            format!("popup-{n}"),
            WebviewUrl::External(url),
        )
        .title("VetCord")
        .inner_size(900.0, 700.0)
        .user_agent(CHROME_USER_AGENT)
        .build()
        {
            Ok(window) => tauri::webview::NewWindowResponse::Create { window },
            Err(_) => tauri::webview::NewWindowResponse::Deny,
        }
    })
    .build()
    .map(|window| {
        #[cfg(windows)]
        pregrant_discord_permissions(&window);
        window
    })?;
    Ok(())
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "VetCord Settings", true, None::<&str>)?;
    let streaming_item = MenuItem::with_id(
        app,
        "streaming_setup",
        "Live Streaming Setup (X / Rumble)",
        true,
        None::<&str>,
    )?;
    let clip_item = MenuItem::with_id(app, "save_clip", "Save Clip", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit VetCord", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&settings_item, &streaming_item, &clip_item, &quit_item],
    )?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
        .expect("embedded icon");
    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("VetCord")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                let _ = open_settings(app.clone());
            }
            "streaming_setup" => {
                let _ = open_streaming_setup(app.clone());
            }
            "save_clip" => {
                let _ = save_clip(app.state::<AppState>());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    // Save on a thread: encoding a clip takes seconds and
                    // must not block the shortcut/OS event loop.
                    let handle = app.clone();
                    std::thread::spawn(move || {
                        let state = handle.state::<AppState>();
                        let _ = do_save_clip(&state);
                    });
                })
                .build(),
        )
        .setup(|app| {
            let path = settings_path(app.handle());
            let settings = Settings::load(&path);
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir());
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir());
            app.manage(AppState {
                settings_path: path,
                config_dir,
                data_dir,
                bridge_token: new_bridge_token(),
                popups: AtomicU32::new(0),
                recorder: std::sync::Mutex::new(None),
                streamer: std::sync::Mutex::new(None),
                last_clip: std::sync::Mutex::new(None),
                registered_hotkey: std::sync::Mutex::new(None),
            });
            build_tray(app)?;
            open_main_window(app.handle(), &settings)?;
            // A bad saved hotkey must not stop the app launching; it is
            // reported and can be fixed the next time Settings is saved.
            let _ = apply_hotkey(app.handle(), &settings);
            if settings.clipping_enabled {
                // Auto-start the replay buffer; failure is visible in Settings.
                let handle = app.handle().clone();
                let state = handle.state::<AppState>();
                if let Ok(recorder) = clip::start_recorder(
                    settings.clip_seconds,
                    settings.clip_fps,
                    &settings.clip_backend,
                    &settings.capture_target,
                    &settings.capture_window,
                ) {
                    *state.recorder.lock().unwrap() = Some(recorder);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_clip_status,
            list_windows,
            start_clipping,
            stop_clipping,
            save_clip,
            set_stream_key,
            stream_keys_present,
            start_stream,
            stop_stream,
            get_stream_status,
            submit_suggestion,
            list_suggestions,
            github_issues_url,
            open_settings,
            open_streaming_setup
        ])
        .build(tauri::generate_context!())
        .expect("error while building VetCord")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Never leave ffmpeg capturing/streaming after VetCord closes.
                let state = app.state::<AppState>();
                let mut guard = state.recorder.lock().unwrap();
                if let Some(mut recorder) = guard.take() {
                    clip::stop_recorder(&mut recorder);
                }
                let mut guard = state.streamer.lock().unwrap();
                if let Some(mut streamer) = guard.take() {
                    stream::stop_stream(&mut streamer);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_media_permissions_are_allowed_so_devices_enumerate() {
        use tauri::webview::{PermissionKind, PermissionResponse};
        let discord: url::Url = "https://discord.com/app".parse().unwrap();
        // Without Allow, getUserMedia is refused and Chromium only ever
        // exposes the "default" mic/camera/speaker to Discord settings.
        assert_eq!(
            media_permission(PermissionKind::Microphone, &discord),
            PermissionResponse::Allow
        );
        assert_eq!(
            media_permission(PermissionKind::Camera, &discord),
            PermissionResponse::Allow
        );
        assert_eq!(
            media_permission(PermissionKind::Notifications, &discord),
            PermissionResponse::Allow
        );
        // Everything else, and every non-Discord origin, keeps the
        // platform default (fail closed).
        assert_eq!(
            media_permission(PermissionKind::Geolocation, &discord),
            PermissionResponse::Default
        );
        let other: url::Url = "https://example.com/".parse().unwrap();
        assert_eq!(
            media_permission(PermissionKind::Microphone, &other),
            PermissionResponse::Default
        );
        assert_eq!(
            media_permission(PermissionKind::Camera, &other),
            PermissionResponse::Default
        );
    }

    #[test]
    fn bridge_actions_require_the_per_launch_token() {
        let token = "abc123token";
        let ok: url::Url = "https://vetcord.bridge/abc123token/open-streaming".parse().unwrap();
        assert_eq!(
            parse_bridge_action(&ok, token),
            Some(BridgeAction::OpenStreamingSetup)
        );
        let dest: url::Url = "https://vetcord.bridge/abc123token/set-stream-enabled?i=1&v=1"
            .parse()
            .unwrap();
        assert_eq!(
            parse_bridge_action(&dest, token),
            Some(BridgeAction::SetStreamEnabled {
                index: 1,
                enabled: true
            })
        );
        // Fail closed: wrong token, wrong host, unknown action, bad value.
        let wrong: url::Url = "https://vetcord.bridge/nope/open-streaming".parse().unwrap();
        assert_eq!(parse_bridge_action(&wrong, token), None);
        let host: url::Url = "https://evil.example/abc123token/open-streaming".parse().unwrap();
        assert_eq!(parse_bridge_action(&host, token), None);
        let unknown: url::Url = "https://vetcord.bridge/abc123token/set-stream-key".parse().unwrap();
        assert_eq!(parse_bridge_action(&unknown, token), None);
        let bad: url::Url = "https://vetcord.bridge/abc123token/set-debloat?v=2".parse().unwrap();
        assert_eq!(parse_bridge_action(&bad, token), None);
        assert_eq!(parse_bridge_action(&ok, ""), None);
    }

    #[test]
    fn discord_urls_are_allowed_in_app() {
        for raw in [
            "https://discord.com/app",
            "https://discord.com/channels/@me",
            "https://canary.discord.com/app",
            "https://cdn.discordapp.com/avatars/x/y.png",
        ] {
            assert!(is_discord_url(&raw.parse().unwrap()), "{raw}");
        }
    }

    #[test]
    fn unrelated_urls_are_not_navigated_in_app() {
        for raw in ["https://example.com/", "https://discord.com.evil.example/"] {
            assert!(!is_discord_url(&raw.parse().unwrap()), "{raw}");
        }
    }
}
