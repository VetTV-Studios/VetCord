/// Script injected into Discord's web client at document start.
///
/// Vesktop-style: this only restyles/hides promotional UI in the page the
/// user already loaded. It does not read, store, or forward any credential,
/// token, or message content.
pub const DEBLOAT_CSS: &str = r#"
/* VetCord debloat: hide Nitro upsells, gifting and shop prompts. */
a[href="/store"], a[href="/shop"],
button[aria-label="Send a gift"],
button[aria-label="Open gift inventory"],
[class*="premiumPromo"], [class*="nitroUpsell"],
[class*="upsellContainer"], [class*="giftButton"] {
  display: none !important;
}
"#;

pub fn initialization_script(debloat_enabled: bool) -> String {
    if !debloat_enabled {
        return "window.__VETCORD__ = { debloat: false };".to_string();
    }
    format!(
        r#"
window.__VETCORD__ = {{ debloat: true, version: "{}" }};
(() => {{
  const css = `{}`;
  const apply = () => {{
    if (document.getElementById("vetcord-debloat")) return;
    const style = document.createElement("style");
    style.id = "vetcord-debloat";
    style.textContent = css;
    (document.head || document.documentElement).appendChild(style);
  }};
  apply();
  new MutationObserver(apply).observe(document.documentElement,
    {{ childList: true, subtree: true }});
}})();
"#,
        env!("CARGO_PKG_VERSION"),
        DEBLOAT_CSS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_debloat_injects_no_css() {
        let script = initialization_script(false);
        assert!(script.contains("debloat: false"));
        assert!(!script.contains("premiumPromo"));
    }

    #[test]
    fn enabled_debloat_hides_promotional_ui() {
        let script = initialization_script(true);
        assert!(script.contains("vetcord-debloat"));
        assert!(script.contains("/store"));
    }
}
