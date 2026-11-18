#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clip;
mod debloat;
mod settings;
mod window_list;

use settings::Settings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

const DISCORD_APP_URL: &str = "https://discord.com/app";
// A current desktop Chrome UA, as Vesktop uses, so Discord serves its
// full desktop web client to WebView2 instead of a degraded page.
const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

struct AppState {
    settings_path: PathBuf,
    popups: AtomicU32,
    recorder: std::sync::Mutex<Option<clip::Recorder>>,
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

#[tauri::command]
fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html".into()))
        .title("VetCord Settings")
        .inner_size(560.0, 640.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
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

fn open_main_window(app: &tauri::AppHandle, settings: &Settings) -> tauri::Result<()> {
    let handle = app.clone();
    let script = debloat::initialization_script(settings.debloat);
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
    .on_navigation(|url| is_discord_url(url))
    .on_new_window(move |url, _features| {
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
    .build()?;
    Ok(())
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "VetCord Settings", true, None::<&str>)?;
    let clip_item = MenuItem::with_id(app, "save_clip", "Save Clip", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit VetCord", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &clip_item, &quit_item])?;
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
            app.manage(AppState {
                settings_path: path,
                popups: AtomicU32::new(0),
                recorder: std::sync::Mutex::new(None),
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
            open_settings
        ])
        .build(tauri::generate_context!())
        .expect("error while building VetCord")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Never leave ffmpeg capturing after VetCord closes.
                let state = app.state::<AppState>();
                let mut guard = state.recorder.lock().unwrap();
                if let Some(mut recorder) = guard.take() {
                    clip::stop_recorder(&mut recorder);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

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
