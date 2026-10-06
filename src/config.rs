//! Persistent configuration.
//!
//! Stored as JSON at `%LOCALAPPDATA%\MP14Tools\config.json`. Every struct is
//! `#[serde(default)]` so a partially written or older file still loads.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

/// What to send when a trigger fires: zero or more modifiers plus one target.
///
/// `target` is an id from [`crate::catalog`] - either a keyboard key (`"KeyA"`,
/// `"F5"`, `"VolumeUp"`) or a mouse button (`"MouseMiddle"`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Action {
    pub modifiers: Vec<String>,
    pub target: Option<String>,
}

impl Action {
    /// True when nothing would be sent.
    pub fn is_unset(&self) -> bool {
        self.target.is_none()
    }
}

/// Lightest pressure that can count as an intentional press.
pub const MIN_LIGHT_PRESS_THRESHOLD: u16 = 1;
/// Upper limit of the light-press threshold.
pub const MAX_LIGHT_PRESS_THRESHOLD: u16 = 500;
/// Upper limit of the deep-press threshold.
pub const MAX_DEEP_PRESS_THRESHOLD: u16 = 1000;
/// Factory light-press threshold of the reference machine.
pub const FACTORY_LIGHT_PRESS_THRESHOLD: u16 = 125;
/// Factory deep-press threshold of the reference machine.
pub const FACTORY_DEEP_PRESS_THRESHOLD: u16 = 500;

/// Clamp a pressure threshold into `min..=max`.
///
/// The thresholds are plain integers: the pressure in the HID report is an
/// integer, and rounding the user's number onto a fixed grid only made the typed
/// value disagree with what the touchpad actually reported.
pub fn clamp_threshold(value: u16, min: u16, max: u16) -> u16 {
    value.clamp(min, max)
}

/// Touchpad deep-press ("remap") settings.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct TouchpadConfig {
    pub enabled: bool,
    /// Keep both thresholds at the values the touchpad ships with, ignoring the
    /// two fields below. A switch rather than a one-off button: it has to stay
    /// true across restarts to be worth anything.
    pub factory_values: bool,
    /// Pressure at which a touch counts as an intentional press (raw HID units).
    /// Below it nothing is tracked at all, so lowering it makes the pad react to
    /// a softer touch.
    pub light_press_threshold: u16,
    /// Pressure that must be reached, and held for two frames, to fire.
    pub deep_press_threshold: u16,
    pub action: Action,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            factory_values: false,
            light_press_threshold: FACTORY_LIGHT_PRESS_THRESHOLD,
            deep_press_threshold: FACTORY_DEEP_PRESS_THRESHOLD,
            action: Action::default(),
        }
    }
}

impl TouchpadConfig {
    /// Clamp both thresholds into the supported range, keep the deep one above
    /// the light one, and honour the factory-value switch.
    pub fn normalize(&mut self) {
        if self.factory_values {
            self.light_press_threshold = FACTORY_LIGHT_PRESS_THRESHOLD;
            self.deep_press_threshold = FACTORY_DEEP_PRESS_THRESHOLD;
            return;
        }

        let light = clamp_threshold(
            self.light_press_threshold,
            MIN_LIGHT_PRESS_THRESHOLD,
            MAX_LIGHT_PRESS_THRESHOLD,
        );
        // One unit, not one grid step: a deep press at the same pressure as the
        // light one would fire on every touch.
        let deep = clamp_threshold(
            self.deep_press_threshold,
            light + 1,
            MAX_DEEP_PRESS_THRESHOLD,
        );

        self.light_press_threshold = light;
        self.deep_press_threshold = deep;
    }
}

