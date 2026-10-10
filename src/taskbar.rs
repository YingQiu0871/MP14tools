//! Taskbar overlay: CPU load, network throughput and battery power, drawn on a
//! transparent strip of the taskbar.
//!
//! The strip either hugs the system tray (default) or starts after the weather
//! widget; both anchors are looked up from the live taskbar each tick, so the
//! overlay survives taskbar rearrangements.
//!
//! The window is layered with per-pixel alpha: the text is rendered white into
//! a 32-bit DIB, the luminance of every pixel becomes its alpha channel and the
//! colour is then tinted white or black to match the taskbar theme. It never
//! activates, never shows in Alt-Tab and lets clicks through.
//!
//! The strip is a *child* of the taskbar window (`Shell_TrayWnd`), the way
//! TrafficMonitor does it: a child is composited together with the taskbar, so
//! it stays visible while the Start menu, search or widgets are open (an
//! ordinary topmost window is covered by the shell's own z-band then). A hidden
//! host window owns the timer, the foreground hook and the "TaskbarCreated"
//! broadcast, because the child dies with Explorer and has to be rebuilt. If
//! the child cannot be created, the old topmost popup is used as a fallback.

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::BOOL;
use windows::Win32::Foundation::{
    COLORREF, FILETIME, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    BLACKNESS, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FF_DONTCARE, FW_SEMIBOLD,
    GetTextExtentPoint32W, GdiFlush, HDC, HGDIOBJ, HFONT, OUT_DEFAULT_PRECIS, PatBlt, SelectObject,
    MapWindowPoints, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Threading::GetSystemTimes;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, HWINEVENTHOOK, IUIAutomation, SetWinEventHook, TreeScope_Descendants,
    UIA_AutomationIdPropertyId,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW,
    FindWindowW, GWLP_USERDATA, GW_CHILD, GetClassNameW, GetForegroundWindow, GetMessageW,
    GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HWND_TOP, HWND_TOPMOST, IsWindow,
    IsWindowVisible, KillTimer, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetTimer, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_DESTROY,
    WM_NCHITTEST, WM_TIMER, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::config::{TaskbarConfig, TaskbarPosition};
use crate::state::Shared;

/// Timer that drives the refresh.
const TIMER_ID: usize = 0x4D02;
/// The weather widget's right edge in its compact form, as a multiple of the
/// taskbar height (measured: about 1.1). Hovering expands the widget to roughly
/// three times that; the placement only accounts for the compact form, which is
/// what is normally on screen.
const WEATHER_RIGHT_RATIO: f32 = 1.2;
/// Free space kept between the overlay and whatever it is anchored to.
const GAP_LOGICAL: f32 = 8.0;
/// How long the popup fallback runs before a child window is tried again.
const POPUP_RETRY: Duration = Duration::from_secs(60);
/// How often the Start button and the weather widget are looked up again
/// through UI Automation; the Start button moves as apps open and close.
const LAYOUT_REFRESH: Duration = Duration::from_secs(2);
/// A taskbar shorter than this is considered auto-hidden.
const HIDDEN_TASKBAR_HEIGHT: i32 = 8;

/// `EVENT_SYSTEM_FOREGROUND`: fires when the foreground window changes.
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
/// `WINEVENT_SKIPOWNPROCESS`: ignore focus changes among our own windows.
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;
/// Posted to the host window when any other window takes the foreground.
const WM_FOREGROUND_CHANGED: u32 = WM_APP + 1;

/// Class of the hidden host window (timer, hook and Explorer-restart receiver).
const HOST_CLASS: &str = "MP14Tools.TaskbarHost";
/// Class of the visible strip.
const OVERLAY_CLASS: &str = "MP14Tools.TaskbarOverlay";

/// Host window handle for the event hook: the callback carries no HWND of
/// ours, so it posts to this one. Zero until the window exists.
static OVERLAY_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Id of the "TaskbarCreated" broadcast Explorer sends after (re)starting.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// How the strip is attached to the screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Child of `Shell_TrayWnd`: composited with the taskbar.
    Child,
    /// Top-level topmost popup: the fallback when a child cannot be created.
    Popup,
}

/// Start the overlay thread. Does nothing visible until the first tick.
pub fn spawn(shared: Arc<Shared>) {
    let spawned = std::thread::Builder::new()
        .name("taskbar-overlay".to_string())
        .spawn(move || run(shared));

    if let Err(error) = spawned {
        crate::log::line(&format!("taskbar: thread spawn failed: {error}"));
    }
}

