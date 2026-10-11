pub mod paint;
use crate::{
    bluetooth::advertisement::is_max,
    model::*,
    shell::{self, tray::Tray},
};
use paint::*;
use std::result::Result::Ok;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    time::Instant,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::{Input::KeyboardAndMouse::EnableWindow, Shell::NIN_SELECT};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*, GdiPlus::*},
        System::LibraryLoader::*,
        UI::{Controls::*, HiDpi::*, WindowsAndMessaging::*},
    },
    core::{BOOL, PCWSTR, w},
};
const NIN_KEYSELECT: u32 = 0x401;
unsafe fn send(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    windows::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, msg, Some(wparam), Some(lparam))
}
const WIDTH: f32 = 320.;
const HEIGHT: f32 = 388.;
fn preview_model(state: Option<&str>) -> ViewModel {
    let mut vm = ViewModel {
        preferences: Preferences::default(),
        ..Default::default()
    };
    if state.is_none() || state == Some("empty") {
        return vm;
    }
    vm.chosen = Some(Paired {
        address: 7,
        name: "AirPods 4".into(),
        model: Some(0x201b),
        ..Default::default()
    });
    vm.paired = vec![vm.chosen.clone().unwrap()];
    vm.name = "AirPods 4".into();
    vm.model = 0x201b;
    vm.state = "Nearby".into();
    vm.available = true;
    match state {
        Some("connected") => {
            vm.chosen.as_mut().unwrap().connected = true;
            vm.state = "Connected".into();
            vm.note = "Estimated battery · updates nearby.".into();
            vm.batteries.approximate = true;
            vm.batteries.parts = [80, 90, 70].map(|level| {
                Some(Sample {
                    level,
                    at: now(),
                    charging: Some(true),
                })
            });
        }
        Some("disconnected") => {
            vm.available = false;
            vm.preferences.disconnected_addresses.push(7);
            vm.state = "Not connected".into();
        }
        Some("connecting") => {
            vm.connecting = true;
            vm.state = "Connecting…".into();
        }
        Some("disconnecting") => {
            vm.disconnecting = true;
            vm.state = "Disconnecting…".into();
        }
        Some("error") => {
            vm.message = "Couldn’t connect. Open the case and try again.".into();
        }
        Some("uncertain") => {
            vm.note = "Waiting for a clear battery signal.".into();
            vm.available = false;
        }
        _ => {}
    }
    vm
}
struct Control {
    window: HWND,
    id: u16,
    page: u8,
    rect: [f32; 4],
    label: String,
    combo: bool,
}
struct Animation {
    start: Instant,
    show: bool,
    x: i32,
    y: i32,
}
struct App {
    hwnd: HWND,
    shared: Arc<Mutex<ViewModel>>,
    tx: SyncSender<Event>,
    vm: ViewModel,
    tray: Option<Tray>,
    page: u8,
    scale: f32,
    controls: Vec<Control>,
    fonts: HashMap<(i32, bool), HFONT>,
    animation: Option<Animation>,
    automatic: bool,
    dismiss_at: u64,
    moved: bool,
    popup_revision: u64,
    alert_revision: u64,
    preview: bool,
    preview_file: Option<String>,
    taskbar_created: u32,
    combo_stamp: String,
    output_stamp: String,
}
pub fn run(
    open: bool,
    preview: bool,
    preview_file: Option<String>,
    preview_state: Option<String>,
) -> windows::core::Result<()> {
    unsafe {
        let _apartment = crate::shell::Apartment::mta();
        let mut token = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        if GdiplusStartup(&mut token, &input, std::ptr::null_mut())
            != windows::Win32::Graphics::GdiPlus::Ok
        {
            return Err(windows::core::Error::from_thread());
        }
        let instance = GetModuleHandleW(None)?;
        let class = if preview {
            w!("Near.Native.Preview")
        } else {
            w!("Near.Native.Flyout")
        };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(instance.0),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            style: CS_DBLCLKS,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let (tx, rx) = mpsc::sync_channel(256);
        let vm = ViewModel::default();
        let shared = Arc::new(Mutex::new(vm.clone()));
        let cell = Box::new(RefCell::new(App {
            hwnd: HWND::default(),
            shared: shared.clone(),
            tx: tx.clone(),
            vm,
            tray: None,
            page: 0,
            scale: 1.,
            controls: vec![],
            fonts: HashMap::new(),
            animation: None,
            automatic: false,
            dismiss_at: 0,
            moved: false,
            popup_revision: 0,
            alert_revision: 0,
            preview,
            preview_file,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            combo_stamp: String::new(),
            output_stamp: String::new(),
        }));
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
            class,
            w!("Near"),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            Some((&*cell as *const RefCell<App>).cast()),
        )?;
        let mut backend_thread = None;
        {
            let mut app = cell.borrow_mut();
            app.hwnd = window;
            if preview {
                app.vm = preview_model(preview_state.as_deref());
                *app.shared.lock().unwrap() = app.vm.clone();
            }
            app.scale = GetDpiForWindow(window).max(96) as f32 / 96.;
            app.create_controls()?;
            app.layout();
            app.update();
            let _ = SetLayeredWindowAttributes(window, COLORREF(0), 255, LWA_ALPHA);
            let corner = 2u32;
            let _ = DwmSetWindowAttribute(
                window,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&corner as *const u32).cast(),
                4,
            );
            let color = rgb(LINE).0;
            let _ =
                DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR, (&color as *const u32).cast(), 4);
            if !preview {
                app.tray = Some(Tray::new(window)?);
                let vm = app.vm.clone();
                app.tray.as_mut().unwrap().update(&vm);
            }

            if !preview {
                let handle = window.0 as usize;
                backend_thread = Some(
                    std::thread::Builder::new()
                        .name("Near Bluetooth".into())
                        .spawn(move || crate::backend::run(shared, rx, tx, handle))
                        .map_err(|_| windows::core::Error::from_thread())?,
                );
            }
            if open {
                app.show(false);
            }
            if preview {
                SetTimer(Some(window), 9, 1000, None);
            }
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = cell.borrow().tx.send(Event::Command(Command::Quit));
        if let Some(thread) = backend_thread {
            let _ = thread.join();
        }
        cell.borrow_mut().tray = None;
        drop(cell);
        GdiplusShutdown(token);
        Ok(())
    }
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let cell = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
    if cell.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    if message == WM_PRINT {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    if message == WM_TIMER && wparam.0 == 9 {
        let _ = KillTimer(Some(hwnd), 9);
        let file = (*cell).borrow().preview_file.clone();
        if let Some(file) = file {
            capture_window(hwnd, &file);
            let _ = DestroyWindow(hwnd);
        }
        return LRESULT(0);
    }
    // Native calls can synchronously re-enter wndproc; RefCell prevents aliased mutable state.
    let Ok(mut app) = (*cell).try_borrow_mut() else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    match message {
        WM_PAINT => {
            app.paint();
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DRAWITEM => {
            app.draw_control(&*(lparam.0 as *const DRAWITEMSTRUCT));
            LRESULT(1)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as u16;
            let notification = ((wparam.0 >> 16) & 0xffff) as u16;
            app.command(id, notification);
            LRESULT(0)
        }
        WM_CLOSE => {
            app.hide();
            LRESULT(0)
        }
        WM_PRINTCLIENT => {
            app.draw(HDC(wparam.0 as *mut _));
            LRESULT(0)
        }
        WM_ACTIVATE if wparam.0 & 0xffff == WA_INACTIVE as usize => {
            if !app.automatic {
                app.hide();
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            let hit = DefWindowProcW(hwnd, message, wparam, lparam);
            if hit.0 == HTCLIENT as isize {
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let x = (lparam.0 as u16 as i16) as i32 - rect.left;
                let y = ((lparam.0 >> 16) as u16 as i16) as i32 - rect.top;
                if y < (56. * app.scale) as i32 && x < ((WIDTH - 100.) * app.scale) as i32 {
                    return LRESULT(HTCAPTION as isize);
                }
            }
            hit
        }
        WM_NCLBUTTONDOWN => {
            app.moved = true;
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_EXITSIZEMOVE => {
            app.clamp();
            LRESULT(0)
        }
        WM_DPICHANGED => {
            app.scale = (wparam.0 & 0xffff) as f32 / 96.;
            app.clear_fonts();
            app.layout();
            app.clamp();
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == 27 => {
            app.hide();
            LRESULT(0)
        }
        WM_TIMER => {
            app.timer(wparam.0);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 == 7 || wparam.0 == 18 {
                let _ = app.tx.try_send(Event::Command(Command::Restart));
            }
            LRESULT(1)
        }
        shell::UPDATE => {
            app.update();
            LRESULT(0)
        }
        shell::IPC => {
            match wparam.0 {
                3 => app.quit(),
                _ => app.activate(),
            }
            LRESULT(0)
        }
        shell::TRAY => {
            match (lparam.0 & 0xffff) as u32 {
                WM_LBUTTONUP | NIN_SELECT | NIN_KEYSELECT => {
                    app.activate();
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => app.menu(),
                _ => {}
            }
            LRESULT(0)
        }
        _ if message == app.taskbar_created => {
            if let Some(tray) = &mut app.tray {
                tray.register();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
impl App {
    unsafe fn f(&mut self, size: i32, bold: bool) -> HFONT {
        if let Some(f) = self.fonts.get(&(size, bold)) {
            return *f;
        }
        let f = font(self.scale, size, bold);
        self.fonts.insert((size, bold), f);
        f
    }
    unsafe fn clear_fonts(&mut self) {
        for (_, f) in self.fonts.drain() {
            let _ = DeleteObject(HGDIOBJ(f.0));
        }
    }
    unsafe fn add(
        &mut self,
        id: u16,
        page: u8,
        label: &str,
        rect: [f32; 4],
        combo: bool,
    ) -> windows::core::Result<()> {
        let text = shell::wide(label);
        let style = WS_CHILD
            | WS_TABSTOP
            | if combo {
                WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0)
            } else {
                WINDOW_STYLE(BS_OWNERDRAW as u32)
            };
        let handle = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            if combo { w!("COMBOBOX") } else { w!("BUTTON") },
            PCWSTR(text.as_ptr()),
            style,
            0,
            0,
            0,
            0,
            Some(self.hwnd),
            Some(HMENU(id as usize as *mut _)),
            None,
            None,
        )?;
        self.controls.push(Control {
            window: handle,
            id,
            page,
            rect,
            label: label.into(),
            combo,
        });
        Ok(())
    }
    unsafe fn create_controls(&mut self) -> windows::core::Result<()> {
        self.add(1, 255, "Settings", [240., 12., 28., 28.], false)?;
        self.add(2, 255, "Hide Near", [276., 12., 28., 28.], false)?;
        self.add(3, 254, "Back", [14., 53., 26., 28.], false)?;
        self.add(10, 0, "Connect", [20., 308., 280., 36.], false)?;
        self.add(11, 0, "Devices", [20., 350., 280., 26.], false)?;
        self.add(20, 0, "Previous track", [200., 252., 28., 36.], false)?;
        self.add(21, 0, "Play or pause", [232., 252., 36., 36.], false)?;
        self.add(22, 0, "Next track", [272., 252., 28., 36.], false)?;
        for (id, label, y) in [
            (30, "Start with Windows", 96.),
            (31, "Case popup", 130.),
            (32, "Animations", 164.),
            (33, "Low battery alerts", 198.),
            (34, "Automatic pause", 232.),
            (35, "Automatic switching", 266.),
        ] {
            self.add(id, 1, label, [20., y, 280., 32.], false)?;
        }
        self.add(90, 1, "Quit Near", [20., 346., 280., 28.], false)?;
        self.add(50, 2, "Paired headphones", [20., 112., 280., 160.], true)?;
        self.add(53, 2, "Refresh", [20., 155., 134., 32.], false)?;
        self.add(54, 2, "Bluetooth settings", [166., 155., 134., 32.], false)?;
        self.add(51, 2, "Speakers", [20., 245., 280., 160.], true)?;
        Ok(())
    }
    unsafe fn layout(&mut self) {
        let height = HEIGHT;
        let s = self.scale;
        let f = self.f(12, false);
        let _ = SetWindowPos(
            self.hwnd,
            None,
            0,
            0,
            (WIDTH * s).round() as i32,
            (height * s).round() as i32,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        for control in &self.controls {
            let [x, y, w, h] = control.rect;
            let _ = SetWindowPos(
                control.window,
                None,
                (x * s).round() as i32,
                (y * s).round() as i32,
                (w * s).round() as i32,
                (h * s).round() as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            send(control.window, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
            if control.combo {
                send(
                    control.window,
                    CB_SETITEMHEIGHT,
                    WPARAM(usize::MAX),
                    LPARAM((24. * s) as isize),
                );
                send(
                    control.window,
                    CB_SETITEMHEIGHT,
                    WPARAM(0),
                    LPARAM((24. * s) as isize),
                );
            }
            let visible = (control.page == 255
                || control.page == 254 && self.page != 0
                || control.page == self.page)
                && (control.id != 10 || self.vm.connection_action().is_some());
            let _ = ShowWindow(control.window, if visible { SW_SHOWNA } else { SW_HIDE });
        }
        let _ = InvalidateRect(Some(self.hwnd), None, false);
    }
    unsafe fn update(&mut self) {
        if let Ok(v) = self.shared.lock() {
            self.vm = v.clone();
        }
        if let Some(tray) = &mut self.tray {
            tray.update(&self.vm);
            if self.vm.alert_revision != self.alert_revision {
                self.alert_revision = self.vm.alert_revision;
                tray.alert(&self.vm.alert);
            }
        }
        if self.vm.popup_revision != self.popup_revision {
            self.popup_revision = self.vm.popup_revision;
            if !IsWindowVisible(self.hwnd).as_bool() {
                self.page = 0;
                self.layout();
                self.show(true);
            }
        }
        let chosen = self.vm.chosen.as_ref().map(|p| p.address);
        let stamp = format!(
            "{:?}:{chosen:?}",
            self.vm
                .paired
                .iter()
                .map(|p| (&p.name, p.address))
                .collect::<Vec<_>>()
        );
        if stamp != self.combo_stamp {
            self.combo_stamp = stamp;
            let h = self.control(50);
            send(h, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
            if self.vm.paired.is_empty() {
                let value = shell::wide("No paired headphones");
                send(h, CB_ADDSTRING, WPARAM(0), LPARAM(value.as_ptr() as isize));
                send(h, CB_SETCURSEL, WPARAM(0), LPARAM(0));
            }
            let _ = EnableWindow(h, !self.vm.paired.is_empty());
            for p in &self.vm.paired {
                let value = shell::wide(&p.name);
                send(h, CB_ADDSTRING, WPARAM(0), LPARAM(value.as_ptr() as isize));
            }
            if let Some(index) = self
                .vm
                .paired
                .iter()
                .position(|p| Some(p.address) == chosen)
            {
                send(h, CB_SETCURSEL, WPARAM(index), LPARAM(0));
            }
        }
        let stamp = format!(
            "{:?}:{:?}:{:?}",
            self.vm.endpoints, self.vm.preferences.speakers_id, chosen
        );
        if stamp != self.output_stamp {
            self.output_stamp = stamp;
            {
                let target = self.vm.preferences.speakers_id.as_ref();
                let outputs: Vec<_> = self
                    .vm
                    .endpoints
                    .iter()
                    .filter(|e| crate::audio::is_speaker_target(e, self.vm.chosen.as_ref()))
                    .collect();
                let h = self.control(51);
                send(h, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
                let automatic = shell::wide("Automatic");
                send(
                    h,
                    CB_ADDSTRING,
                    WPARAM(0),
                    LPARAM(automatic.as_ptr() as isize),
                );
                for endpoint in &outputs {
                    let value = shell::wide(&endpoint.name);
                    send(h, CB_ADDSTRING, WPARAM(0), LPARAM(value.as_ptr() as isize));
                }
                let index = target
                    .and_then(|t| outputs.iter().position(|e| &e.id == t))
                    .map(|i| i + 1)
                    .unwrap_or(0);
                send(h, CB_SETCURSEL, WPARAM(index), LPARAM(0));
                let _ = EnableWindow(h, !outputs.is_empty());
            }
        }
        let action = self.vm.connection_action();
        let (label, enabled) = action.unwrap_or(("Connect", false));
        let value = shell::wide(label);
        let _ = SetWindowTextW(self.control(10), PCWSTR(value.as_ptr()));
        let _ = EnableWindow(self.control(10), enabled);
        let _ = ShowWindow(
            self.control(10),
            if self.page == 0 && action.is_some() {
                SW_SHOWNA
            } else {
                SW_HIDE
            },
        );
        for (id, available) in [
            (20, self.vm.media.previous),
            (21, self.vm.media.toggle),
            (22, self.vm.media.next),
        ] {
            let _ = EnableWindow(self.control(id), available);
        }
        if IsWindowVisible(self.hwnd).as_bool() {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            for c in &self.controls {
                if !c.combo {
                    let _ = InvalidateRect(Some(c.window), None, false);
                }
            }
        }
    }
    fn control(&self, id: u16) -> HWND {
        self.controls
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.window)
            .unwrap_or_default()
    }
    unsafe fn command(&mut self, id: u16, notification: u16) {
        if notification == CBN_SELCHANGE as u16 {
            let index = send(self.control(id), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
            if id == 50 {
                if let Some(p) = self.vm.paired.get(index as usize) {
                    self.send(Command::Select(p.address));
                }
            } else if id == 51 {
                let value = if index <= 0 {
                    None
                } else {
                    self.vm
                        .endpoints
                        .iter()
                        .filter(|e| crate::audio::is_speaker_target(e, self.vm.chosen.as_ref()))
                        .nth(index as usize - 1)
                        .map(|e| e.id.clone())
                };
                let change = PreferenceChange::Speakers(value);
                self.vm.preferences.apply(change.clone());
                self.send(Command::Preferences(change));
            }
            return;
        }
        match id {
            1 => {
                self.page = if self.page == 1 { 0 } else { 1 };
                self.automatic = false;
                self.layout();
            }
            2 => self.hide(),
            3 => {
                self.page = 0;
                self.layout();
            }
            10 => {
                if self
                    .vm
                    .connection_action()
                    .is_some_and(|(_, enabled)| enabled)
                {
                    self.send(if self.vm.chosen.as_ref().is_some_and(|p| p.connected) {
                        Command::Disconnect
                    } else {
                        Command::Connect
                    });
                }
            }
            11 => {
                self.page = 2;
                self.automatic = false;
                self.layout();
            }
            20 => self.send(Command::Media("previous")),
            21 => self.send(Command::Media("toggle")),
            22 => self.send(Command::Media("next")),
            30 => {
                if self.preview {
                    return;
                }
                if let Err(e) = shell::startup::set(!shell::startup::enabled()) {
                    self.vm.message = "Could not update startup.".into();
                    shell::log(&e.to_string());
                }
                let _ = InvalidateRect(Some(self.control(30)), None, false);
            }
            31..=35 => {
                let change = match id {
                    31 => PreferenceChange::TogglePopup,
                    32 => PreferenceChange::ToggleMotion,
                    33 => PreferenceChange::ToggleLowBattery,
                    34 => PreferenceChange::ToggleEarDetection,
                    _ => PreferenceChange::ToggleSpeakerSwitch,
                };
                self.vm.preferences.apply(change.clone());
                self.send(Command::Preferences(change));
                let _ = InvalidateRect(Some(self.control(id)), None, false);
            }
            53 => self.send(Command::Refresh),
            54 => shell::open_bluetooth(),
            90 => self.quit(),
            _ => {}
        }
    }
    fn send(&self, command: Command) {
        if !self.preview && self.tx.try_send(Event::Command(command)).is_err() {
            shell::log("The command queue was full.");
        }
    }
    unsafe fn placement(&mut self) -> (i32, i32) {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let anchor = self.tray.as_ref().and_then(|t| t.anchor());
        if let Some(r) = anchor {
            point = POINT {
                x: (r.left + r.right) / 2,
                y: r.top,
            };
        }
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let s = self.scale;
        let work = info.rcWork;
        let w = (WIDTH * s) as i32;
        let h = (HEIGHT * s) as i32;
        let desired = anchor
            .map(|r| ((r.left + r.right - w) / 2, work.bottom - h - 12))
            .unwrap_or((work.right - w - 12, work.bottom - h - 12));
        (
            desired
                .0
                .clamp(work.left + 8, (work.right - w - 8).max(work.left + 8)),
            desired
                .1
                .clamp(work.top + 8, (work.bottom - h - 8).max(work.top + 8)),
        )
    }
    unsafe fn animations_enabled(&self) -> bool {
        let mut enabled = BOOL(1);
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        self.vm.preferences.motion && enabled.as_bool()
    }
    unsafe fn show(&mut self, automatic: bool) {
        self.automatic = automatic;
        self.dismiss_at = now() + 8000;
        if !automatic {
            let _ = KillTimer(Some(self.hwnd), 2);
        }
        if IsWindowVisible(self.hwnd).as_bool() {
            // A tray click can arrive while deactivation is fading the window out.
            if self.animation.as_ref().is_some_and(|a| !a.show) {
                let a = self.animation.take().unwrap();
                let _ = KillTimer(Some(self.hwnd), 1);
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    a.x,
                    a.y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
            if !automatic {
                let _ = SetForegroundWindow(self.hwnd);
            }
            self.update();
            return;
        }
        let (x, y) = if self.moved {
            let mut r = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut r);
            (r.left, r.top)
        } else {
            self.placement()
        };
        let _ = SetWindowPos(
            self.hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(
            self.hwnd,
            if automatic {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        );
        if !automatic {
            let _ = SetForegroundWindow(self.hwnd);
        }
        if self.animations_enabled() {
            self.animation = Some(Animation {
                start: Instant::now(),
                show: true,
                x,
                y,
            });
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 0, LWA_ALPHA);
            SetTimer(Some(self.hwnd), 1, 16, None);
        } else {
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 255, LWA_ALPHA);
        }
        if automatic {
            SetTimer(Some(self.hwnd), 2, 400, None);
        }
        self.update();
    }
    unsafe fn activate(&mut self) {
        if !IsWindowVisible(self.hwnd).as_bool() {
            self.page = 0;
            self.layout();
        }
        self.show(false);
    }
    unsafe fn hide(&mut self) {
        if !IsWindowVisible(self.hwnd).as_bool() {
            return;
        }
        self.automatic = false;
        let _ = KillTimer(Some(self.hwnd), 2);
        if self.animations_enabled() {
            let mut r = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut r);
            self.animation = Some(Animation {
                start: Instant::now(),
                show: false,
                x: r.left,
                y: r.top,
            });
            SetTimer(Some(self.hwnd), 1, 16, None);
        } else {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
    unsafe fn timer(&mut self, id: usize) {
        if id == 1 {
            if let Some(a) = &self.animation {
                let fraction = (a.start.elapsed().as_secs_f32() / 0.19).min(1.);
                let ease = 1. - (1. - fraction).powi(3);
                let alpha = if a.show { ease } else { 1. - ease };
                let offset = if a.show { (1. - ease) * 8. } else { ease * 5. };
                let _ = SetLayeredWindowAttributes(
                    self.hwnd,
                    COLORREF(0),
                    (alpha * 255.) as u8,
                    LWA_ALPHA,
                );
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    a.x,
                    a.y + (offset * self.scale) as i32,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                if fraction >= 1. {
                    let show = a.show;
                    let (x, y) = (a.x, a.y);
                    self.animation = None;
                    let _ = KillTimer(Some(self.hwnd), 1);
                    if !show {
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                    }
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        x,
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 255, LWA_ALPHA);
                }
            }
        } else if id == 2 {
            let mut p = POINT::default();
            let mut r = RECT::default();
            let _ = GetCursorPos(&mut p);
            let _ = GetWindowRect(self.hwnd, &mut r);
            if p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom {
                self.dismiss_at = now() + 4000;
            } else if now() >= self.dismiss_at {
                self.hide();
            }
        }
    }
    unsafe fn clamp(&mut self) {
        let mut r = RECT::default();
        let _ = GetWindowRect(self.hwnd, &mut r);
        let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
        let mut m = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut m);
        let x = r.left.clamp(
            m.rcWork.left,
            (m.rcWork.right - (r.right - r.left)).max(m.rcWork.left),
        );
        let y = r.top.clamp(
            m.rcWork.top,
            (m.rcWork.bottom - (r.bottom - r.top)).max(m.rcWork.top),
        );
        let _ = SetWindowPos(
            self.hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    unsafe fn menu(&mut self) {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Open Near"));
        let _ = AppendMenuW(menu, MF_STRING, 90, w!("Quit Near"));
        let _ = SetForegroundWindow(self.hwnd);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            p.x,
            p.y,
            None,
            self.hwnd,
            None,
        )
        .0;
        let _ = DestroyMenu(menu);
        if selected == 90 {
            self.quit();
        } else if selected == 1 {
            self.page = 0;
            self.layout();
            self.show(false);
        }
    }
    unsafe fn quit(&mut self) {
        self.tray = None;
        let _ = DestroyWindow(self.hwnd);
    }
    unsafe fn paint(&mut self) {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(self.hwnd, &mut ps);
        let mut bounds = RECT::default();
        let _ = GetClientRect(self.hwnd, &mut bounds);
        let buffer = CreateCompatibleDC(Some(dc));
        let bitmap = CreateCompatibleBitmap(dc, bounds.right, bounds.bottom);
        if !buffer.is_invalid() && !bitmap.is_invalid() {
            let old = SelectObject(buffer, HGDIOBJ(bitmap.0));
            self.draw(buffer);
            let _ = BitBlt(
                dc,
                0,
                0,
                bounds.right,
                bounds.bottom,
                Some(buffer),
                0,
                0,
                SRCCOPY,
            );
            SelectObject(buffer, old);
        } else {
            self.draw(dc);
        }
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(buffer);
        let _ = EndPaint(self.hwnd, &ps);
    }
    unsafe fn draw(&mut self, dc: HDC) {
        let small = self.f(11, false);
        let normal = self.f(12, false);
        let medium = self.f(14, true);
        let large = self.f(22, true);
        let title = self.f(17, true);
        let c = Canvas::new(dc, self.scale);
        c.round(0., 0., WIDTH, HEIGHT, 12., BG);
        c.logo(20., 14., 22.);
        c.text("near", 50., 12., 110., 26., medium, INK, false);
        if self.page != 0 {
            c.text(
                if self.page == 1 {
                    "Settings"
                } else {
                    "Devices"
                },
                46.,
                53.,
                220.,
                30.,
                title,
                INK,
                false,
            );
        }
        if self.page == 0 {
            c.text(&self.vm.name, 20., 53., 211., 28., title, INK, false);
            c.text(
                &self.vm.state,
                20.,
                83.,
                219.,
                20.,
                normal,
                if self.vm.chosen.as_ref().is_some_and(|p| p.connected) {
                    ACCENT
                } else {
                    MUTED
                },
                false,
            );
            c.artwork(246., 54., 0.54, is_max(self.vm.model));
            c.round(20., 120., 280., 78., 10., 0xffffffff);
            for i in 0..3 {
                let x = 32. + i as f32 * 91.;
                let label = if is_max(self.vm.model) && i == 0 {
                    "Headphones"
                } else {
                    ["Left", "Right", "Case"][i]
                };
                c.text(label, x, 129., 80., 18., small, MUTED, false);
                c.text(
                    &self.vm.batteries.value(i),
                    x,
                    149.,
                    80.,
                    29.,
                    large,
                    INK,
                    false,
                );
                if self.vm.batteries.parts[i].is_some_and(|s| s.charging == Some(true)) {
                    c.text("Charging", x, 179., 80., 16., small, ACCENT, false);
                } else {
                    c.round(x, 187., 70., 3., 1.5, LINE);
                    if let Some(sample) = self.vm.batteries.parts[i] {
                        c.round(
                            x,
                            187.,
                            (70. * sample.level as f32 / 100.).max(3.),
                            3.,
                            1.5,
                            ACCENT,
                        );
                    }
                }
            }
            let helper = if self.vm.message.is_empty() || self.vm.message == "Connecting…" {
                &self.vm.note
            } else {
                &self.vm.message
            };
            c.wrapped(helper, [20., 208., 280., 36.], normal, MUTED);
            let media_title = if self.vm.media.title.is_empty() {
                "Nothing playing"
            } else {
                &self.vm.media.title
            };
            c.text(media_title, 20., 254., 169., 18., normal, INK, false);
            c.text(
                &self.vm.media.artist,
                20.,
                276.,
                169.,
                16.,
                small,
                MUTED,
                false,
            );
        } else if self.page == 2 {
            c.text("Your headphones", 20., 90., 280., 18., small, MUTED, false);
            c.text(
                "Automatic switching",
                20.,
                199.,
                280.,
                20.,
                normal,
                INK,
                false,
            );
            c.text("Speakers", 20., 225., 280., 16., small, MUTED, false);
            c.wrapped(
                if self.vm.paired.is_empty() {
                    "Pair your headphones in Bluetooth settings first."
                } else {
                    "Pair once in Windows. Near reconnects for you."
                },
                [20., 289., 280., 36.],
                normal,
                MUTED,
            );
        }
    }
    unsafe fn draw_control(&mut self, item: &DRAWITEMSTRUCT) {
        let id = item.CtlID as u16;
        let Some(control) = self.controls.iter().find(|c| c.id == id) else {
            return;
        };
        let label = control.label.clone();
        let [_, _, w, h] = control.rect;
        let f = self.f(12, false);
        let c = Canvas::new(item.hDC, self.scale);
        c.round(0., 0., w, h, 0.1, BG);
        let focus = item.itemState.0 & ODS_FOCUS.0 != 0;
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let ink = if disabled { MUTED } else { INK };
        if (30..=35).contains(&id) {
            let enabled = match id {
                30 => shell::startup::enabled(),
                31 => self.vm.preferences.popup,
                32 => self.vm.preferences.motion,
                33 => self.vm.preferences.low_battery,
                34 => self.vm.preferences.ear_detection,
                _ => self.vm.preferences.speaker_switch,
            };
            c.text(&label, 0., 0., w - 48., h, f, INK, false);
            c.round(
                w - 32.,
                7.,
                32.,
                18.,
                9.,
                if enabled { ACCENT } else { 0xff788780 },
            );
            c.ellipse(
                w - 29. + if enabled { 14. } else { 0. },
                10.,
                12.,
                12.,
                0xffffffff,
            );
        } else if id == 1 || id == 2 || id == 3 {
            if pressed || focus {
                c.round(0., 0., w, h, 7., LINE);
            }
            let (x, y) = (w / 2., h / 2.);
            if id == 1 {
                c.ring(x - 5., y - 5., 10., 1.4, MUTED);
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::FRAC_PI_4;
                    c.line(
                        x + angle.cos() * 6.,
                        y + angle.sin() * 6.,
                        x + angle.cos() * 8.,
                        y + angle.sin() * 8.,
                        1.4,
                        MUTED,
                    );
                }
                c.ring(x - 1.5, y - 1.5, 3., 1.2, MUTED);
            } else if id == 2 {
                c.line(x - 4., y - 4., x + 4., y + 4., 1.4, MUTED);
                c.line(x + 4., y - 4., x - 4., y + 4., 1.4, MUTED);
            } else {
                c.line(x + 2., y - 5., x - 3., y, 1.5, MUTED);
                c.line(x - 3., y, x + 2., y + 5., 1.5, MUTED);
            }
        } else if id == 20 || id == 21 || id == 22 {
            if id == 21 || pressed || focus {
                c.round(0., 3., w, h - 6., 8., LINE);
            }
            let (x, y) = (w / 2., h / 2.);
            if id == 21 && self.vm.media.playing {
                c.round(x - 5., y - 6., 3., 12., 0.8, ink);
                c.round(x + 2., y - 6., 3., 12., 0.8, ink);
            } else if id == 21 {
                c.triangle([(x - 4., y - 6.), (x + 6., y), (x - 4., y + 6.)], ink);
            } else {
                let direction = if id == 20 { -1. } else { 1. };
                c.triangle(
                    [
                        (x - direction * 3., y - 5.),
                        (x + direction * 4., y),
                        (x - direction * 3., y + 5.),
                    ],
                    ink,
                );
                c.line(
                    x + direction * 5.,
                    y - 5.,
                    x + direction * 5.,
                    y + 5.,
                    1.4,
                    ink,
                );
            }
        } else {
            let primary = id == 10
                && !self.vm.disconnecting
                && !self.vm.chosen.as_ref().is_some_and(|p| p.connected);
            let color = if primary && !disabled {
                if pressed { 0xff194b40 } else { ACCENT }
            } else if id == 11 || id == 90 {
                BG
            } else {
                LINE
            };
            c.round(0., 0., w, h, 8., color);
            if id == 10 && !primary {
                c.round(1., 1., w - 2., h - 2., 7., BG);
            }
            let text = if id == 10 {
                self.vm
                    .connection_action()
                    .map(|(label, _)| label)
                    .unwrap_or("Connect")
            } else {
                &label
            };
            c.text(
                text,
                4.,
                0.,
                w - 8.,
                h,
                f,
                if primary && !disabled {
                    0xffffffff
                } else {
                    ink
                },
                true,
            );
        }
        drop(c);
        if focus {
            let r = RECT {
                left: 2,
                top: 2,
                right: item.rcItem.right - 2,
                bottom: item.rcItem.bottom - 2,
            };
            let _ = DrawFocusRect(item.hDC, &r);
        }
    }
}
unsafe fn capture_window(hwnd: HWND, file: &str) {
    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
    let w = (WIDTH * scale) as i32;
    let h = (HEIGHT * scale) as i32;
    let screen = GetDC(Some(hwnd));
    let dc = CreateCompatibleDC(Some(screen));
    let mut bits = std::ptr::null_mut();
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: 40,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    if let Ok(bitmap) = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        let old = SelectObject(dc, HGDIOBJ(bitmap.0));
        let _ = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(0));
        let length = (w * h * 4) as usize;
        let mut output = Vec::with_capacity(length + 54);
        output.extend_from_slice(b"BM");
        output.extend_from_slice(&((length + 54) as u32).to_le_bytes());
        output.extend_from_slice(&[0; 4]);
        output.extend_from_slice(&54u32.to_le_bytes());
        output.extend_from_slice(&40u32.to_le_bytes());
        output.extend_from_slice(&w.to_le_bytes());
        output.extend_from_slice(&(-h).to_le_bytes());
        output.extend_from_slice(&1u16.to_le_bytes());
        output.extend_from_slice(&32u16.to_le_bytes());
        output.extend_from_slice(&[0; 24]);
        output.extend_from_slice(std::slice::from_raw_parts(bits.cast::<u8>(), length));
        let _ = std::fs::write(file, output);
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    let _ = DeleteDC(dc);
    let _ = ReleaseDC(Some(hwnd), screen);
}

impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            self.clear_fonts();
        }
    }
}
