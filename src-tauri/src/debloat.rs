use crate::settings::Settings;

/// Script injected into Discord's web client at document start.
///
/// Vesktop-style: this only restyles/hides promotional UI in the page the
/// user already loaded, and adds a "Vetcord" entry to Discord's User
/// Settings sidebar (gear/pinwheel) opening a VetCord panel: Discord
/// tweaks (CSS-only, BetterDiscord-style), app shortcuts, and live
/// streaming controls for X/Rumble. Panel actions reach Rust only via
/// per-launch-token bridge URLs (see `parse_bridge_action` in main.rs);
/// the script never reads, stores, or forwards any credential, token,
/// message content, or stream key. Stream keys are entered only in the
/// native VetCord Settings window.
pub fn initialization_script(settings: &Settings, bridge_token: &str) -> String {
    // Settings contains no secrets (stream keys live in the encrypted
    // key file), so a snapshot is safe to embed for the panel.
    let snapshot = serde_json::to_string(settings).unwrap_or_else(|_| "{}".to_string());
    format!(
        "window.__VETCORD__ = {{ debloat: {}, version: \"{}\", bridgeToken: \"{}\", settings: {} }};\n{}",
        settings.debloat,
        env!("CARGO_PKG_VERSION"),
        bridge_token,
        snapshot,
        PANEL_SCRIPT
    )
}