/// Lowest haptic strength the firmware accepts.
///
/// The scale is not invented here: the touchpad takes each strength as a single
/// byte of the private HID payload, the vendor's own settings application caps
/// both values at 128, and every value it ever writes - 0, 56, 80, 104, 128 - is
/// a multiple of 8. That gives the window and the step below directly.
pub const MIN_HAPTIC_STRENGTH: u16 = 0;
/// Highest haptic strength the firmware accepts.
pub const MAX_HAPTIC_STRENGTH: u16 = 128;
/// Step of the strength slider, chosen so every documented value sits on a tick.
pub const HAPTIC_STRENGTH_STEP: u16 = 8;
/// Factory strength of the light ("normal") feedback - the value the touchpad
/// ships with, and therefore also the default here.
pub const DEFAULT_NORMAL_STRENGTH: u16 = 80;
/// Factory strength of the deep-press feedback.
pub const DEFAULT_DEEP_PRESS_STRENGTH: u16 = 104;

/// Snap a haptic strength onto [`HAPTIC_STRENGTH_STEP`] and clamp it.
pub fn snap_strength(value: u16) -> u16 {
    let step = HAPTIC_STRENGTH_STEP as u32;
    let snapped = (value as u32 + step / 2) / step * step;
    (snapped as u16).clamp(MIN_HAPTIC_STRENGTH, MAX_HAPTIC_STRENGTH)
}

/// Touchpad haptic ("vibration") strength.
///
/// The touchpad has a second, vendor-private HID collection that accepts the two
/// strengths the firmware uses: one for ordinary feedback, one for the deep
/// press. Writing to it is opt-in - until [`HapticsConfig::enabled`] is set the
/// tool stays exactly as read-only as it was before, and both values keep the
/// factory defaults, so switching the feature on changes nothing by itself.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct HapticsConfig {
    /// Allow writing the two strengths to the touchpad firmware.
    pub enabled: bool,
    /// Keep both strengths at the values the touchpad leaves the factory with.
    pub factory_values: bool,
    /// Strength of the light feedback (raw firmware units).
    pub normal_strength: u16,
    /// Strength of the deep-press feedback (raw firmware units).
    pub deep_press_strength: u16,
    /// Substring of the HID interface path that selects the private collection.
    /// Lower case, matched case-insensitively. Only needed when a machine exposes
    /// the touchpad under a different name; the log lists what it did find.
    pub device_marker: String,
}

impl Default for HapticsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            factory_values: true,
            normal_strength: DEFAULT_NORMAL_STRENGTH,
            deep_press_strength: DEFAULT_DEEP_PRESS_STRENGTH,
            device_marker: "hid#bltp7853&col05".to_string(),
        }
    }
}

impl HapticsConfig {
    /// Snap both strengths onto the step grid and inside the firmware window,
    /// keep the deep-press feedback at least as strong as the light one, and
    /// honour the factory-value switch.
    pub fn normalize(&mut self) {
        if self.factory_values {
            self.normal_strength = DEFAULT_NORMAL_STRENGTH;
            self.deep_press_strength = DEFAULT_DEEP_PRESS_STRENGTH;
        }

        let normal = snap_strength(self.normal_strength);
        let deep = snap_strength(self.deep_press_strength).max(normal);

        self.normal_strength = normal;
        self.deep_press_strength = deep;

        if self.device_marker.trim().is_empty() {
            self.device_marker = HapticsConfig::default().device_marker;
        }
    }
}

/// Log output options.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LogConfig {
    /// Show a console window with the log while the tool runs.
    pub console: bool,
    /// Where the log file goes. Empty means [`data_dir`].
    pub directory: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            console: false,
            directory: String::new(),
        }
    }
}

impl LogConfig {
    /// Directory the log actually goes to, with the default filled in.
    pub fn resolved_directory(&self) -> PathBuf {
        let trimmed = self.directory.trim();
        if trimmed.is_empty() {
            data_dir()
        } else {
            PathBuf::from(trimmed)
        }
    }
}

/// What to do when the machine switches to battery power.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BatteryAction {
    /// Leave the display alone.
    Off,
    /// Offer the switch and wait for the button to be clicked.
    #[default]
    Notify,
    /// Switch without asking, and say so afterwards.
    Force,
}

