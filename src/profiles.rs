//! Power profiles: refresh rate, HDR and an optional command per state.
//!
//! Windows switches the *power mode* by itself when the charger is plugged in,
//! but it never touches the panel: unplugging does not drop 120 Hz to 60 Hz, and
//! HDR stays on. This module fills that gap, and adds a third state on top -
//! "battery saver is on" - which is also the state that runs a command, so a
//! Windows power scheme can be swapped through a pre-created scheduled task
//! without this tool ever needing administrator rights (see
//! `tools/mp14-eco-setup.ps1`).
//!
//! The power source and the battery-saver flag are polled; `GetSystemPowerStatus`
//! reads a handful of bytes, and only a *change* of state writes to the display,
//! so a refresh rate the user changed by hand is left alone until the next real
//! transition.
//!
//! This is a separate subsystem from [`crate::power`] on purpose: that one is the
//! original battery policy, gated behind `display.enabled` (off by default), and
//! the two are not meant to run at the same time.

use std::os::windows::process::CommandExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Profile;
use crate::display;
use crate::state::Shared;

/// How long to wait before retrying a profile whose writes failed.
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// What is currently in effect, so a poll that changes nothing writes nothing.
struct Applied {
    name: &'static str,
    profile: Profile,
    /// Set when the last attempt failed: the earliest time to try again. A
    /// transient failure (a display that was busy during a mode change) then
    /// heals by itself instead of waiting for the next power transition.
    retry_at: Option<Instant>,
}

/// Start the profile thread. It idles until `profiles.enabled` is true.
pub fn spawn(shared: Arc<Shared>) {
    let spawned = std::thread::Builder::new()
        .name("profiles".to_string())
        .spawn(move || run(shared));

    if let Err(error) = spawned {
        crate::log::line(&format!("profiles: could not start the thread ({error})"));
    }
}

fn run(shared: Arc<Shared>) {
    let mut applied: Option<Applied> = None;
    let mut reported_missing = false;

    loop {
        // Cloned out of the lock on purpose: `apply` reads the configuration
        // again, and the config watcher may want the write lock meanwhile.
        let config = match shared.config.read() {
            Ok(config) => config.profiles.clone(),
            Err(_) => return,
        };

        let interval = Duration::from_millis(config.poll_ms);

        if !config.enabled {
            // Forgetting the state here is what makes re-enabling the feature
            // apply immediately instead of waiting for the next transition.
            applied = None;
            std::thread::sleep(interval);
            continue;
        }

        // A machine that cannot report a power source - a desktop, some VMs -
        // must not have its display switched at all: no profile is guessed for
        // it, matching what the original battery policy does.
        let battery = match display::on_battery() {
            Some(battery) => {
                // A source that comes back has to be able to warn again if it
                // disappears later on.
                reported_missing = false;
                battery
            }
            None => {
                if !reported_missing {
                    crate::log::line(
                        "profiles: this machine does not report a power source; the profiles stay off",
                    );
                    reported_missing = true;
                }
                applied = None;
                std::thread::sleep(interval);
                continue;
            }
        };

        let on_ac = !battery;
        let saver_on = display::battery_saver_on().unwrap_or(false);
        let (profile, name) = config.select(on_ac, saver_on);

        // Only a *real* profile change writes anything, command included.
        // Comparing the raw power state as well would re-run `command` on every
        // plug/unplug that leaves the profile unchanged - and since a command
        // may switch the very power scheme the battery-saver threshold lives in,
        // that is also how a ping-pong between two profiles would start.
        let changed = match &applied {
            Some(previous) => {
                previous.name != name
                    || previous.profile != *profile
                    || previous
                        .retry_at
                        .map(|at| Instant::now() >= at)
                        .unwrap_or(false)
            }
            None => true,
        };

        if changed {
            crate::log::line(&format!(
                "profiles: {name} (AC={on_ac}, battery saver={saver_on})"
            ));
            let written = apply(&shared, name, profile);
            applied = Some(Applied {
                name,
                profile: profile.clone(),
                // A write that failed is retried, but not on every poll: a
                // panel that refuses a mode would otherwise fill the log.
                retry_at: if written {
                    None
                } else {
                    Some(Instant::now() + RETRY_INTERVAL)
                },
            });
        }

        std::thread::sleep(interval);
    }
}

