const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);

async function load() {
  const s = await invoke("get_settings");
  $("debloat").checked = s.debloat;
  $("start_minimised").checked = s.start_minimised;
  $("clipping_enabled").checked = s.clipping_enabled;
  $("clip_seconds").value = s.clip_seconds;
  $("clip_fps").value = s.clip_fps;
  $("clip_backend").value = s.clip_backend;
  $("hotkey_enabled").checked = s.hotkey_enabled;
  $("hotkey").value = s.hotkey;
  $("capture_target").value = s.capture_target;
  await refreshWindows(s.capture_window);

  await refreshClipStatus();
}

async function refreshClipStatus() {
  const c = await invoke("get_clip_status");
  $("clip-status").textContent =
    `${c.message} · Backend: ${c.backend} · Buffered: ${c.buffered_seconds}s / ${c.buffer_seconds}s · ` +
    `Output: ${c.output_dir} · ffmpeg: ${c.ffmpeg_available ? "found" : "not found"}` +
    (c.last_clip ? ` · Last clip: ${c.last_clip}` : "");
  $("toggle-clip").textContent = c.capturing ? "Stop clipping" : "Start clipping";
  return c;
}

async function refreshWindows(selected) {
  const titles = await invoke("list_windows");
  const select = $("capture_window");
  select.innerHTML = "";
  for (const title of titles) {
    const opt = document.createElement("option");
    opt.value = title;
    opt.textContent = title;
    select.appendChild(opt);
  }
  if (selected && titles.includes(selected)) select.value = selected;
}

$("refresh-windows").addEventListener("click", async () => {
  await refreshWindows($("capture_window").value);
});

$("toggle-clip").addEventListener("click", async () => {
  const status = $("save-status");
  try {
    const c = await refreshClipStatus();
    if (c.capturing) {
      await invoke("stop_clipping");
      status.textContent = "Clipping stopped.";
    } else {
      await invoke("start_clipping");
      status.textContent = "Clipping started - replay buffer is recording.";
    }
  } catch (e) {
    status.textContent = `Clipping error: ${e}`;
  }
  await refreshClipStatus();
});

$("save").addEventListener("click", async () => {
  const status = $("save-status");
  try {
    await invoke("save_settings", {
      settings: {
        debloat: $("debloat").checked,
        start_minimised: $("start_minimised").checked,
        clipping_enabled: $("clipping_enabled").checked,
        clip_seconds: Number($("clip_seconds").value),
        clip_fps: Number($("clip_fps").value),
        clip_backend: $("clip_backend").value,
        hotkey_enabled: $("hotkey_enabled").checked,
        hotkey: $("hotkey").value.trim(),
        capture_target: $("capture_target").value,
        capture_window: $("capture_window").value,
      },
    });
    status.textContent = "Saved.";
  } catch (e) {
    // A failed save must be visible, never shown as success.
    status.textContent = `Save failed: ${e}`;
  }
});

$("save-clip").addEventListener("click", async () => {
  const status = $("save-status");
  try {
    status.textContent = await invoke("save_clip");
  } catch (e) {
    status.textContent = `Clip not saved: ${e}`;
  }
});

load().catch((e) => { $("save-status").textContent = `Load failed: ${e}`; });