/// Target of an externally connected display while on battery.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExternalRate {
    /// Set the display to the highest mode it offers at the current resolution.
    #[serde(rename = "highest")]
    Highest,
    /// Set the display to 60 Hz.
    #[default]
    #[serde(rename = "60hz")]
    Hz60,
}

/// The two modes the built-in panel is offered as a battery target.
pub const INTERNAL_RATES: [u32; 2] = [60, 120];

/// Snap a hand-edited value onto the two rates the panel is offered at.
pub fn nearest_internal_rate(value: u32) -> u32 {
    if value <= INTERNAL_RATES[0] + 30 {
        INTERNAL_RATES[0]
    } else {
        INTERNAL_RATES[1]
    }
}

/// Battery refresh-rate switching and HDR checks (the "smart refresh rate"
/// behaviour of the reference machine's own tool, reimplemented here).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DisplayConfig {
    /// Master switch; while false nothing below runs.
    pub enabled: bool,
    /// What to do on the AC -> battery transition.
    pub battery_action: BatteryAction,
    /// Mode the built-in panel is switched to on battery: 60 or 120 Hz. The
    /// closest available mode not above it is used.
    pub internal_refresh_rate: u32,
    /// Mode externally connected displays are switched to on battery.
    pub external_refresh_rate: ExternalRate,
    /// Watch the built-in panel's HDR and offer to turn it off while on battery.
    pub hdr_check: bool,
    /// The built-in panel takes part.
    pub internal: bool,
    /// Externally connected displays take part.
    pub external: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            battery_action: BatteryAction::Notify,
            internal_refresh_rate: INTERNAL_RATES[0],
            external_refresh_rate: ExternalRate::Hz60,
            hdr_check: true,
            internal: true,
            external: true,
        }
    }
}

impl DisplayConfig {
    /// Keep a hand-edited file on the modes the panel is actually offered at.
    pub fn normalize(&mut self) {
        self.internal_refresh_rate = nearest_internal_rate(self.internal_refresh_rate);
    }
}

/// One power profile: what the display should look like while it is in effect,
/// plus an optional command to run on entry.
///
/// This is the part the battery-driven [`DisplayConfig`] cannot express - it
/// covers "plugged in", "on battery" and "battery saver is on" separately, and
/// it can turn HDR *on* again, not only off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Profile {
    /// Target refresh rate of the built-in panel, snapped to the rates the
    /// panel is offered at (60 or 120). `0` leaves the refresh rate alone.
    pub refresh: u32,
    /// HDR state to enforce: `Some(true)` turns it on, `Some(false)` turns it
    /// off, `None` leaves HDR alone in this profile.
    pub hdr: Option<bool>,
    /// Also apply the refresh rate to externally connected displays.
    pub external: bool,
    /// Optional command run on entering the profile, without a console window.
    ///
    /// This is the escape hatch for everything a no-admin tool cannot do
    /// itself, e.g. switching a Windows power scheme through a pre-created
    /// scheduled task: `schtasks /run /tn MP14Tools-EcoOn`.
    pub command: String,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            refresh: INTERNAL_RATES[0],
            hdr: Some(false),
            external: false,
            command: String::new(),
        }
    }
}

impl Profile {
    pub fn normalize(&mut self) {
        if self.refresh != 0 {
            self.refresh = nearest_internal_rate(self.refresh);
        }
        self.command = self.command.trim().to_string();
    }
}

/// Which switch decides between the three profiles.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileMode {
    /// Battery saver is the switch: saver on -> `eco`, otherwise `high` while
    /// plugged in and `medium` on battery (see
    /// [`ProfilesConfig::high_only_on_ac`]).
    #[default]
    BatterySaver,
    /// The power source is the switch: AC -> `high`, battery -> `eco` while
    /// battery saver is on, otherwise `medium`.
    AcDc,
}