/// Everything the window procedure needs, owned by the overlay thread.
struct State {
    shared: Arc<Shared>,
    /// Hidden host window; owns the timer and this state.
    host: HWND,
    /// The visible strip, once created.
    overlay: Option<HWND>,
    mode: Mode,
    /// Taskbar window the strip is currently a child of (child mode).
    parent: HWND,
    /// The child strip has been drawn successfully at least once.
    child_ok: bool,
    /// When the popup fallback was entered; a child is tried again later.
    popup_since: Instant,
    font: Option<HFONT>,
    font_key: (u32, i32),
    mem_dc: Option<HDC>,
    bitmap: Option<HGDIOBJ>,
    bits: *mut u8,
    width: i32,
    height: i32,
    visible: bool,
    /// A snip overlay is on screen and the overlay stepped aside for it.
    capturing: bool,
    interval_ms: u32,
    light_taskbar: bool,
    theme_checked: Instant,
    metrics: Metrics,
}

/// Where the Start button and the weather widget sit on the taskbar.
///
/// On Windows 11 both are drawn by the taskbar's XAML island and have no
/// window of their own (the legacy `Start` child window is missing or does not
/// match the button), so they are found through UI Automation by their
/// automation ids. UI Automation must not be called from a thread that owns
/// windows in the tree it walks - the strip is a child of the taskbar - so a
/// separate worker thread does the lookup every [`LAYOUT_REFRESH`] and this
/// only reads its latest result.
#[derive(Clone, Copy, Default)]
struct Layout {
    /// Screen rectangle of the Start button.
    start: Option<RECT>,
    /// Screen rectangle of the weather widget.
    widgets: Option<RECT>,
}

/// Latest [`Layout`] published by the lookup thread.
static LAYOUT: std::sync::Mutex<Layout> = std::sync::Mutex::new(Layout {
    start: None,
    widgets: None,
});

impl Layout {
    /// The latest lookup, with the legacy `Start` child window as the fallback
    /// on older shells.
    fn current(taskbar: HWND) -> Self {
        let mut layout = LAYOUT.lock().map(|layout| *layout).unwrap_or_default();
        if layout.start.is_none() {
            layout.start = child_rect(taskbar, "Start").filter(|rect| rect.right > rect.left);
        }
        layout
    }

    /// Start the lookup thread (multithreaded apartment, owns no windows).
    fn spawn_lookup() {
        let spawned = std::thread::Builder::new()
            .name("taskbar-layout".to_string())
            .spawn(|| unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let automation: IUIAutomation =
                    match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                        Ok(automation) => automation,
                        Err(error) => {
                            crate::log::line(&format!(
                                "taskbar: UI Automation unavailable ({error}), using estimates"
                            ));
                            return;
                        }
                    };
                loop {
                    let layout = Self::lookup(&automation);
                    if let Ok(mut shared) = LAYOUT.lock() {
                        *shared = layout;
                    }
                    std::thread::sleep(LAYOUT_REFRESH);
                }
            });
        if let Err(error) = spawned {
            crate::log::line(&format!("taskbar: layout thread spawn failed: {error}"));
        }
    }

    unsafe fn lookup(automation: &IUIAutomation) -> Self {
        let mut layout = Self::default();
        let Ok(taskbar) = (unsafe { FindWindowW(windows::core::w!("Shell_TrayWnd"), None) }) else {
            return layout;
        };
        let Ok(root) = (unsafe { automation.ElementFromHandle(taskbar) }) else {
            return layout;
        };
        let find = |id: &str| -> Option<RECT> {
            unsafe {
                let value = VARIANT::from(windows::core::BSTR::from(id));
                let condition = automation
                    .CreatePropertyCondition(UIA_AutomationIdPropertyId, &value)
                    .ok()?;
                let element = root.FindFirst(TreeScope_Descendants, &condition).ok()?;
                let rect = element.CurrentBoundingRectangle().ok()?;
                (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
            }
        };
        layout.start = find("StartButton");
        layout.widgets = find("WidgetsButton");
        layout
    }
}

struct Metrics {
    /// idle and total (kernel+user) processor times, 100 ns units.
    last_cpu: Option<(u64, u64)>,
    cpu_percent: f32,
    /// received and sent octets plus when they were sampled.
    last_net: Option<(u64, u64, Instant)>,
    net_in: f64,
    net_out: f64,
}

