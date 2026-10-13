use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Gdi::*, GdiPlus::*},
    },
    core::{PCWSTR, w},
};
/// Brand green for the logo and tray badge, which keep one color in both themes.
pub const BRAND: u32 = 0xff236759;
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: u32,
    pub card: u32,
    pub ink: u32,
    pub muted: u32,
    /// Disabled icons; WCAG exempts inactive controls from text contrast.
    pub faint: u32,
    /// Filled buttons and enabled toggles, under white text.
    pub accent: u32,
    pub pressed: u32,
    /// Accent text and battery bars on the page background.
    pub highlight: u32,
    pub line: u32,
    pub off: u32,
    /// Low battery: the one warm color, reserved for levels at or below 20%.
    pub warn: u32,
}
const LIGHT: Palette = Palette {
    bg: 0xfff7f8f6,
    card: 0xffffffff,
    ink: 0xff182c29,
    muted: 0xff566862,
    faint: 0xffa9b5af,
    accent: BRAND,
    pressed: 0xff194b40,
    highlight: BRAND,
    line: 0xffdee5e0,
    off: 0xff788780,
    warn: 0xffa8500e,
};
const DARK: Palette = Palette {
    bg: 0xff1b211f,
    card: 0xff252d2a,
    ink: 0xffe7eeea,
    muted: 0xffa1b0aa,
    faint: 0xff5f6d68,
    accent: 0xff2e7766,
    pressed: 0xff245e51,
    highlight: 0xff6cc0a7,
    line: 0xff343e3a,
    off: 0xff6b7a75,
    warn: 0xfff2a65a,
};
static DARK_THEME: AtomicBool = AtomicBool::new(false);
// Theme brushes live for the whole process; window messages read them without borrowing App.
static BG_BRUSH: AtomicIsize = AtomicIsize::new(0);
static CARD_BRUSH: AtomicIsize = AtomicIsize::new(0);
pub fn palette() -> &'static Palette {
    if dark() { &DARK } else { &LIGHT }
}
/// Page background, also used to erase owner-drawn buttons instead of the system's white.
pub fn bg_brush() -> HBRUSH {
    HBRUSH(BG_BRUSH.load(Ordering::Relaxed) as *mut _)
}
/// Dropdown list background on the dark theme.
pub fn card_brush() -> HBRUSH {
    HBRUSH(CARD_BRUSH.load(Ordering::Relaxed) as *mut _)
}
/// Softens the white headphone artwork on the dark theme so the battery numbers stay the focus.
fn tone(color: u32) -> u32 {
    if !dark() {
        return color;
    }
    let channel = |shift: u32| ((((color >> shift) & 0xff) * 4 / 5) & 0xff) << shift;
    0xff000000 | channel(16) | channel(8) | channel(0)
}
pub fn set_dark(dark: bool) {
    DARK_THEME.store(dark, Ordering::Relaxed);
    let p = palette();
    for (slot, color) in [(&BG_BRUSH, p.bg), (&CARD_BRUSH, p.card)] {
        let brush = unsafe { CreateSolidBrush(rgb(color)) };
        let old = slot.swap(brush.0 as isize, Ordering::Relaxed);
        if old != 0 {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(old as *mut _));
            }
        }
    }
}
pub fn dark() -> bool {
    DARK_THEME.load(Ordering::Relaxed)
}
/// Follows Settings > Personalization > Colors > "Choose your app mode".
pub fn system_dark() -> bool {
    use windows::Win32::System::Registry::*;
    let mut value = 1u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok()
            && value == 0
    }
}
pub struct Canvas {
    g: *mut GpGraphics,
    pub scale: f32,
}
impl Canvas {
    pub unsafe fn new(dc: HDC, scale: f32) -> Self {
        let mut g = std::ptr::null_mut();
        GdipCreateFromHDC(dc, &mut g);
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        Self { g, scale }
    }
    pub fn round(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        unsafe {
            let s = self.scale;
            let (x, y, w, h, r) = (x * s, y * s, w * s, h * s, r * s);
            let mut path = std::ptr::null_mut();
            GdipCreatePath(FillModeAlternate, &mut path);
            for (a, b, angle) in [
                (x, y, 180.),
                (x + w - 2. * r, y, 270.),
                (x + w - 2. * r, y + h - 2. * r, 0.),
                (x, y + h - 2. * r, 90.),
            ] {
                GdipAddPathArc(path, a, b, 2. * r, 2. * r, angle, 90.);
            }
            GdipClosePathFigure(path);
            let mut brush = std::ptr::null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillPath(self.g, brush.cast(), path);
            GdipDeleteBrush(brush.cast());
            GdipDeletePath(path);
        }
    }
    pub fn ellipse(&self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        unsafe {
            let mut brush = std::ptr::null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillEllipse(
                self.g,
                brush.cast(),
                x * self.scale,
                y * self.scale,
                w * self.scale,
                h * self.scale,
            );
            GdipDeleteBrush(brush.cast());
        }
    }
    pub fn line(&self, x: f32, y: f32, x2: f32, y2: f32, width: f32, color: u32) {
        unsafe {
            let mut pen = std::ptr::null_mut();
            GdipCreatePen1(color, width * self.scale, UnitPixel, &mut pen);
            GdipDrawLine(
                self.g,
                pen,
                x * self.scale,
                y * self.scale,
                x2 * self.scale,
                y2 * self.scale,
            );
            GdipDeletePen(pen);
        }
    }
    pub fn ring(&self, x: f32, y: f32, size: f32, width: f32, color: u32) {
        unsafe {
            let mut pen = std::ptr::null_mut();
            GdipCreatePen1(color, width * self.scale, UnitPixel, &mut pen);
            GdipDrawEllipse(
                self.g,
                pen,
                x * self.scale,
                y * self.scale,
                size * self.scale,
                size * self.scale,
            );
            GdipDeletePen(pen);
        }
    }
    pub fn triangle(&self, points: [(f32, f32); 3], color: u32) {
        self.polygon(&points, color);
    }
    /// Charging mark: a bolt `h` tall with its top-left at (x, y).
    pub fn bolt(&self, x: f32, y: f32, h: f32, color: u32) {
        let k = h / 12.;
        let p = |a: f32, b: f32| (x + a * k, y + b * k);
        self.polygon(
            &[
                p(5., 0.),
                p(0.5, 7.),
                p(3.6, 7.),
                p(2.6, 12.),
                p(7.5, 4.8),
                p(4.4, 4.8),
            ],
            color,
        );
    }
    pub fn polygon(&self, points: &[(f32, f32)], color: u32) {
        unsafe {
            let mut path = std::ptr::null_mut();
            GdipCreatePath(FillModeAlternate, &mut path);
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                GdipAddPathLine(
                    path,
                    a.0 * self.scale,
                    a.1 * self.scale,
                    b.0 * self.scale,
                    b.1 * self.scale,
                );
            }
            GdipClosePathFigure(path);
            let mut brush = std::ptr::null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillPath(self.g, brush.cast(), path);
            GdipDeleteBrush(brush.cast());
            GdipDeletePath(path);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        font: HFONT,
        color: u32,
        center: bool,
    ) {
        self.text_flags(
            text,
            [x, y, w, h],
            font,
            color,
            DT_SINGLELINE
                | DT_VCENTER
                | DT_END_ELLIPSIS
                | DT_NOPREFIX
                | if center { DT_CENTER } else { DT_LEFT },
        );
    }
    pub fn wrapped(&self, text: &str, rect: [f32; 4], font: HFONT, color: u32) {
        self.text_flags(text, rect, font, color, DT_WORDBREAK | DT_NOPREFIX);
    }
    fn text_flags(
        &self,
        text: &str,
        rect: [f32; 4],
        font: HFONT,
        color: u32,
        flags: DRAW_TEXT_FORMAT,
    ) {
        let [x, y, w, h] = rect;
        // DrawTextW reads the string pointer even for a zero-length Rust slice.
        if text.is_empty() {
            return;
        }
        unsafe {
            // GDI+ owns the original DC state; acquire it before mixing in GDI text.
            let mut dc = HDC::default();
            if GdipGetDC(self.g, &mut dc) != windows::Win32::Graphics::GdiPlus::Ok {
                return;
            }
            let old = SelectObject(dc, HGDIOBJ(font.0));
            let _ = SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, rgb(color));
            let mut value: Vec<u16> = text.encode_utf16().collect();
            let s = self.scale;
            let mut rect = RECT {
                left: (x * s) as i32,
                top: (y * s) as i32,
                right: ((x + w) * s) as i32,
                bottom: ((y + h) * s) as i32,
            };
            DrawTextW(dc, &mut value, &mut rect, flags);
            SelectObject(dc, old);
            GdipReleaseDC(self.g, dc);
        }
    }
    pub fn logo(&self, x: f32, y: f32, size: f32) {
        self.round(x, y, size, size, size * 0.28, BRAND);
        let k = size / 32.;
        self.ellipse(x + 7. * k, y + 7. * k, 9. * k, 9. * k, 0xffffffff);
        self.round(
            x + 11. * k,
            y + 11. * k,
            4. * k,
            14. * k,
            2. * k,
            0xffffffff,
        );
        self.ellipse(x + 18. * k, y + 6. * k, 8. * k, 8. * k, 0xffffffff);
        self.round(
            x + 18. * k,
            y + 10. * k,
            4. * k,
            13. * k,
            2. * k,
            0xffffffff,
        );
        self.ellipse(x + 8. * k, y + 9. * k, 3. * k, 4. * k, BRAND);
        self.ellipse(x + 22. * k, y + 8. * k, 2. * k, 3. * k, BRAND);
    }
    pub fn artwork(&self, x: f32, y: f32, scale: f32, max: bool) {
        let k = scale;
        if max {
            self.round(
                x + 12. * k,
                y + 6. * k,
                64. * k,
                54. * k,
                25. * k,
                tone(0xffc6d4cd),
            );
            self.round(
                x + 17. * k,
                y + 12. * k,
                54. * k,
                48. * k,
                22. * k,
                palette().bg,
            );
            self.round(
                x + 4. * k,
                y + 32. * k,
                24. * k,
                42. * k,
                10. * k,
                tone(0xffcbd6d0),
            );
            self.round(
                x + 60. * k,
                y + 32. * k,
                24. * k,
                42. * k,
                10. * k,
                tone(0xffcbd6d0),
            );
            self.round(
                x + 9. * k,
                y + 36. * k,
                14. * k,
                34. * k,
                7. * k,
                tone(0xffecf0ed),
            );
            self.round(
                x + 65. * k,
                y + 36. * k,
                14. * k,
                34. * k,
                7. * k,
                tone(0xffecf0ed),
            );
            return;
        }
        for (dx, dy, mirror) in [(18., 4., false), (61., 0., true)] {
            self.ellipse(
                x + (dx - 2.) * k,
                y + dy * k,
                30. * k,
                27. * k,
                tone(0xffdbe2df),
            );
            self.ellipse(
                x + dx * k,
                y + (dy + 1.) * k,
                26. * k,
                23. * k,
                tone(0xffffffff),
            );
            self.round(
                x + (dx + if mirror { 1. } else { 16. }) * k,
                y + (dy + 13.) * k,
                8. * k,
                44. * k,
                4. * k,
                tone(0xffdbe2df),
            );
            self.round(
                x + (dx + if mirror { 2. } else { 17. }) * k,
                y + (dy + 13.) * k,
                6. * k,
                43. * k,
                3. * k,
                tone(0xffffffff),
            );
            self.ellipse(
                x + (dx + if mirror { 18. } else { 4. }) * k,
                y + (dy + 7.) * k,
                5. * k,
                8. * k,
                tone(0xff253d36),
            );
            self.line(
                x + (dx + if mirror { 4. } else { 19. }) * k,
                y + (dy + 47.) * k,
                x + (dx + if mirror { 7. } else { 22. }) * k,
                y + (dy + 47.) * k,
                1.,
                tone(0xffc5d0ca),
            );
        }
        self.round(
            x + 24. * k,
            y + 64. * k,
            52. * k,
            35. * k,
            13. * k,
            tone(0xffdbe2df),
        );
        self.round(
            x + 25. * k,
            y + 65. * k,
            50. * k,
            32. * k,
            12. * k,
            tone(0xffffffff),
        );
        self.line(
            x + 27. * k,
            y + 75. * k,
            x + 73. * k,
            y + 75. * k,
            0.7,
            tone(0xffdbe2df),
        );
        self.ellipse(x + 49. * k, y + 81. * k, 2. * k, 2. * k, BRAND);
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.g);
        }
    }
}
pub fn rgb(argb: u32) -> COLORREF {
    COLORREF(((argb >> 16) & 255) | (((argb >> 8) & 255) << 8) | ((argb & 255) << 16))
}
fn font_family() -> &'static [u16] {
    static FACE: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    FACE.get_or_init(|| unsafe {
        let dc = CreateCompatibleDC(None);
        let candidate = CreateFontW(
            -14,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI Variable Text"),
        );
        let old = SelectObject(dc, HGDIOBJ(candidate.0));
        let mut name = [0u16; 64];
        GetTextFaceW(dc, Some(&mut name));
        let n = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        let available = String::from_utf16_lossy(&name[..n]) == "Segoe UI Variable Text";
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(candidate.0));
        let _ = DeleteDC(dc);
        crate::shell::wide(if available {
            "Segoe UI Variable Text"
        } else {
            "Segoe UI"
        })
    })
}
pub unsafe fn font(scale: f32, size: i32, bold: bool) -> HFONT {
    CreateFontW(
        -(size as f32 * scale).round() as i32,
        0,
        0,
        0,
        if bold { 600 } else { 400 },
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        PCWSTR(font_family().as_ptr()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_empty_label_does_not_pass_a_dangling_slice_to_windows() {
        let canvas = std::mem::ManuallyDrop::new(Canvas {
            g: std::ptr::null_mut(),
            scale: 1.,
        });
        canvas.text("", 0., 0., 10., 10., HFONT::default(), 0, false);
    }
    #[test]
    fn helper_text_fits_without_clipping_at_supported_display_scales() {
        unsafe {
            let dc = CreateCompatibleDC(None);
            for scale in [1., 1.25, 1.5, 2.] {
                let f = font(scale, 12, false);
                let old = SelectObject(dc, HGDIOBJ(f.0));
                for text in [
                    "Waiting for a clear battery signal.",
                    "Open your case to see battery levels.",
                    "Last known battery · open your case to update.",
                    "Couldn’t connect. Open the case and try again.",
                    "Couldn’t disconnect. Try Bluetooth settings.",
                    "Low battery · Left ≈10% · Right ≈10% · Case ≈10%",
                    "Pair your headphones in Bluetooth settings first.",
                ] {
                    let mut value: Vec<u16> = text.encode_utf16().collect();
                    let mut rect = RECT {
                        right: (280. * scale) as i32,
                        ..Default::default()
                    };
                    let height = DrawTextW(
                        dc,
                        &mut value,
                        &mut rect,
                        DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT,
                    );
                    assert!(
                        height > 0 && height <= (36. * scale) as i32,
                        "helper clipped at {scale}: {text}"
                    );
                }
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
            let _ = DeleteDC(dc);
        }
    }
    #[test]
    fn ui_fonts_keep_the_selected_family_and_scale() {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let expected = String::from_utf16_lossy(&font_family()[..font_family().len() - 1]);
            for scale in [1., 1.25, 1.5, 2.] {
                for (size, bold) in [(11, false), (12, false), (14, true), (17, true), (22, true)] {
                    let f = font(scale, size, bold);
                    assert!(!f.is_invalid());
                    let old = SelectObject(dc, HGDIOBJ(f.0));
                    let mut face = [0u16; 64];
                    GetTextFaceW(dc, Some(&mut face));
                    let n = face.iter().position(|c| *c == 0).unwrap_or(face.len());
                    assert_eq!(String::from_utf16_lossy(&face[..n]), expected);
                    let mut info = LOGFONTW::default();
                    assert_ne!(
                        GetObjectW(
                            HGDIOBJ(f.0),
                            std::mem::size_of::<LOGFONTW>() as i32,
                            Some((&mut info as *mut LOGFONTW).cast())
                        ),
                        0
                    );
                    assert_eq!(info.lfHeight, -(size as f32 * scale).round() as i32);
                    SelectObject(dc, old);
                    let _ = DeleteObject(HGDIOBJ(f.0));
                }
            }
            let _ = DeleteDC(dc);
        }
    }
}