/// Refresh-rate / HDR profiles, and the battery-saver switch that drives them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ProfilesConfig {
    /// Master switch. On by default in this fork - the profile switching is the
    /// feature it was forked for. Set it to false to keep the display untouched
    /// and use only the touchpad/hotkey side of the tool.
    pub enabled: bool,
    pub mode: ProfileMode,
    /// Plugged in, battery saver off: 120 Hz and HDR on by default.
    pub high: Profile,
    /// On battery, battery saver off: 60 Hz and HDR off by default.
    pub medium: Profile,
    /// Battery saver on: 60 Hz, HDR off, and whatever `command` does.
    pub eco: Profile,
    /// Only enter `high` while plugged in. Guards against running 120 Hz + HDR
    /// on battery just because battery saver happens to be off.
    pub high_only_on_ac: bool,
    /// How often the power source and the battery-saver flag are polled.
    /// `GetSystemPowerStatus` reads a handful of bytes, so a short interval is
    /// cheap; it is not worth registering for power-setting notifications.
    pub poll_ms: u64,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: ProfileMode::BatterySaver,
            high: Profile {
                refresh: INTERNAL_RATES[1],
                hdr: Some(true),
                external: false,
                command: String::new(),
            },
            medium: Profile::default(),
            eco: Profile::default(),
            high_only_on_ac: true,
            poll_ms: 2000,
        }
    }
}

impl ProfilesConfig {
    pub fn normalize(&mut self) {
        self.high.normalize();
        self.medium.normalize();
        self.eco.normalize();
        self.poll_ms = self.poll_ms.clamp(500, 60000);
    }

    /// The profile for the current state, with its name for the log and the UI.
    ///
    /// Only ever called with a state the machine actually reported: when the
    /// power source is unknown the profile thread skips instead of guessing,
    /// exactly like the original battery policy does.
    pub fn select(&self, on_ac: bool, saver_on: bool) -> (&Profile, &'static str) {
        match self.mode {
            ProfileMode::AcDc => {
                if on_ac {
                    (&self.high, "high")
                } else if saver_on {
                    (&self.eco, "eco")
                } else {
                    (&self.medium, "medium")
                }
            }
            ProfileMode::BatterySaver => {
                if saver_on {
                    (&self.eco, "eco")
                } else if on_ac || !self.high_only_on_ac {
                    (&self.high, "high")
                } else {
                    (&self.medium, "medium")
                }
            }
        }
    }
}

/// One OEM/vendor hotkey, identified by a HID report prefix from WMI.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OemKey {
    /// Shown in the settings window.
    pub name: String,
    pub enabled: bool,
    /// Hex prefix matched against the `EventDetail` byte array, e.g. `"01-28-01"`.
    pub report_hex: String,
    /// Only fire while the key is held (`Active == true`), not on release.
    pub press_only: bool,
    pub action: Action,
}

impl Default for OemKey {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            report_hex: String::new(),
            press_only: true,
            action: Action::default(),
        }
    }
}

impl OemKey {
    /// Normalised (upper case, separator-free) hex prefix used for matching.
    pub fn normalized_prefix(&self) -> String {
        normalize_hex(&self.report_hex)
    }
}

/// Strip `-`, spaces and case so `"01-28-01"` and `"012801"` compare equal.
pub fn normalize_hex(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

/// On-screen display preferences.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct OsdConfig {
    pub enabled: bool,
    /// How long the banner stays fully opaque. The fade-out that follows is a
    /// fixed two seconds and is not configurable.
    pub duration_ms: u32,
}

impl Default for OsdConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: OSD_HOLD_DEFAULT_MS,
        }
    }
}

/// Time the banner stays fully opaque before it starts fading.
pub const OSD_HOLD_DEFAULT_MS: u32 = 5_000;
/// Shortest and longest hold time a hand-edited file may ask for.
pub const OSD_HOLD_RANGE: (u32, u32) = (1_000, 60_000);

