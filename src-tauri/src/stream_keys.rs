//! Stream keys, encrypted at rest with Windows DPAPI (per-user).
//!
//! Keys never appear in settings.json, status output, or logs. They are
//! stored as base64 DPAPI ciphertext in `stream_keys.json` in the app
//! config directory, keyed by destination name.

use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;
use std::path::PathBuf;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
};

fn blob_from(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    }
}

unsafe fn take_blob(blob: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let data =
        unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
    let _ = unsafe { LocalFree(Some(HLOCAL(blob.pbData as *mut std::ffi::c_void))) };
    data
}

pub fn protect(plaintext: &str) -> Result<String, String> {
    let input = blob_from(plaintext.as_bytes());
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output,
        )
        .map_err(|e| format!("DPAPI protect failed: {e}"))?;
        Ok(STANDARD.encode(take_blob(&output)))
    }
}

pub fn unprotect(encoded: &str) -> Result<String, String> {
    let ciphertext = STANDARD
        .decode(encoded)
        .map_err(|e| format!("stored key is not valid base64: {e}"))?;
    let input = blob_from(&ciphertext);
    let mut output = CRYPT_INTEGER_BLOB::default();
    let plaintext = unsafe {
        CryptUnprotectData(
            &input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output,
        )
        .map_err(|e| format!("DPAPI unprotect failed: {e}"))?;
        take_blob(&output)
    };
    String::from_utf8(plaintext).map_err(|e| format!("stored key is not UTF-8: {e}"))
}

pub struct KeyStore {
    path: PathBuf,
}

impl KeyStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { path: config_dir.join("stream_keys.json") }
    }

    fn load_map(&self) -> HashMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save_map(&self, map: &HashMap<String, String>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, text).map_err(|e| e.to_string())
    }

    pub fn set(&self, destination: &str, key: &str) -> Result<(), String> {
        let mut map = self.load_map();
        map.insert(destination.to_string(), protect(key)?);
        self.save_map(&map)
    }

    pub fn get(&self, destination: &str) -> Option<String> {
        self.load_map()
            .get(destination)
            .and_then(|encoded| unprotect(encoded).ok())
    }

    pub fn clear(&self, destination: &str) -> Result<(), String> {
        let mut map = self.load_map();
        map.remove(destination);
        self.save_map(&map)
    }

    pub fn has(&self, destination: &str) -> bool {
        self.load_map().contains_key(destination)
    }
}

/// Join an ingest server URL and a stream key into one RTMP URL.
pub fn full_rtmp_url(server: &str, key: &str) -> String {
    if key.is_empty() {
        return server.to_string();
    }
    format!("{}/{}", server.trim_end_matches('/'), key)
}

/// Mask an RTMP URL for display so the stream key is never shown.
pub fn mask_url(url: &str) -> String {
    match url.rfind('/') {
        Some(pos) if pos > "rtmp://".len() => format!("{}********", &url[..=pos]),
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trips() {
        let encrypted = protect("test-stream-key-123").unwrap();
        assert_ne!(encrypted, "test-stream-key-123");
        assert_eq!(unprotect(&encrypted).unwrap(), "test-stream-key-123");
    }

    #[test]
    fn key_store_set_get_clear() {
        let dir = std::env::temp_dir().join("vetcord-test-keys");
        let store = KeyStore::new(dir.clone());
        assert!(!store.has("X"));
        store.set("X", "secret").unwrap();
        assert!(store.has("X"));
        assert_eq!(store.get("X").as_deref(), Some("secret"));
        // The on-disk file must not contain the plaintext key.
        let raw = std::fs::read_to_string(dir.join("stream_keys.json")).unwrap();
        assert!(!raw.contains("secret"));
        store.clear("X").unwrap();
        assert!(!store.has("X"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn url_join_and_mask_hide_the_key() {
        let url = full_rtmp_url("rtmp://rtmp.rumble.com/live/", "abc123");
        assert_eq!(url, "rtmp://rtmp.rumble.com/live/abc123");
        let masked = mask_url(&url);
        assert!(!masked.contains("abc123"), "masked: {masked}");
        assert!(masked.contains("rtmp.rumble.com"));
    }
}
