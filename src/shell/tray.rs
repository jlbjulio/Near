use crate::{
    model::ViewModel,
    ui::paint::{Canvas, font},
};
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};
pub struct Tray {
    data: NOTIFYICONDATAW,
    icon: HICON,
    stamp: String,
    tip: String,
}
impl Tray {
    pub unsafe fn new(hwnd: HWND) -> windows::core::Result<Self> {
        let icon = LoadIconW(None, IDI_APPLICATION)?;
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: crate::shell::TRAY,
            hIcon: icon,
            ..Default::default()
        };
        let mut tray = Self {
            data,
            icon: HICON::default(),
            stamp: String::new(),
            tip: String::new(),
        };
        tray.set_tip("Near · AirPods for Windows");
        tray.register();
        Ok(tray)
    }
    pub unsafe fn register(&mut self) {
        let _ = Shell_NotifyIconW(NIM_ADD, &self.data);
        self.data.Anonymous.uVersion = 4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &self.data);
    }
    fn set_tip(&mut self, tip: &str) {
        self.data.szTip.fill(0);
        for (i, c) in tip.encode_utf16().take(127).enumerate() {
            self.data.szTip[i] = c;
        }
    }
    pub unsafe fn update(&mut self, vm: &ViewModel) {
        let level = vm.batteries.tray_level();
        let charging = vm.batteries.charging();
        let stamp = format!("{level:?}:{charging}");
        let tip = format!(
            "Near · {}\nLeft {} · Right {} · Case {}",
            vm.name,
            vm.batteries.value(0),
            vm.batteries.value(1),
            vm.batteries.value(2)
        );
        if stamp == self.stamp && tip == self.tip {
            return;
        }
        if stamp != self.stamp {
            if let Ok(icon) = icon(level, charging) {
                let previous = self.icon;
                self.icon = icon;
                self.data.hIcon = icon;
                if !previous.is_invalid() {
                    let _ = DestroyIcon(previous);
                }
            }
            self.stamp = stamp;
        }
        self.set_tip(&tip);
        self.tip = tip;
        let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data);
    }
    pub unsafe fn alert(&mut self, message: &str) {
        self.data.szInfo.fill(0);
        self.data.szInfoTitle.fill(0);
        for (i, c) in message.encode_utf16().take(255).enumerate() {
            self.data.szInfo[i] = c;
        }
        for (i, c) in "Low battery".encode_utf16().enumerate() {
            self.data.szInfoTitle[i] = c;
        }
        self.data.dwInfoFlags = NIIF_WARNING;
        self.data.uFlags |= NIF_INFO;
        let _ = Shell_NotifyIconW(NIM_MODIFY, &self.data);
        self.data.uFlags &= !NIF_INFO;
    }
    pub unsafe fn anchor(&self) -> Option<RECT> {
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.data.hWnd,
            uID: 1,
            ..Default::default()
        };
        Shell_NotifyIconGetRect(&identifier).ok()
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}
unsafe fn icon(level: Option<u8>, charging: bool) -> windows::core::Result<HICON> {
    let screen = GetDC(None);
    let dc = CreateCompatibleDC(Some(screen));
    let mut bits = std::ptr::null_mut();
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 32,
            biHeight: -32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let bitmap = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
    let old = SelectObject(dc, HGDIOBJ(bitmap.0));
    std::ptr::write_bytes(bits, 0, 32 * 32 * 4);
    {
        let c = Canvas::new(dc, 1.);
        if let Some(level) = level {
            c.round(1., 1., 30., 30., 8., crate::ui::paint::ACCENT);
            let f = font(1., if level == 100 { 16 } else { 21 }, true);
            c.text(&level.to_string(), 0., 0., 32., 30., f, 0xffffffff, true);
            let _ = DeleteObject(HGDIOBJ(f.0));
        } else {
            c.logo(1., 1., 30.);
        }
        if charging {
            c.ellipse(24., 1., 7., 7., 0xffffffff);
        }
    }
    // GDI text does not write alpha; fill alpha only inside the opaque rounded surface.
    let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), 1024);
    for y in 0..32 {
        for x in 0..32 {
            if (2..30).contains(&x) && (2..30).contains(&y) {
                pixels[y * 32 + x] |= 0xff000000;
            }
        }
    }
    let mask = CreateBitmap(32, 32, 1, 1, None);
    let result = CreateIconIndirect(&ICONINFO {
        fIcon: true.into(),
        hbmMask: mask,
        hbmColor: bitmap,
        ..Default::default()
    });
    SelectObject(dc, old);
    let _ = DeleteObject(HGDIOBJ(mask.0));
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(dc);
    let _ = ReleaseDC(None, screen);
    result
}
