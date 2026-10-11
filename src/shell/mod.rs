pub mod startup;
pub mod tray;
use std::path::Path;
use windows::{
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};
pub const UPDATE: u32 = WM_APP + 1;
pub const IPC: u32 = WM_APP + 2;
pub const TRAY: u32 = WM_APP + 3;
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
pub struct Apartment;
impl Apartment {
    pub fn mta() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        Self
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
pub fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let a = wide(&from.to_string_lossy());
    let b = wide(&to.to_string_lossy());
    unsafe {
        MoveFileExW(
            PCWSTR(a.as_ptr()),
            PCWSTR(b.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))
    }
}
pub fn notify(window: usize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(window as *mut _)), UPDATE, WPARAM(0), LPARAM(0));
    }
}
pub fn open_bluetooth() {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:bluetooth"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}
pub fn log(text: &str) {
    let root = crate::model::data_dir();
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::write(
        root.join("last-error.log"),
        format!("{}\n{}", crate::model::now(), text),
    );
}
