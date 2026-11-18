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
  desktop is recorded locally — nothing is uploaded. Capture targets:
  whole desktop, a single window picked from a list of open windows
  (`gdigrab` title capture), or **Fullscreen Display** — GPU/Desktop
  Duplication API capture of the display, which is the path that sees
  exclusive-fullscreen games (`gdigrab` records black frames from
  those, so VetCord deliberately does not fall back to it there).
- **Go Live in Discord:** Discord has no RTMP/stream-key ingest for
  user accounts, so screen-sharing in a server uses Discord's own
  **Share Your Screen / Go Live** button in a voice channel — it works
  in VetCord exactly as in the official client. Settings includes a
  short guide, including creating a community server via Discord's
  **+ → Create My Own** flow.
- **Live multistreaming:** one ffmpeg process pushes your capture to
  X, Rumble, and/or any custom RTMP server simultaneously (tee muxer,
  `onfail=ignore` per destination). Stream keys are stored encrypted
  with Windows DPAPI, never in `settings.json`, and shown only masked.
- **Suggest a feature:** Settings has a suggestion form saved locally
  (`suggestions.jsonl` in the app data folder — VetCord has no backend
  server) plus a GitHub Issues link for public tracking.

## Install

Download/run the NSIS installer from
`src-tauri\target\release\bundle\nsis\VetCord_0.3.0_x64-setup.exe`
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

## Streaming: testing it yourself

Live ingestion against X/Rumble needs your own account keys (X Media
Studio / Rumble Studio). In Settings: enable a destination, paste its
stream key, **Save key**, then **Save settings**, pick a capture
target, and press **Go live**. The ffmpeg command construction and
the tee/`onfail=ignore` behaviour are covered by unit tests and were
exercised locally, but end-to-end ingestion against real X/Rumble
accounts has not been tested by the author.

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