impl OsdConfig {
    /// Keep the hold time in a range that cannot leave a banner on screen
    /// forever.
    pub fn normalize(&mut self) {
        self.duration_ms = self
            .duration_ms
            .clamp(OSD_HOLD_RANGE.0, OSD_HOLD_RANGE.1);
    }
}

/// The Windows power-scheme settings the settings window's "省电" page applies.
///
/// These live in the active power plan and need administrator rights, so the
/// page only stores the values here and hands them to `mp14-eco-setup.ps1`
/// (elevated) when the user presses apply.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct EcoSetupConfig {
    /// Battery-side maximum processor state, in percent.
    pub cpu_max_percent: u16,
    /// Battery-side display brightness index, in percent.
    pub brightness_percent: u16,
    /// Seconds of inactivity before the display turns off on battery.
    pub screen_off_seconds: u32,
    /// Battery percentage at which Windows turns battery saver on.
    pub saver_threshold_percent: u16,
    /// Disable turbo boost while on battery.
    pub disable_turbo: bool,
    /// Set PCIe link state power management to maximum savings.
    pub max_pcie_aspm: bool,
    /// Set the wireless adapter's power saving mode to maximum.
    pub wifi_max_saving: bool,
    /// Helper script to run; empty means the copy next to the configuration
    /// file (which is where the installer puts it).
    pub script: String,
}

impl Default for EcoSetupConfig {
    fn default() -> Self {
        Self {
            cpu_max_percent: 50,
            brightness_percent: 40,
            screen_off_seconds: 60,
            saver_threshold_percent: 40,
            disable_turbo: true,
            max_pcie_aspm: true,
            wifi_max_saving: true,
            script: String::new(),
        }
    }
}

impl EcoSetupConfig {
    pub fn normalize(&mut self) {
        self.cpu_max_percent = self.cpu_max_percent.clamp(10, 100);
        self.brightness_percent = self.brightness_percent.clamp(0, 100);
        self.screen_off_seconds = self.screen_off_seconds.clamp(30, 3600);
        self.saver_threshold_percent = self.saver_threshold_percent.clamp(0, 100);
        self.script = self.script.trim().to_string();
    }

    /// Where the helper script is: the configured path, else the copy that
    /// lives next to the configuration file.
    pub fn resolved_script(&self) -> PathBuf {
        if self.script.is_empty() {
            data_dir().join("mp14-eco-setup.ps1")
        } else {
            PathBuf::from(self.script.clone())
        }
    }

    /// Where the helper script's output is captured, for the settings window.
    pub fn log_path() -> PathBuf {
        data_dir().join("eco-apply.log")
    }
}

/// Root configuration object.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    /// Log output options (console window and log file location).
    pub log: LogConfig,
    pub start_with_windows: bool,
    pub show_tray_icon: bool,
    pub osd: OsdConfig,
    pub touchpad: TouchpadConfig,
    pub haptics: HapticsConfig,
    pub display: DisplayConfig,
    /// Refresh-rate / HDR profiles, driven by the battery-saver switch.
    pub profiles: ProfilesConfig,
    /// Power-scheme values the "省电" page applies through the helper script.
    pub eco_setup: EcoSetupConfig,
    pub oem_keys: Vec<OemKey>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            log: LogConfig::default(),
            start_with_windows: false,
            show_tray_icon: true,
            osd: OsdConfig::default(),
            touchpad: TouchpadConfig::default(),
            haptics: HapticsConfig::default(),
            display: DisplayConfig::default(),
            profiles: ProfilesConfig::default(),
            eco_setup: EcoSetupConfig::default(),
            oem_keys: default_oem_keys(),
        }
    }
}

