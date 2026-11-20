const invoke = window.__TAURI__.core.invoke;
const $ = (id) => document.getElementById(id);

// Fallback presets if the backend ever returns no destinations, so the
// X/Rumble setup rows always show.
const PRESET_DESTINATIONS = [
  { name: "X", kind: "x", server: "rtmps://rtmp-ams.pscp.tv:443/x", enabled: false },
  { name: "Rumble", kind: "rumble", server: "rtmp://rtmp.rumble.com/live", enabled: false },
];

async function fetchKeysPresent() {
  try {
    return await invoke("stream_keys_present");
  } catch {
    return [];
  }
}

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
  // Render streaming setup before other refreshes: a failing window list
  // or key store must not leave the X/Rumble rows blank.
  $("stream_bitrate").value = s.stream_bitrate_kbps;
  renderDestinations(s.stream_destinations, await fetchKeysPresent());
  await refreshWindows(s.capture_window);
  $("github-issues-url").textContent = await invoke("github_issues_url");

  await refreshClipStatus();
  await refreshStreamStatus();
  await refreshSuggestions();
  if (location.hash) {
    document.querySelector(location.hash)?.scrollIntoView();
  }
}

function renderDestinations(destinations, keysPresent) {
  if (!destinations?.length) destinations = PRESET_DESTINATIONS;
  const container = $("stream-destinations");
  container.innerHTML = "";
  destinations.forEach((d, i) => {
    const div = document.createElement("div");
    div.className = "destination";
    div.innerHTML =
      `<label><input type="checkbox" id="dest-enabled-${i}"> <strong></strong></label>` +
      `<label>Server <input type="text" id="dest-server-${i}" size="40"></label>` +
      `<label>Stream key <input type="password" id="dest-key-${i}" size="30" ` +
      `placeholder=""> <button type="button" id="dest-savekey-${i}">Save key</button></label>`;
    div.querySelector("strong").textContent = d.name;
    container.appendChild(div);
    $(`dest-enabled-${i}`).checked = d.enabled;
    $(`dest-server-${i}`).value = d.server;
    $(`dest-key-${i}`).placeholder = keysPresent.includes(d.name)
      ? "key saved (enter to replace)"
      : "no key saved";
    $(`dest-savekey-${i}`).addEventListener("click", async () => {
      const status = $("save-status");
      try {
        await invoke("set_stream_key", {
          destination: d.name,
          key: $(`dest-key-${i}`).value,
        });
        $(`dest-key-${i}`).value = "";
        status.textContent = `Stream key for ${d.name} saved (encrypted).`;
        const s = await invoke("get_settings");
        renderDestinations(s.stream_destinations, await fetchKeysPresent());
      } catch (e) {
        status.textContent = `Key save failed: ${e}`;
      }
    });
  });
}

function readDestinations(existing) {
  if (!existing?.length) existing = PRESET_DESTINATIONS;
  return existing.map((d, i) => ({
    name: d.name,
    kind: d.kind,
    server: $(`dest-server-${i}`)?.value.trim() ?? d.server,
    enabled: $(`dest-enabled-${i}`)?.checked ?? d.enabled,
  }));
}

async function refreshStreamStatus() {
  const st = await invoke("get_stream_status");
  $("stream-status").textContent =
    `${st.message} · ${st.bitrate_kbps} kbps` +
    (st.destinations.length ? ` · ${st.destinations.join("  ")}` : " · no destinations enabled");
  $("toggle-stream").textContent = st.streaming ? "Stop streaming" : "Go live";
  return st;
}

$("toggle-stream").addEventListener("click", async () => {
  const status = $("save-status");
  try {
    const st = await refreshStreamStatus();
    if (st.streaming) {
      await invoke("stop_stream");
      status.textContent = "Stream stopped.";
    } else {
      await invoke("start_stream");
      status.textContent = "Streaming started.";
    }
  } catch (e) {
    status.textContent = `Stream error: ${e}`;
  }
  await refreshStreamStatus();
});

async function refreshSuggestions() {
  const items = await invoke("list_suggestions");
  const list = $("suggestions-list");
  list.innerHTML = "";
  for (const item of items.slice(-10).reverse()) {
    const li = document.createElement("li");
    li.textContent = item.details ? `${item.title} — ${item.details}` : item.title;
    list.appendChild(li);
  }
}

$("submit-suggestion").addEventListener("click", async () => {
  const status = $("suggest-status");
  try {
    await invoke("submit_suggestion", {
      title: $("suggest-title").value,
      details: $("suggest-details").value,
    });
    $("suggest-title").value = "";
    $("suggest-details").value = "";
    status.textContent = "Suggestion saved locally. Thank you!";
  } catch (e) {
    status.textContent = `Could not save suggestion: ${e}`;
  }
  await refreshSuggestions();
});

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
    const current = await invoke("get_settings");
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
        stream_bitrate_kbps: Number($("stream_bitrate").value),
        stream_destinations: readDestinations(current.stream_destinations),
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
