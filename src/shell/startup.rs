use crate::shell::wide;
use windows::{Win32::System::Registry::*, core::w};
fn command() -> String {
    format!(
        "\"{}\" --background",
        std::env::current_exe().unwrap_or_default().display()
    )
}
pub fn enabled() -> bool {
    unsafe {
        let mut buffer = [0u16; 2048];
        let mut size = (buffer.len() * 2) as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            w!("Near"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .is_err()
        {
            return false;
        }
        String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len())],
        ) == command()
    }
}
pub fn set(enabled: bool) -> windows::core::Result<()> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let result = if enabled {
            let text = wide(&command());
            let bytes = std::slice::from_raw_parts(text.as_ptr().cast::<u8>(), text.len() * 2);
            RegSetValueExW(key, w!("Near"), None, REG_SZ, Some(bytes)).ok()
        } else {
            let code = RegDeleteValueW(key, w!("Near"));
            if code == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                code.ok()
            }
        };
        let _ = RegDeleteValueW(key, w!("NearAppleInterop"));
        let _ = RegCloseKey(key);
        result
    }
}