impl Metrics {
    fn new() -> Self {
        Self {
            last_cpu: None,
            cpu_percent: 0.0,
            last_net: None,
            net_in: 0.0,
            net_out: 0.0,
        }
    }

    fn sample(&mut self) {
        self.sample_cpu();
        self.sample_net();
    }

    fn sample_cpu(&mut self) {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
            return;
        }

        let to_u64 = |time: &FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
        let idle = to_u64(&idle);
        let total = to_u64(&kernel) + to_u64(&user);

        if let Some((last_idle, last_total)) = self.last_cpu {
            let delta_total = total.saturating_sub(last_total);
            let delta_idle = idle.saturating_sub(last_idle);
            if delta_total > 0 {
                let busy = 1.0 - delta_idle as f32 / delta_total as f32;
                // A little smoothing: raw per-second samples jitter visibly.
                self.cpu_percent = self.cpu_percent * 0.4 + busy.clamp(0.0, 1.0) * 0.6;
            }
        }
        self.last_cpu = Some((idle, total));
    }

    fn sample_net(&mut self) {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let mut received = 0u64;
        let mut sent = 0u64;

        unsafe {
            if GetIfTable2(&mut table).is_ok() && !table.is_null() {
                let rows = std::slice::from_raw_parts(
                    (*table).Table.as_ptr(),
                    (*table).NumEntries as usize,
                );
                for row in rows {
                    if row.Type == windows::Win32::NetworkManagement::IpHelper::IF_TYPE_SOFTWARE_LOOPBACK {
                        continue;
                    }
                    if row.OperStatus != IfOperStatusUp {
                        continue;
                    }
                    received = received.saturating_add(row.InOctets);
                    sent = sent.saturating_add(row.OutOctets);
                }
                FreeMibTable(table as *const core::ffi::c_void);
            }
        }

        if let Some((last_in, last_out, at)) = self.last_net {
            let seconds = at.elapsed().as_secs_f64();
            if seconds > 0.2 {
                self.net_in = received.saturating_sub(last_in) as f64 / seconds;
                self.net_out = sent.saturating_sub(last_out) as f64 / seconds;
            }
        }
        self.last_net = Some((received, sent, Instant::now()));
    }
}

fn run(shared: Arc<Shared>) {
    unsafe {
        Layout::spawn_lookup();

        let host_class = crate::win::wide(HOST_CLASS);
        let overlay_class = crate::win::wide(OVERLAY_CLASS);
        let host_registered = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: crate::win::module_instance(),
            lpszClassName: crate::win::pcw(&host_class),
            ..Default::default()
        });
        let overlay_registered = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: crate::win::module_instance(),
            lpszClassName: crate::win::pcw(&overlay_class),
            ..Default::default()
        });
        if host_registered == 0 || overlay_registered == 0 {
            crate::log::line("taskbar: RegisterClassW failed");
            return;
        }

        // A plain top-level window that is never shown: only top-level windows
        // receive the "TaskbarCreated" broadcast.
        let host = match CreateWindowExW(
            Default::default(),
            crate::win::pcw(&host_class),
            crate::win::pcw(&host_class),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(crate::win::module_instance()),
            None,
        ) {
            Ok(window) => window,
            Err(error) => {
                crate::log::line(&format!("taskbar: CreateWindowExW failed: {error}"));
                return;
            }
        };

        let interval = shared
            .config
            .read()
            .map(|config| config.taskbar.update_ms)
            .unwrap_or(1000);

        let created = crate::win::wide("TaskbarCreated");
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(crate::win::pcw(&created)),
            Ordering::Relaxed,
        );

        // The snip overlay takes the foreground the moment it appears. Reacting
        // to that event (rather than to the next one-second tick) keeps the
        // overlay from flashing on the capture screen at all.
        OVERLAY_WINDOW.store(host.0 as isize, Ordering::Relaxed);
        let _ = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_SKIPOWNPROCESS,
        );

        let state = Box::into_raw(Box::new(State {
            shared,
            host,
            overlay: None,
            mode: Mode::Child,
            parent: HWND(std::ptr::null_mut()),
            child_ok: false,
            popup_since: Instant::now(),
            font: None,
            font_key: (0, 0),
            mem_dc: None,
            bitmap: None,
            bits: std::ptr::null_mut(),
            width: 0,
            height: 0,
            visible: false,
            capturing: false,
            interval_ms: interval,
            light_taskbar: false,
            theme_checked: Instant::now() - Duration::from_secs(10),
            metrics: Metrics::new(),
        }));
        SetWindowLongPtrW(host, GWLP_USERDATA, state as isize);
        SetTimer(Some(host), TIMER_ID, interval, None);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe fn state_of(window: HWND) -> Option<&'static mut State> {
    let raw = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut State;
    unsafe { raw.as_mut() }
}