const PANEL_SCRIPT: &str = r##"
(() => {
  const VC = window.__VETCORD__ || {};
  const TOKEN = VC.bridgeToken || "";
  const SNAP = VC.settings || {};
  const TWEAKS = {
    hideUpsells: {
      label: "Hide Nitro & shop upsells",
      css: 'a[href="/store"], a[href="/shop"], [class*="premiumPromo"], [class*="nitroUpsell"], [class*="upsellContainer"] { display: none !important; }'
    },
    hideGift: {
      label: "Hide gift buttons",
      css: 'button[aria-label="Send a gift"], button[aria-label="Open gift inventory"], [class*="giftButton"] { display: none !important; }'
    },
    hideApps: {
      label: "Hide Apps & sticker buttons",
      css: 'button[aria-label="Open Apps"], button[aria-label="Open sticker picker"] { display: none !important; }'
    },
    hideActivity: {
      label: "Hide Active Now column (Friends page)",
      css: '[class*="nowPlayingColumn"] { display: none !important; }'
    }
  };
  const tweakDefaults = () => ({
    hideUpsells: !!VC.debloat,
    hideGift: !!VC.debloat,
    hideApps: false,
    hideActivity: false
  });
  const loadTweaks = () => {
    try {
      const saved = JSON.parse(localStorage.getItem("vetcord.tweaks.v1") || "null");
      return Object.assign(tweakDefaults(), saved || {});
    } catch { return tweakDefaults(); }
  };
  let tweaks = loadTweaks();
  const saveTweaks = () => {
    try { localStorage.setItem("vetcord.tweaks.v1", JSON.stringify(tweaks)); } catch {}
  };
  const applyTweaks = () => {
    let style = document.getElementById("vetcord-tweaks");
    if (!style) {
      style = document.createElement("style");
      style.id = "vetcord-tweaks";
      (document.head || document.documentElement).appendChild(style);
    }
    style.textContent =
      "#vetcord-settings-item{cursor:pointer} " +
      ".vetcord-custom-row{display:flex;align-items:center;gap:8px;margin:6px 8px;padding:8px 10px;border-radius:6px;background:#5865f2;color:#fff;font-weight:600;font-size:14px;cursor:pointer}\n" +
      Object.entries(TWEAKS)
        .filter(([k]) => tweaks[k])
        .map(([, v]) => v.css)
        .join("\n");
  };
  const bridge = (action, query) => {
    if (!TOKEN) return;
    const a = document.createElement("a");
    a.href = "https://vetcord.bridge/" + TOKEN + "/" + action + (query || "");
    a.target = "_blank";
    a.rel = "noreferrer";
    document.body.appendChild(a);
    a.click();
    a.remove();
  };
  const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

  const closePanel = () => document.getElementById("vetcord-panel-overlay")?.remove();
  const btnStyle = "background:#5865f2;color:#fff;border:0;border-radius:6px;padding:8px 14px;cursor:pointer;margin:4px 6px 4px 0";
  const openPanel = () => {
    closePanel();
    const dests = Array.isArray(SNAP.stream_destinations) ? SNAP.stream_destinations : [];
    const overlay = document.createElement("div");
    overlay.id = "vetcord-panel-overlay";
    overlay.style.cssText = "position:fixed;inset:0;background:rgba(0,0,0,0.65);z-index:100000;display:flex;align-items:center;justify-content:center";
    overlay.innerHTML =
      '<div style="width:min(640px,92vw);max-height:88vh;overflow:auto;background:#2b2d31;color:#e3e5e8;border-radius:8px;padding:20px 22px;font-family:inherit">' +
      '<div style="display:flex;justify-content:space-between;align-items:center">' +
      "<h2 style=\"margin:0\">Vetcord</h2>" +
      '<button type="button" id="vc-close" style="' + btnStyle + '">Close</button></div>' +
      '<p style="color:#949ba4;font-size:13px">VetCord-only features. Discord tweaks apply instantly ' +
      "and are saved on this PC. Version " + esc(VC.version || "") + ".</p>" +
      "<h3>Discord tweaks</h3>" +
      '<div id="vc-tweaks"></div>' +
      '<label style="display:block;margin:10px 0"><input type="checkbox" id="vc-debloat"> ' +
      "Debloat by default on launch (app setting)</label>" +
      "<h3>VetCord app</h3>" +
      '<p style="color:#949ba4;font-size:13px">Clipping: ' + esc(SNAP.clip_seconds ?? 60) + "s replay buffer, " +
      esc(SNAP.clip_fps ?? 60) + " fps, target " + esc(SNAP.capture_target || "desktop") + ". " +
      "Save-clip hotkey: " + esc(SNAP.hotkey || "Ctrl+Alt+C") + ".</p>" +
      '<button type="button" id="vc-open-settings" style="' + btnStyle + '">Open VetCord Settings</button>' +
      '<button type="button" id="vc-toggle-clip" style="' + btnStyle + '">Start / stop clipping</button>' +
      '<button type="button" id="vc-save-clip" style="' + btnStyle + '">Save clip now</button>' +
      "<h3>Live streaming to X &amp; Rumble</h3>" +
      '<p style="color:#949ba4;font-size:13px">Direct RTMP multistream, separate from Discord Go Live ' +
      "(which only streams inside Discord). Bitrate: " + esc(SNAP.stream_bitrate_kbps ?? 6000) + " kbps. " +
      "Stream keys are entered only in the native VetCord window, never in this page.</p>" +
      '<div id="vc-dests">' +
      (dests.length
        ? dests.map((d, i) =>
            '<label style="display:block;margin:8px 0"><input type="checkbox" class="vc-dest-enabled" data-i="' + i + '"' +
            (d.enabled ? " checked" : "") + "> <strong>" + esc(d.name) + "</strong> " +
            '<span style="color:#949ba4;font-size:12px">' + esc(d.server) + "</span></label>").join("")
        : '<p style="color:#949ba4;font-size:13px">No destinations yet - open streaming setup to add X and Rumble.</p>') +
      "</div>" +
      '<button type="button" id="vc-open-streaming" style="' + btnStyle + '">Open X / Rumble streaming setup</button>' +
      '<button type="button" id="vc-toggle-stream" style="' + btnStyle + '">Go Live (RTMP) / Stop</button>' +
      '<p id="vc-note" style="color:#949ba4;font-size:12px"></p>' +
      "</div>";
    overlay.addEventListener("click", (e) => { if (e.target === overlay) closePanel(); });
    document.body.appendChild(overlay);
    const note = (t) => { const n = overlay.querySelector("#vc-note"); if (n) n.textContent = t; };
    overlay.querySelector("#vc-close").addEventListener("click", closePanel);
    const tweaksBox = overlay.querySelector("#vc-tweaks");
    for (const [key, def] of Object.entries(TWEAKS)) {
      const label = document.createElement("label");
      label.style.cssText = "display:block;margin:10px 0";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = !!tweaks[key];
      box.addEventListener("change", () => { tweaks[key] = box.checked; saveTweaks(); applyTweaks(); });
      label.appendChild(box);
      label.appendChild(document.createTextNode(" " + def.label));
      tweaksBox.appendChild(label);
    }
    const debloatBox = overlay.querySelector("#vc-debloat");
    debloatBox.checked = !!SNAP.debloat;
    debloatBox.addEventListener("change", () => {
      SNAP.debloat = debloatBox.checked;
      bridge("set-debloat", "?v=" + (debloatBox.checked ? "1" : "0"));
      note("Saved. This sets the launch default; the toggles above apply right now.");
    });
    overlay.querySelectorAll(".vc-dest-enabled").forEach((box) => {
      box.addEventListener("change", () => {
        const i = box.getAttribute("data-i");
        if (SNAP.stream_destinations?.[i]) SNAP.stream_destinations[i].enabled = box.checked;
        bridge("set-stream-enabled", "?i=" + i + "&v=" + (box.checked ? "1" : "0"));
        note("Destination " + (box.checked ? "enabled" : "disabled") + ".");
      });
    });
    overlay.querySelector("#vc-open-settings").addEventListener("click", () => bridge("open-settings"));
    overlay.querySelector("#vc-toggle-clip").addEventListener("click", () => { bridge("toggle-clip"); note("Clipping toggled in VetCord."); });
    overlay.querySelector("#vc-save-clip").addEventListener("click", () => { bridge("save-clip"); note("Clip save requested."); });
    overlay.querySelector("#vc-open-streaming").addEventListener("click", () => bridge("open-streaming"));
    overlay.querySelector("#vc-toggle-stream").addEventListener("click", () => { bridge("toggle-stream"); note("Streaming toggled in VetCord. Check VetCord Settings for status."); });
  };

  // Add a "VetCord" entry to Discord's User Settings sidebar (the one
  // opened by the gear/pinwheel next to the username), and re-insert
  // after React re-renders remove it. Discord's redesigned settings
  // dropped the "My Account" label for "Account" / "Account Info", so
  // several anchors are tried, most specific first.
  const SETTINGS_MARKERS = ["Account Info", "Voice & Video", "Data & Privacy", "Notifications", "Log Out"];
  const settingsMarkerCount = (text) =>
    SETTINGS_MARKERS.filter((m) => text && text.includes(m)).length;
  const clickableRow = (el) => {
    let row = el;
    for (let i = 0; i < 6 && row; i++) {
      const role = row.getAttribute && row.getAttribute("role");
      if (role === "tab" || role === "button" || row.tagName === "BUTTON") break;
      row = row.parentElement;
    }
    return row || el.parentElement;
  };
  const wireItem = (item) => {
    item.id = "vetcord-settings-item";
    item.removeAttribute("aria-selected");
    item.style.cursor = "pointer";
    item.addEventListener("click", (ev) => {
      ev.preventDefault();
      ev.stopPropagation();
      openPanel();
    }, true);
    return item;
  };
  const cloneRowItem = (row, anchorText) => {
    const item = row.cloneNode(true);
    const leaf = [...item.querySelectorAll("*")]
      .find((e) => e.children.length === 0 && e.textContent.trim() === anchorText);
    if (leaf) {
      leaf.textContent = "VetCord";
      // Clear any other text (badges, sublabels) copied from the anchor row.
      [...item.querySelectorAll("*")]
        .filter((e) => e !== leaf && e.children.length === 0 && e.textContent.trim())
        .forEach((e) => { e.textContent = ""; });
    } else {
      item.textContent = "VetCord";
    }
    return wireItem(item);
  };
  const findLeaf = (text) =>
    [...document.querySelectorAll("div, span, a, button")]
      .find((e) => e.children.length === 0 && e.textContent.trim() === text);
  const ensureSidebarItem = () => {
    if (document.getElementById("vetcord-settings-item")) return;
    if (!document.body) return;
    const bodyText = document.body.textContent || "";
    if (settingsMarkerCount(bodyText) < 2) return; // User Settings not open
    // Strategy 1: clone an existing settings row (old and new labels).
    for (const anchor of ["My Account", "Notifications", "Nitro", "Account Info", "Voice & Video"]) {
      const el = findLeaf(anchor);
      if (!el) continue;
      const row = clickableRow(el);
      if (!row || !row.parentElement) continue;
      // The row must sit inside the settings navigation, not elsewhere.
      let container = row.parentElement;
      let inSettingsNav = settingsMarkerCount(container.textContent) >= 2;
      for (let i = 0; i < 6 && !inSettingsNav && container; i++) {
        container = container.parentElement;
        inSettingsNav = container && settingsMarkerCount(container.textContent) >= 2;
      }
      if (!inSettingsNav) continue;
      row.after(cloneRowItem(row, anchor));
      return;
    }
    // Strategy 2: custom row under the settings Search box, near the top
    // of the sidebar where it is always visible.
    const search = document.querySelector('input[placeholder*="Search" i]');
    if (search) {
      let wrap = search;
      for (let i = 0; i < 4 && wrap.parentElement; i++) wrap = wrap.parentElement;
      const item = document.createElement("div");
      item.className = "vetcord-custom-row";
      item.textContent = "VetCord";
      wrap.after(wireItem(item));
    }
  };

  document.addEventListener("keydown", (e) => { if (e.key === "Escape") closePanel(); });
  let pending = false;
  const tick = () => {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      try { applyTweaks(); ensureSidebarItem(); } catch {}
    }, 400);
  };
  new MutationObserver(tick).observe(document.documentElement, { childList: true, subtree: true });
  tick();
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn script_injects_vetcord_settings_section_with_streaming() {
        let script = initialization_script(&Settings::default(), "tok123");
        assert!(script.contains("vetcord-settings-item"), "sidebar entry");
        assert!(script.contains("My Account"), "old settings sidebar anchor");
        // Redesigned Discord settings: "Account Info" replaces
        // "My Account", and the Search box is the fallback anchor.
        assert!(script.contains("Account Info"), "new settings sidebar anchor");
        assert!(script.contains("vetcord-custom-row"), "search-box fallback row");
        assert!(script.contains("Live streaming to X"), "streaming section");
        assert!(script.contains("vetcord.bridge/tok123") || script.contains("tok123"));
        // The settings snapshot lets the panel list X/Rumble destinations.
        assert!(script.contains("stream_destinations"));
        assert!(script.contains("rtmp.rumble.com"));
        // No stream-key bridge action exists: keys stay in the native UI.
        assert!(!script.contains("set-stream-key"));
    }

    #[test]
    fn disabled_debloat_is_the_tweak_default() {
        let settings = Settings {
            debloat: false,
            ..Settings::default()
        };
        let script = initialization_script(&settings, "tok123");
        assert!(script.contains("debloat: false"));
        // Tweak definitions are always present; defaults follow debloat.
        assert!(script.contains("hideUpsells: !!VC.debloat"));
    }

    #[test]
    fn enabled_debloat_hides_promotional_ui() {
        let script = initialization_script(&Settings::default(), "tok123");
        assert!(script.contains("debloat: true"));
        assert!(script.contains("/store"));
        assert!(script.contains("vetcord-tweaks"));
    }
}

