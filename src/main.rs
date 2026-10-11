#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unsafe_op_in_unsafe_fn)]
mod alerts;
mod audio;
mod backend;
mod bluetooth;
mod connection;
mod model;
mod shell;
mod ui;
use windows::{
    Win32::{
        Foundation::*,
        System::Threading::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::w,
};
fn main() {
    std::panic::set_hook(Box::new(|e| shell::log(&e.to_string())));
    if let Err(e) = run() {
        shell::log(&e.to_string());
        unsafe {
            MessageBoxW(
                None,
                &windows::core::HSTRING::from(e.to_string()),
                w!("Near"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}
fn run() -> windows::core::Result<()> {
    unsafe {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args == ["--startup-enable"] {
            return shell::startup::set(true);
        }
        if args == ["--startup-disable"] {
            return shell::startup::set(false);
        }
        let preview = args.first().is_some_and(|s| s == "--preview");
        let existing = FindWindowW(w!("Near.Native.Flyout"), None).ok();
        if !preview {
            if let Some(window) = existing {
                let cmd = if args == ["--quit"] {
                    3
                } else if args == ["--flyout"] {
                    2
                } else {
                    1
                };
                let _ = PostMessageW(Some(window), shell::IPC, WPARAM(cmd), LPARAM(0));
                return Ok(());
            }
            if args == ["--quit"] {
                return Ok(());
            }
        }
        let user = std::env::var("USERPROFILE").unwrap_or_default();
        let hash = user.bytes().fold(14695981039346656037u64, |h, b| {
            (h ^ b as u64).wrapping_mul(1099511628211)
        });
        let name = shell::wide(&format!("Local\\Near.Rust.{hash:x}"));
        let mutex = CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr()))?;
        if !preview && GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(mutex);
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let result = ui::run(
            args.is_empty() || args.iter().any(|s| s == "--open") || preview,
            preview,
            args.get(1).filter(|arg| !arg.starts_with("--")).cloned(),
            args.iter()
                .find_map(|arg| arg.strip_prefix("--preview-state="))
                .map(String::from),
        );
        let _ = CloseHandle(mutex);
        result
    }
}