/// `EVENT_SYSTEM_FOREGROUND` hook: nudge the overlay thread to re-evaluate.
///
/// Runs on the overlay thread's message loop (out-of-context hook), so it only
/// posts a message and returns immediately.
unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let target = OVERLAY_WINDOW.load(Ordering::Relaxed);
    if target != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(target as *mut core::ffi::c_void)),
                WM_FOREGROUND_CHANGED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Window procedure of the hidden host window.
///
/// The strip is a child of another process's window and shares its input
/// queue, so everything here has to stay quick and must never block.
unsafe extern "system" fn host_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar_created != 0 && message == taskbar_created {
        // Explorer (re)started: the old strip died with the old taskbar. The
        // tick notices that and builds a new one. A popup fallback chosen while
        // the shell was half up gets another chance at the child mode.
        if let Some(state) = unsafe { state_of(window) } {
            retry_child(state);
            tick(state);
        }
        return LRESULT(0);
    }

    match message {
        WM_TIMER => {
            if let Some(state) = unsafe { state_of(window) } {
                tick(state);
            }
            LRESULT(0)
        }
        WM_FOREGROUND_CHANGED => {
            // Cheap to run on every focus change: one layered-window update at
            // most, and it is what makes the snip reaction immediate.
            if let Some(state) = unsafe { state_of(window) } {
                tick(state);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            OVERLAY_WINDOW.store(0, Ordering::Relaxed);
            if let Some(state) = unsafe { state_of(window) } {
                unsafe {
                    // Take the strip off the taskbar before the host goes.
                    destroy_overlay(state);
                    release_gdi(state);
                    drop(Box::from_raw(state));
                }
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            }
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Window procedure of the strip itself: it only has to be click-through.
unsafe extern "system" fn overlay_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // HTTRANSPARENT: let the taskbar underneath receive the mouse.
        WM_NCHITTEST => LRESULT(-1),
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn release_gdi(state: &mut State) {
    unsafe {
        // The DC first: DeleteObject refuses to free a bitmap that is still
        // selected into a DC, so the bitmap only becomes deletable once the
        // memory DC is gone.
        if let Some(dc) = state.mem_dc.take() {
            let _ = DeleteDC(dc);
        }
        if let Some(bitmap) = state.bitmap.take() {
            let _ = DeleteObject(bitmap);
        }
        if let Some(font) = state.font.take() {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
        state.bits = std::ptr::null_mut();
    }
}

/// Destroy the strip window, if there is one, and forget it.
unsafe fn destroy_overlay(state: &mut State) {
    if let Some(window) = state.overlay.take() {
        unsafe {
            if IsWindow(Some(window)).as_bool() {
                let _ = DestroyWindow(window);
            }
        }
    }
    state.visible = false;
}

/// Leave the popup fallback: the next tick tries a child window again.
fn retry_child(state: &mut State) {
    if state.mode == Mode::Popup {
        crate::log::line("taskbar: trying the child window again");
        unsafe { destroy_overlay(state) };
        state.mode = Mode::Child;
        state.child_ok = false;
    }
}

/// Make sure a live strip window exists for the current taskbar and return it.
///
/// Child mode re-parents implicitly by recreating: when `Shell_TrayWnd` is a
/// different window than before (Explorer restarted) or the strip is gone, a
/// new child is created. If a child cannot be created at all, the strip falls
/// back to a topmost popup for good.
fn ensure_overlay(state: &mut State, taskbar: HWND) -> Option<HWND> {
    if state.mode == Mode::Popup && state.popup_since.elapsed() >= POPUP_RETRY {
        retry_child(state);
    }
    unsafe {
        if let Some(window) = state.overlay {
            let alive = IsWindow(Some(window)).as_bool();
            if alive && (state.mode == Mode::Popup || state.parent == taskbar) {
                return Some(window);
            }
            crate::log::line("taskbar: recreating overlay window");
            destroy_overlay(state);
        }

        let class = crate::win::wide(OVERLAY_CLASS);
        if state.mode == Mode::Child {
            match CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                crate::win::pcw(&class),
                crate::win::pcw(&class),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                10,
                10,
                Some(taskbar),
                None,
                Some(crate::win::module_instance()),
                None,
            ) {
                Ok(window) => {
                    state.overlay = Some(window);
                    state.parent = taskbar;
                    return Some(window);
                }
                Err(error) => {
                    crate::log::line(&format!(
                        "taskbar: child window failed ({error}), using topmost popup"
                    ));
                    state.mode = Mode::Popup;
                    state.popup_since = Instant::now();
                }
            }
        }

        match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            crate::win::pcw(&class),
            crate::win::pcw(&class),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(crate::win::module_instance()),
            None,
        ) {
            Ok(window) => {
                // Keep the popup out of every screen capture (PrintScreen, snip
                // tools, recorders). Only top-level windows support this; older
                // Windows builds reject the flag and the popup behaves as before.
                let _ = SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
                state.overlay = Some(window);
                Some(window)
            }
            Err(error) => {
                crate::log::line(&format!("taskbar: popup window failed: {error}"));
                None
            }
        }
    }
}

fn tick(state: &mut State) {
    let host = state.host;
    let config = match state.shared.config.read() {
        Ok(config) => config.taskbar.clone(),
        Err(_) => return,
    };

    // The refresh interval is configurable; re-arm the timer when it changes.
    if config.update_ms != state.interval_ms {
        state.interval_ms = config.update_ms;
        unsafe {
            let _ = KillTimer(Some(host), TIMER_ID);
            SetTimer(Some(host), TIMER_ID, config.update_ms, None);
        }
    }

    if !config.enabled {
        state.capturing = false;
        hide(state);
        return;
    }

    // While the snip overlay is up the shell reshuffles the taskbar under it;
    // stand still (keep the last position) and stay out of the shot entirely.
    // The child strip is not excluded from captures (that flag only works on
    // top-level windows), so this is also what keeps it out of the shot.
    //
    // Only the popup fallback needs to dodge the Start menu and friends:
    // Windows stops compositing ordinary topmost windows over the taskbar's
    // left region while a shell flyout is open, whereas a child of the taskbar
    // is composited with it and stays visible.
    let snip = capture_overlay_active();
    let shell_menu = state.mode == Mode::Popup && shell_menu_active();
    if snip || shell_menu {
        if !state.capturing {
            state.capturing = true;
            crate::log::line(if snip {
                "taskbar: snip overlay detected, hiding"
            } else {
                "taskbar: shell menu open, hiding"
            });
        }
        hide(state);
        return;
    }
    if state.capturing {
        state.capturing = false;
        crate::log::line("taskbar: overlay visible again");
    }

    let taskbar = match unsafe { FindWindowW(windows::core::w!("Shell_TrayWnd"), None) } {
        Ok(taskbar) => taskbar,
        Err(_) => {
            hide(state);
            return;
        }
    };

    let mut taskbar_rect = RECT::default();
    if unsafe { GetWindowRect(taskbar, &mut taskbar_rect) }.is_err() {
        hide(state);
        return;
    }
    let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
    if taskbar_height < HIDDEN_TASKBAR_HEIGHT {
        hide(state);
        return;
    }

    let dpi = unsafe { GetDpiForWindow(taskbar) };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };

    if state.theme_checked.elapsed() >= Duration::from_secs(2) {
        state.light_taskbar = system_uses_light_taskbar();
        state.theme_checked = Instant::now();
    }

    state.metrics.sample();
    let text = compose(&config, &state.metrics);

    if !unsafe { render(state, &text, dpi, &config) } {
        hide(state);
        return;
    }

    let Some(window) = ensure_overlay(state, taskbar) else {
        return;
    };

    // Horizontal anchor and vertical centring (plus the fine-tune nudges), in
    // screen coordinates.
    let layout = Layout::current(taskbar);
    let x = anchor_x(&config, taskbar, taskbar_rect, &layout, state.width, scale);
    let y = taskbar_rect.top
        + (taskbar_height - state.height) / 2
        + (config.offset_y as f32 * scale) as i32;

    let size = SIZE {
        cx: state.width,
        cy: state.height,
    };
    let source = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    unsafe {
        let screen = windows::Win32::Graphics::Gdi::GetDC(None);
        let result = if state.mode == Mode::Child {
            // A child is positioned in its parent's client coordinates, and
            // UpdateLayeredWindow's destination point is unreliable for it:
            // place it first, then only update the contents. It has to stay the
            // topmost sibling, above the XAML island that paints the Windows 11
            // taskbar; the z-order is only touched when something got above it.
            let mut origin = [POINT { x, y }];
            MapWindowPoints(None, Some(taskbar), &mut origin);
            let on_top = GetWindow(taskbar, GW_CHILD).ok() == Some(window);
            let flags = if on_top {
                SWP_NOACTIVATE | SWP_NOZORDER
            } else {
                SWP_NOACTIVATE
            };
            let _ = SetWindowPos(
                window,
                Some(HWND_TOP),
                origin[0].x,
                origin[0].y,
                state.width,
                state.height,
                flags,
            );
            UpdateLayeredWindow(
                window,
                Some(screen),
                None,
                Some(&size),
                state.mem_dc,
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        } else {
            let point = POINT { x, y };
            UpdateLayeredWindow(
                window,
                Some(screen),
                Some(&point),
                Some(&size),
                state.mem_dc,
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        };
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen);

        if result.is_err() {
            if state.mode == Mode::Child && !state.child_ok {
                // The child never drew once: this shell does not take layered
                // children. Fall back to the popup on the next tick.
                crate::log::line("taskbar: layered child update failed, using topmost popup");
                destroy_overlay(state);
                state.mode = Mode::Popup;
                state.popup_since = Instant::now();
            }
            return;
        }

        if state.mode == Mode::Popup {
            // Re-assert topmost every tick: the shell activates the taskbar on
            // lock/unlock and similar events, which pushes a never-activated
            // topmost window behind it. No move, no size, no activation.
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        } else {
            state.child_ok = true;
        }

        if !state.visible {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            state.visible = true;
        }
    }
}

/// Left edge of the overlay, from the configured anchor.
///
/// The anchors come from the live taskbar:
/// * `Auto` (default) centres the line in the gap between the weather widget
///   and the Start button; when the gap is too narrow it slides right up to
///   (but never over) the Start button.
/// * `Widget` starts right after the weather widget, with no Start-button
///   clamp.
/// * `Tray` right-aligns against the notification area, which does not move
///   when the task list grows (the list can still reach into it).
///
/// The weather widget's right edge comes from UI Automation when available,
/// otherwise from [`WEATHER_RIGHT_RATIO`].
fn anchor_x(
    config: &TaskbarConfig,
    taskbar: HWND,
    taskbar_rect: RECT,
    layout: &Layout,
    width: i32,
    scale: f32,
) -> i32 {
    let base_gap = (GAP_LOGICAL * scale) as i32;
    let offset = (config.offset_x as f32 * scale) as i32;
    let gap = base_gap + offset;
    let margin = (4.0 * scale) as i32;
    let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
    let estimate = taskbar_rect.left + (taskbar_height as f32 * WEATHER_RIGHT_RATIO) as i32;
    // Only trust a widget found on the left half: with a left-aligned taskbar
    // the weather moves next to the tray.
    let middle = (taskbar_rect.left + taskbar_rect.right) / 2;
    let widget_right = match layout.widgets {
        Some(widgets) if widgets.right < middle => widgets.right,
        _ => estimate,
    };
    let widget_x = widget_right + gap;

    let tray_x = || match child_rect(taskbar, "TrayNotifyWnd") {
        Some(tray) => tray.left - width - gap,
        // No notification area (unusual shell); hug the taskbar's right edge.
        None => taskbar_rect.right - width - gap,
    };

    let x = match config.position {
        TaskbarPosition::Widget => widget_x,
        // A Start button left of the widget means a left-aligned taskbar:
        // there is no gap to centre in, so just follow the widget.
        TaskbarPosition::Auto => match layout.start.filter(|start| start.left > widget_right) {
            Some(start) => {
                // The fine-tune offset shifts the result right (positive) or
                // left, the same way as for the other anchors.
                let span = start.left - widget_right;
                if span >= width + 2 * base_gap {
                    // Equal room on both sides.
                    widget_right + (span - width) / 2 + offset
                } else {
                    // No room to centre: keep clear of the Start button.
                    start.left - base_gap - width + offset
                }
            }
            None => widget_x,
        },
        TaskbarPosition::Tray => tray_x(),
    };

    x.max(taskbar_rect.left + margin)
}

/// Screen rectangle of a direct child window of `parent`, by class name.
fn child_rect(parent: HWND, class: &str) -> Option<RECT> {
    let name = crate::win::wide(class);
    let child =
        unsafe { FindWindowExW(Some(parent), None, crate::win::pcw(&name), None) }.ok()?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(child, &mut rect) }.ok()?;
    Some(rect)
}

/// True while a snip tool's selection overlay is on screen.
///
/// The Snipping Tool (and its predecessors) show a full-screen overlay window
/// while a snip is being taken; the shell rearranges the taskbar underneath,
/// which used to make the overlay jump around mid-capture.
///
/// The class is looked up by enumeration: `FindWindow` does not see these
/// WinUI windows (measured on this machine), `EnumWindows` does.
fn capture_overlay_active() -> bool {
    let mut found = false;
    unsafe {
        let _ = EnumWindows(
            Some(enum_capture_window),
            LPARAM((&mut found as *mut bool) as isize),
        );
    }
    found
}

/// True while a shell flyout (Start menu, widgets board, search) owns the
/// foreground.
///
/// Only used by the topmost-popup fallback: Windows stops compositing ordinary
/// topmost windows over the taskbar's left region while one of these is open,
/// so the popup steps aside instead of being silently blanked.
fn shell_menu_active() -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
    let Some(name) = crate::efficiency::process_name(pid) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "searchhost.exe"
            | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe"
            | "widgets.exe"
            | "widgetservice.exe"
            | "microsoftwindows.client.webexperience.exe"
    )
}