/// Write the profile's display state, then run its command.
///
/// Returns whether every write it attempted succeeded, so the caller can decide
/// to retry later.
fn apply(shared: &Arc<Shared>, name: &str, profile: &Profile) -> bool {
    if shared
        .config
        .read()
        .map(|config| config.display.enabled)
        .unwrap_or(false)
    {
        crate::log::line(
            "profiles: display.enabled is also on; both policies write the refresh rate",
        );
    }

    let mut written = true;
    let mut notes: Vec<String> = Vec::new();

    for screen in display::list() {
        if !screen.internal && !profile.external {
            continue;
        }

        if profile.refresh != 0 && screen.refresh != profile.refresh {
            // `rate_at_most` also covers the panel that cannot reach the
            // requested rate at all, instead of failing the whole profile.
            match display::rate_at_most(
                &screen.adapter,
                screen.width,
                screen.height,
                profile.refresh,
            ) {
                Some(target) if target != screen.refresh => {
                    match display::set_rate(&screen.adapter, target) {
                        Ok(()) => notes.push(format!("{} → {target} Hz", screen.label)),
                        Err(error) => {
                            written = false;
                            crate::log::line(&format!("profiles: {error}"));
                        }
                    }
                }
                None => {
                    // No mode list at all for this display: worth retrying.
                    written = false;
                }
                _ => {}
            }
        }

        if screen.hdr_supported {
            // `None` means "leave HDR alone in this profile".
            if let Some(want) = profile.hdr {
                if screen.hdr_enabled != want {
                    match display::set_hdr(&screen, want) {
                        Ok(()) => notes.push(format!(
                            "{} HDR {}",
                            screen.label,
                            if want { "开" } else { "关" }
                        )),
                        Err(error) => {
                            written = false;
                            crate::log::line(&format!("profiles: {error}"));
                        }
                    }
                }
            }
        }
    }

    let summary = if notes.is_empty() {
        format!("{name}：无需调整")
    } else {
        format!("{name}：{}", notes.join("，"))
    };

    crate::log::line(&format!("profiles: {summary}"));
    shared.set_display_status(summary.clone());

    if shared
        .config
        .read()
        .map(|config| config.osd.enabled)
        .unwrap_or(false)
    {
        shared.queue_osd(summary);
    }

    run_command(profile);
    written
}

/// Run the profile's command, if it has one, without a console window.
fn run_command(profile: &Profile) {
    let command = profile.command.trim();
    if command.is_empty() {
        return;
    }

    /// A child console window would flash on every transition, and this process
    /// has no console of its own to inherit.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    crate::log::line(&format!("profiles: running '{command}'"));
    // `raw_arg` on purpose: the value is a command *line* for `cmd /C`, and the
    // standard argument escaping would mangle one that contains quotes.
    let spawned = std::process::Command::new("cmd.exe")
        .arg("/C")
        .raw_arg(command)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();

    match spawned {
        Ok(mut child) => {
            // The exit code is the only feedback there is: `schtasks` refusing
            // to start a missing task would otherwise be invisible.
            std::thread::spawn(move || match child.wait() {
                Ok(status) if !status.success() => {
                    crate::log::line(&format!("profiles: command exited with {status}"));
                }
                Err(error) => {
                    crate::log::line(&format!("profiles: could not wait for the command ({error})"));
                }
                _ => {}
            });
        }
        Err(error) => crate::log::line(&format!("profiles: could not run the command ({error})")),
    }
}
