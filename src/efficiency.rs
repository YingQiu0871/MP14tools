//! Background-process "Efficiency Mode" (Windows 11 EcoQoS).
//!
//! Whatever is not in the foreground and owns a visible window is put into
//! efficiency mode - Windows then schedules it on the slower cores and lower
//! power states - and restored the moment it comes back to the front. This is
//! the same mechanism Task Manager's "efficiency mode" entry uses, and it is
//! what tools like EnergyStarX automate.
//!
//! Deliberately conservative: only processes with a visible window are
//! touched, shell/session-critical processes never are, and the user's ignore
//! list is always honoured. No administrator rights involved.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, ProcessPowerThrottling,
    QueryFullProcessImageNameW, SetProcessInformation,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::state::Shared;

/// Never throttled: the shell, session-critical processes and anything whose
/// slowdown would make the machine feel broken. Stored without the `.exe`.
const SYSTEM_PROCESSES: &[&str] = &[
    "explorer",
    "dwm",
    "csrss",
    "winlogon",
    "wininit",
    "services",
    "lsass",
    "smss",
    "svchost",
    "taskhostw",
    "sihost",
    "ctfmon",
    "textinputhost",
    "startmenuexperiencehost",
    "searchhost",
    "searchexperiencehost",
    "shellexperiencehost",
    "runtimebroker",
    "applicationframehost",
    "systemsettings",
    "widgetservice",
    "widgets",
    "mp14tools",
    "energystarx",
];

/// Pids currently in efficiency mode because of this module.
static THROTTLED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Start the watcher thread. Does nothing until the first sweep.
pub fn spawn(shared: Arc<Shared>) {
    let spawned = std::thread::Builder::new()
        .name("efficiency-watch".to_string())
        .spawn(move || run(shared));

    if let Err(error) = spawned {
        crate::log::line(&format!("efficiency: thread spawn failed: {error}"));
    }
}

fn run(shared: Arc<Shared>) {
    let mut active = false;
    loop {
        let (enabled, only_battery, interval_ms, ignore) = match shared.config.read() {
            Ok(config) => (
                config.efficiency.enabled,
                config.efficiency.only_on_battery,
                config.efficiency.interval_ms,
                config.efficiency.ignore.clone(),
            ),
            Err(_) => (false, false, 2000, Vec::new()),
        };

        std::thread::sleep(Duration::from_millis(interval_ms as u64));

        let on_battery = crate::battery::cached().map(|b| !b.on_ac).unwrap_or(false);
        if !enabled || (only_battery && !on_battery) {
            if active {
                active = false;
                let restored = clear_throttled();
                crate::log::line(&format!(
                    "efficiency: stopped, restored {restored} background process(es)"
                ));
            }
            shared.set_efficiency_count(0);
            continue;
        }

        let ignore: HashSet<String> = ignore.iter().map(|name| key_of(name)).collect();
        let count = sweep(&ignore);
        shared.set_efficiency_count(count);

        if !active {
            active = true;
            crate::log::line("efficiency: watching background processes");
        }
    }
}

/// One pass: throttle what should be throttled, restore what should not.
/// Returns how many processes are throttled afterwards.
fn sweep(ignore: &HashSet<String>) -> usize {
    let foreground = foreground_pid();
    let mut wanted: Vec<u32> = Vec::new();

    for pid in window_processes() {
        if Some(pid) == foreground {
            continue;
        }
        let Some(name) = process_name(pid) else {
            continue;
        };
        let key = key_of(&name);
        if ignore.contains(&key) || SYSTEM_PROCESSES.contains(&key.as_str()) {
            continue;
        }
        wanted.push(pid);
    }

    let mut applied = Vec::new();
    for pid in &wanted {
        if set_throttle(*pid, true) {
            applied.push(*pid);
        }
    }

    // Restore whatever was throttled last round and is not wanted any more.
    if let Ok(mut throttled) = THROTTLED.lock() {
        let previous = std::mem::replace(&mut *throttled, applied);
        for pid in previous {
            if !throttled.contains(&pid) {
                set_throttle(pid, false);
            }
        }
        throttled.len()
    } else {
        wanted.len()
    }
}

/// Restore every process this module throttled (feature switched off, or the
/// program is exiting). Returns how many were restored.
pub fn clear_throttled() -> usize {
    let mut restored = 0;
    if let Ok(mut throttled) = THROTTLED.lock() {
        for pid in throttled.drain(..) {
            set_throttle(pid, false);
            restored += 1;
        }
    }
    restored
}

/// Turn efficiency mode on or off for one process. Silently `false` when the
/// process cannot be opened (exited, elevated, protected).
fn set_throttle(pid: u32, on: bool) -> bool {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) else {
            return false;
        };
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if on {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };
        let result = SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &state as *const PROCESS_POWER_THROTTLING_STATE as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
        let _ = CloseHandle(handle);
        result.is_ok()
    }
}

fn foreground_pid() -> Option<u32> {
    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// Pids of every process that owns a visible top-level window.
fn window_processes() -> Vec<u32> {
    let mut pids: Vec<u32> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_window_pid),
            LPARAM((&mut pids as *mut Vec<u32>) as isize),
        );
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

unsafe extern "system" fn collect_window_pid(window: HWND, lparam: LPARAM) -> BOOL {
    let pids = unsafe { &mut *(lparam.0 as *mut Vec<u32>) };
    if unsafe { IsWindowVisible(window).as_bool() } {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid != 0 {
            pids.push(pid);
        }
    }
    BOOL(1)
}

/// File name of a process, lower-case, or `None` when it cannot be queried.
fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 512];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        let name = path.rsplit(['\\', '/']).next()?;
        (!name.is_empty()).then(|| name.to_string())
    }
}

/// Comparison key: lower-case, `.exe` stripped.
fn key_of(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .map(str::to_string)
        .unwrap_or(lower)
}