unsafe extern "system" fn enum_capture_window(window: HWND, lparam: LPARAM) -> BOOL {
    const CLASSES: [&str; 2] = ["SnipOverlayRootWindow", "ScreenClippingWindow"];

    let found = unsafe { &mut *(lparam.0 as *mut bool) };
    if *found {
        return BOOL(0);
    }

    if unsafe { IsWindowVisible(window).as_bool() } {
        let mut buffer = [0u16; 64];
        let length = unsafe { GetClassNameW(window, &mut buffer) };
        if length > 0 {
            let name = String::from_utf16_lossy(&buffer[..length as usize]);
            if CLASSES.contains(&name.as_str()) {
                *found = true;
                return BOOL(0);
            }
        }
    }

    BOOL(1)
}

fn hide(state: &mut State) {
    if state.visible {
        if let Some(window) = state.overlay {
            unsafe {
                let _ = ShowWindow(window, SW_HIDE);
            }
        }
        state.visible = false;
    }
}

/// Render `text` into the layered buffer. Returns false when nothing could be
/// drawn (no font or no DIB).
unsafe fn render(
    state: &mut State,
    text: &str,
    dpi: u32,
    config: &TaskbarConfig,
) -> bool {
    unsafe {
        ensure_font(state, dpi, config.font_size);
        let Some(font) = state.font else {
            return false;
        };

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut measured = SIZE::default();

        // Measure with the memory DC when it exists; otherwise a temporary one.
        let temp = if state.mem_dc.is_none() {
            let dc = CreateCompatibleDC(None);
            Some(dc)
        } else {
            None
        };
        let measure_dc = state.mem_dc.or(temp);
        let Some(measure_dc) = measure_dc else {
            return false;
        };

        let previous = SelectObject(measure_dc, HGDIOBJ(font.0));
        let _ = GetTextExtentPoint32W(measure_dc, &wide, &mut measured);
        SelectObject(measure_dc, previous);
        if let Some(dc) = temp {
            let _ = DeleteDC(dc);
        }

        if measured.cx <= 0 || measured.cy <= 0 {
            return false;
        }

        let pad_x = (6.0 * (dpi as f32 / 96.0)) as i32;
        let width = measured.cx + pad_x * 2;
        let height = measured.cy;

        if !ensure_buffer(state, width, height) {
            return false;
        }
        let Some(dc) = state.mem_dc else {
            return false;
        };

        let _ = PatBlt(dc, 0, 0, width, height, BLACKNESS);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));

        let previous = SelectObject(dc, HGDIOBJ(font.0));
        let mut bounds = RECT {
            left: pad_x,
            top: 0,
            right: width - pad_x,
            bottom: height,
        };
        DrawTextW(
            dc,
            &mut wide,
            &mut bounds,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, previous);

        // The luminance pass below reads the DIB's memory directly: make sure
        // the batched GDI drawing has actually landed there before reading.
        // (Without this the overlay can render as empty right after shell
        // events such as opening the Start menu.)
        let _ = GdiFlush();

        // White-on-black glyphs: the luminance is the coverage; tint it white
        // or black and turn it into premultiplied alpha.
        let count = (width * height) as usize;
        let pixels = std::slice::from_raw_parts_mut(state.bits as *mut u32, count);
        let white_text = !state.light_taskbar;
        for pixel in pixels.iter_mut() {
            let value = *pixel;
            let blue = value & 0xFF;
            let green = (value >> 8) & 0xFF;
            let red = (value >> 16) & 0xFF;
            let alpha = red.max(green).max(blue);
            *pixel = if white_text {
                (alpha << 24) | (alpha << 16) | (alpha << 8) | alpha
            } else {
                alpha << 24
            };
        }

        true
    }
}

