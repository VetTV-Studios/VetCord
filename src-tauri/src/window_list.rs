//! Lists visible top-level windows so the user can pick one (e.g. a game)
//! to capture instead of the whole desktop.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
};

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let titles = &mut *(lparam.0 as *mut Vec<String>);
    if IsWindowVisible(hwnd).as_bool() {
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buf = vec![0u16; len as usize + 1];
            let read = GetWindowTextW(hwnd, &mut buf);
            if read > 0 {
                let title = String::from_utf16_lossy(&buf[..read as usize]);
                let title = title.trim().to_string();
                if !title.is_empty() && !titles.contains(&title) {
                    titles.push(title);
                }
            }
        }
    }
    BOOL::from(true)
}

/// Titles of visible windows, sorted for a stable picker UI.
pub fn list_window_titles() -> Vec<String> {
    let mut titles: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect),
            LPARAM(&mut titles as *mut Vec<String> as isize),
        );
    }
    titles.sort();
    titles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_sorted_and_non_empty() {
        // The test host always has some shell windows; just check the
        // contract (sorted, no blanks) rather than specific titles.
        let titles = list_window_titles();
        let mut sorted = titles.clone();
        sorted.sort();
        assert_eq!(titles, sorted);
        assert!(titles.iter().all(|t| !t.trim().is_empty()));
    }
}
