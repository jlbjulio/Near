use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Gdi::*, GdiPlus::*},
    },
    core::{PCWSTR, w},
};
pub const BG: u32 = 0xfff7f8f6;
pub const INK: u32 = 0xff182c29;
pub const MUTED: u32 = 0xff566862;
pub const ACCENT: u32 = 0xff236759;
pub const LINE: u32 = 0xffdee5e0;
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
        unsafe {
            let mut path = std::ptr::null_mut();
            GdipCreatePath(FillModeAlternate, &mut path);
            for i in 0..3 {
                let a = points[i];
                let b = points[(i + 1) % 3];
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
            DrawTextW(
                dc,
                &mut value,
                &mut rect,
                DT_SINGLELINE
                    | DT_VCENTER
                    | DT_END_ELLIPSIS
                    | DT_NOPREFIX
                    | if center { DT_CENTER } else { DT_LEFT },
            );
            SelectObject(dc, old);
            GdipReleaseDC(self.g, dc);
        }
    }
    pub fn logo(&self, x: f32, y: f32, size: f32) {
        self.round(x, y, size, size, size * 0.28, ACCENT);
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
        self.ellipse(x + 8. * k, y + 9. * k, 3. * k, 4. * k, ACCENT);
        self.ellipse(x + 22. * k, y + 8. * k, 2. * k, 3. * k, ACCENT);
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
                0xffc6d4cd,
            );
            self.round(x + 17. * k, y + 12. * k, 54. * k, 48. * k, 22. * k, BG);
            self.round(
                x + 4. * k,
                y + 32. * k,
                24. * k,
                42. * k,
                10. * k,
                0xffcbd6d0,
            );
            self.round(
                x + 60. * k,
                y + 32. * k,
                24. * k,
                42. * k,
                10. * k,
                0xffcbd6d0,
            );
            self.round(
                x + 9. * k,
                y + 36. * k,
                14. * k,
                34. * k,
                7. * k,
                0xffecf0ed,
            );
            self.round(
                x + 65. * k,
                y + 36. * k,
                14. * k,
                34. * k,
                7. * k,
                0xffecf0ed,
            );
            return;
        }
        for (dx, dy, mirror) in [(18., 4., false), (61., 0., true)] {
            self.ellipse(x + (dx - 2.) * k, y + dy * k, 30. * k, 27. * k, 0xffdbe2df);
            self.ellipse(x + dx * k, y + (dy + 1.) * k, 26. * k, 23. * k, 0xffffffff);
            self.round(
                x + (dx + if mirror { 1. } else { 16. }) * k,
                y + (dy + 13.) * k,
                8. * k,
                44. * k,
                4. * k,
                0xffdbe2df,
            );
            self.round(
                x + (dx + if mirror { 2. } else { 17. }) * k,
                y + (dy + 13.) * k,
                6. * k,
                43. * k,
                3. * k,
                0xffffffff,
            );
            self.ellipse(
                x + (dx + if mirror { 18. } else { 4. }) * k,
                y + (dy + 7.) * k,
                5. * k,
                8. * k,
                0xff253d36,
            );
            self.line(
                x + (dx + if mirror { 4. } else { 19. }) * k,
                y + (dy + 47.) * k,
                x + (dx + if mirror { 7. } else { 22. }) * k,
                y + (dy + 47.) * k,
                1.,
                0xffc5d0ca,
            );
        }
        self.round(
            x + 24. * k,
            y + 64. * k,
            52. * k,
            35. * k,
            13. * k,
            0xffdbe2df,
        );
        self.round(
            x + 25. * k,
            y + 65. * k,
            50. * k,
            32. * k,
            12. * k,
            0xffffffff,
        );
        self.line(
            x + 27. * k,
            y + 75. * k,
            x + 73. * k,
            y + 75. * k,
            0.7,
            0xffdbe2df,
        );
        self.ellipse(x + 49. * k, y + 81. * k, 2. * k, 2. * k, ACCENT);
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
