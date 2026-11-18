# VetCord

A lightweight, open-source Discord client for Windows, written in Rust.

VetCord works like [Vesktop](https://github.com/Vencord/Vesktop): it loads
Discord's official web client (`https://discord.com/app`) inside a native
WebView2 window provided by [Tauri](https://tauri.app/). You sign in on
Discord's own login page, with your official account, and get your real
servers, DMs, communities, voice and screen share — because it *is*
Discord's web client, minus the Electron shell around it.

VetCord never sees, stores, or transmits your password or token. Login
cookies live in WebView2's own profile storage, exactly as in a browser.

## Why

- **Less bloat:** Tauri uses the WebView2 runtime already on Windows
  instead of bundling a full Chromium + Electron shell, and VetCord hides
  Nitro upsells, gift and shop buttons via an injected stylesheet
  (toggleable in Settings).
- **More privacy:** no VetCord telemetry, analytics, or accounts. A tray
  menu opens Settings; navigation is restricted to Discord domains.
- **Less AI:** no added AI features; promotional surfaces are hidden.
- **Clipping:** a rolling replay buffer (ffmpeg, 5-second segments,
  bounded at your chosen 15–600 s) records the desktop while enabled.
  Backend is selectable: Auto tries GPU capture first (Desktop
  Duplication API `ddagrab` + NVENC) and falls back to CPU
  (`gdigrab` + x264) if the GPU path cannot start; Settings shows the
  backend actually in use. **Save Clip** from Settings, the tray, or the
  global hotkey (default `Ctrl+Alt+C`, configurable, works while a game
  is focused) writes the last N seconds to
  `%USERPROFILE%\Videos\VetCord\Clips`. Requires ffmpeg:
  `winget install -e --id Gyan.FFmpeg`. While capturing, your whole
  desktop is recorded locally — nothing is uploaded. Capture target
  can also be a single window picked from a list of open windows
  (e.g. your game), recorded with `gdigrab` title capture.

## Install

Download/run the NSIS installer from
`src-tauri\target\release\bundle\nsis\VetCord_0.2.0_x64-setup.exe`
(per-user install, no admin needed), or build it yourself with
`npx @tauri-apps/cli@2 build` from `src-tauri\`.

## Build & run

Prerequisites: Rust (via [rustup](https://rustup.rs/)), the VS 2022 C++
Build Tools, and WebView2 (preinstalled on current Windows 10/11).

```powershell
cd src-tauri
cargo run            # dev build, opens VetCord on Discord's login/app page
cargo test           # Rust unit tests
cargo build --release  # optimised binary in target\release\vetcord.exe
```

Sign in in the main window. Open **VetCord Settings** from the system
tray icon to toggle debloat/clipping options.

## Honest limitations

- Third-party Discord clients are against Discord's Terms of Service;
  use at your own risk. Vesktop-style web wrappers are the lowest-risk
  variant (no user-token automation, no self-botting), but VetCord cannot
  guarantee an account will never be flagged.
- Debloat CSS selectors target Discord's current class names and may
  need updating when Discord restyles its client.
- Privacy gains are local: Discord's servers still process your messages
  as with any official client, so this is not end-to-end encryption.

## License

MIT — see [LICENSE](LICENSE).
