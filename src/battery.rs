//! Battery state, straight from the power manager.
//!
//! `CallNtPowerInformation(SystemBatteryState)` reports the pack's remaining
//! and full capacity plus the instantaneous rate of change - the discharge
//! power the "省电" side of the window shows. A short cache keeps the UI from
//! calling into powrprof on every frame.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::System::Power::{
    CallNtPowerInformation, SystemBatteryState, SYSTEM_BATTERY_STATE,
};

/// How long a reading is reused before asking the power manager again.
const CACHE: Duration = Duration::from_millis(1500);

/// One snapshot of the battery.
#[derive(Clone, Copy, Debug)]
pub struct Reading {
    /// Running from the wall adapter.
    pub on_ac: bool,
    pub charging: bool,
    pub discharging: bool,
    /// Charge percentage, when the battery reports a capacity.
    pub percent: Option<u8>,
    /// Rate of change in milliwatts (discharge power while on battery).
    pub rate_mw: Option<u32>,
    /// Remaining capacity in milliwatt-hours.
    pub remaining_mwh: Option<u32>,
    /// Full-charge capacity in milliwatt-hours.
    pub full_mwh: Option<u32>,
    /// Seconds of runtime the firmware estimates.
    pub estimated_seconds: Option<u32>,
}

static CACHE_SLOT: Mutex<Option<(Instant, Reading)>> = Mutex::new(None);

/// Latest reading, refreshed at most every [`CACHE`].
pub fn cached() -> Option<Reading> {
    let mut slot = CACHE_SLOT.lock().ok()?;
    if let Some((stamp, reading)) = *slot {
        if stamp.elapsed() < CACHE {
            return Some(reading);
        }
    }

    let reading = read()?;
    *slot = Some((Instant::now(), reading));
    Some(reading)
}

fn read() -> Option<Reading> {
    let mut state = SYSTEM_BATTERY_STATE::default();
    let status = unsafe {
        CallNtPowerInformation(
            SystemBatteryState,
            None,
            0,
            Some(&mut state as *mut SYSTEM_BATTERY_STATE as *mut core::ffi::c_void),
            std::mem::size_of::<SYSTEM_BATTERY_STATE>() as u32,
        )
    };
    if !status.is_ok() || !state.BatteryPresent {
        return None;
    }

    let percent = if state.MaxCapacity > 0 {
        Some(((state.RemainingCapacity as u64 * 100 / state.MaxCapacity as u64) as u8).min(100))
    } else {
        None
    };

    // The pack speaks in a *signed* rate although the field is documented as
    // unsigned: while discharging this firmware reports a negative number
    // (measured: 0xFFFFC204 = -16.3 W). Take the magnitude.
    let rate_mw = (state.Rate as i32).unsigned_abs();

    Some(Reading {
        on_ac: state.AcOnLine,
        charging: state.Charging,
        discharging: state.Discharging,
        percent,
        // A couple of milliwatts of noise (and the -1 "not ready" sentinel)
        // are not a reading; neither is anything above a quarter kilowatt.
        rate_mw: (rate_mw >= 10 && rate_mw <= 250_000).then_some(rate_mw),
        remaining_mwh: (state.RemainingCapacity > 0).then_some(state.RemainingCapacity),
        full_mwh: (state.MaxCapacity > 0).then_some(state.MaxCapacity),
        // A runtime estimate is only meaningful while discharging; the "not
        // ready" sentinel works out to more than a week of runtime.
        estimated_seconds: (state.Discharging
            && state.EstimatedTime > 0
            && state.EstimatedTime <= 7 * 24 * 3600)
        .then_some(state.EstimatedTime),
    })
}