unsafe fn ensure_font(state: &mut State, dpi: u32, font_size: f32) {
    let height = -((font_size * (dpi as f32 / 96.0)) as i32);
    let key = (dpi, height);
    if state.font.is_some() && state.font_key == key {
        return;
    }

    unsafe {
        if let Some(font) = state.font.take() {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
        let face = crate::win::wide("Microsoft YaHei UI");
        let font = CreateFontW(
            height,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 as u32) | (FF_DONTCARE.0 as u32),
            windows::core::PCWSTR(face.as_ptr()),
        );
        state.font = Some(font);
        state.font_key = key;
    }
}

unsafe fn ensure_buffer(state: &mut State, width: i32, height: i32) -> bool {
    if state.mem_dc.is_some() && state.width == width && state.height == height {
        return true;
    }

    unsafe {
        // Same ordering as release_gdi: the DC has to go before the bitmap it
        // has selected, otherwise DeleteObject silently fails and leaks it.
        if let Some(dc) = state.mem_dc.take() {
            let _ = DeleteDC(dc);
        }
        if let Some(bitmap) = state.bitmap.take() {
            let _ = DeleteObject(bitmap);
        }
        state.bits = std::ptr::null_mut();

        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            return false;
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => {
                SelectObject(dc, HGDIOBJ(bitmap.0));
                state.mem_dc = Some(dc);
                state.bitmap = Some(HGDIOBJ(bitmap.0));
                state.bits = bits as *mut u8;
                state.width = width;
                state.height = height;
                true
            }
            Err(_) => {
                let _ = DeleteDC(dc);
                false
            }
        }
    }
}