impl Config {
    /// Bring a freshly loaded configuration into the supported range.
    ///
    /// Runs after every load - from disk at startup and from the file watcher -
    /// so a hand-edited file can never push a threshold out of bounds.
    pub fn normalize(&mut self) {
        self.osd.normalize();
        self.touchpad.normalize();
        self.haptics.normalize();
        self.display.normalize();
        self.profiles.normalize();
        self.eco_setup.normalize();
    }
}

/// The two options that have to be known before the first log line, read with a
/// minimal schema instead of a full [`Config`] parse.
///
/// They cannot wait for the normal load: the console must exist before the first
/// `println!` (the standard handles are cached on first use), and the log file is
/// opened on the first line written.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct StartupPreview {
    log: LogConfig,
}

impl StartupPreview {
    pub fn console(&self) -> bool {
        self.log.console
    }

    pub fn log_directory(&self) -> PathBuf {
        self.log.resolved_directory()
    }
}

/// Read [`StartupPreview`] from the configuration file, falling back to defaults.
pub fn startup_preview() -> StartupPreview {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|text| serde_json::from_str::<StartupPreview>(&text).ok())
        .unwrap_or_default()
}

/// Vendor hotkeys of the reference machine (Xiaomi Book Pro 14 2026).
///
/// These report prefixes match `HID_EVENT20` events. They are only a starting
/// point - any OEM key can be added by copying a line and editing `report_hex`
/// (a prefix, so `"01-2B"` matches every report starting with those bytes).
fn default_oem_keys() -> Vec<OemKey> {
    fn key(name: &str, report_hex: &str) -> OemKey {
        OemKey {
            name: name.to_string(),
            enabled: true,
            report_hex: report_hex.to_string(),
            press_only: true,
            action: Action::default(),
        }
    }

    const FULL: &str = "-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00-00";

    vec![
        key("PC Manager", &format!("01-25-01{FULL}")),
        key("XiaoAi", &format!("01-23-01{FULL}")),
        key("Settings", &format!("01-1B{FULL}")),
        key("Projection", &format!("01-01{FULL}")),
        key("Performance mode (Fn+K)", "01-28-01"),
        key("Fn Lock", "01-07"),
        key("Caps Lock", "01-09"),
        key("Microphone mute (on)", &format!("01-21-00{FULL}")),
        key("Microphone unmute", &format!("01-21-01{FULL}")),
        key("Keyboard backlight", "01-05"),
    ]
}

/// `%LOCALAPPDATA%\MP14Tools`
pub fn data_dir() -> PathBuf {
    local_app_data().join("MP14Tools")
}

/// Directory used before the tool was renamed from MeowBox Lite.
fn legacy_data_dir() -> PathBuf {
    local_app_data().join("MeowBoxLite")
}

fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Take over the configuration written by the pre-rename build, once.
///
/// The data directory changed together with the product name, so without this
/// the first start after the rename would look like a fresh install and the user
/// would silently lose their mappings.
pub fn adopt_legacy_config() {
    let legacy = legacy_data_dir().join("config.json");
    if config_path().exists() || !legacy.exists() {
        return;
    }

    if fs::create_dir_all(data_dir()).is_err() {
        return;
    }

    match fs::copy(&legacy, config_path()) {
        Ok(_) => crate::log::line(&format!(
            "configuration adopted from {}",
            legacy.display()
        )),
        Err(error) => crate::log::line(&format!("could not adopt the old configuration: {error}")),
    }
}

/// Read the configuration, falling back to defaults on any error.
///
/// The offending file is left untouched so the user can fix it by hand.
pub fn load() -> Config {
    let mut config = match fs::read_to_string(config_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            crate::log::line(&format!("config parse failed, using defaults: {error}"));
            Config::default()
        }),
        Err(_) => Config::default(),
    };

    config.normalize();
    config
}

/// Write the configuration, creating the data directory when needed.
pub fn save(config: &Config) -> io::Result<()> {
    fs::create_dir_all(data_dir())?;
    let text = serde_json::to_string_pretty(config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(config_path(), text)
}