/// "CPU 23%   ↓1.2M ↑0.3M   12.3W"
fn compose(config: &TaskbarConfig, metrics: &Metrics) -> String {
    let mut parts: Vec<String> = Vec::new();

    if config.show_cpu {
        parts.push(format!("CPU {:.0}%", metrics.cpu_percent * 100.0));
    }
    if config.show_network {
        parts.push(format!(
            "↓{} ↑{}",
            format_speed(metrics.net_in),
            format_speed(metrics.net_out)
        ));
    }
    if config.show_power {
        parts.push(format_power());
    }

    parts.join("   ")
}

fn format_power() -> String {
    match crate::battery::cached() {
        Some(battery) => {
            let rate = battery.rate_mw.map(|rate| rate as f32 / 1000.0);
            if battery.discharging {
                match rate {
                    Some(watts) => format!("{watts:.1}W"),
                    None => "电池".to_string(),
                }
            } else if battery.charging {
                match rate {
                    Some(watts) => format!("+{watts:.1}W"),
                    None => "充电".to_string(),
                }
            } else {
                "AC".to_string()
            }
        }
        None => "AC".to_string(),
    }
}

fn format_speed(bytes_per_second: f64) -> String {
    const K: f64 = 1024.0;
    const M: f64 = 1024.0 * 1024.0;
    if bytes_per_second < K / 2.0 {
        "0".to_string()
    } else if bytes_per_second < M {
        format!("{:.0}K", bytes_per_second / K)
    } else if bytes_per_second < 10.0 * M {
        format!("{:.1}M", bytes_per_second / M)
    } else {
        format!("{:.0}M", bytes_per_second / M)
    }
}

/// The taskbar follows `SystemUsesLightTheme` rather than the app theme.
fn system_uses_light_taskbar() -> bool {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };

    unsafe {
        let mut key = HKEY::default();
        let path = crate::win::wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            crate::win::pcw(&path),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        ) != windows::Win32::Foundation::ERROR_SUCCESS
        {
            return false;
        }

        let name = crate::win::wide("SystemUsesLightTheme");
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = RegQueryValueExW(
            key,
            crate::win::pcw(&name),
            None,
            None,
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        status == windows::Win32::Foundation::ERROR_SUCCESS && value != 0
    }
}
