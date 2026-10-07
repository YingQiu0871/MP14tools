//! "省电体检": compare the power-scheme values actually stored on the machine
//! against the settings the window shows.
//!
//! Windows keeps per-scheme setting values in the registry, which is readable
//! without admin rights and - unlike `powercfg /query` - also exposes hidden
//! settings (EPP, core parking, cooling policy...). A value that is absent
//! means "scheme default", which is exactly what the check should flag.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
    REG_DWORD, REG_SZ,
};

use crate::config::EcoSetupConfig;

const BASE: &str = r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes";

const SUB_PROCESSOR: &str = "54533251-82be-4824-96c1-47b60b740d00";
const SUB_VIDEO: &str = "7516b95f-f776-4464-8c53-06167f40cc99";
const SUB_SLEEP: &str = "238c9fa8-0aad-41ed-83f4-97be242c8f20";
const SUB_PCIEXPRESS: &str = "501a4d13-42af-4429-9fd1-a8218c268e20";
const SUB_ENERGYSAVER: &str = "de830923-a562-41af-a086-e3a2c6bad2da";
const SUB_WIRELESS: &str = "19cbb8fa-5279-450e-9fac-8a3d5fedd0c1";

const PROCTHROTTLEMAX: &str = "bc5038f7-23e0-4960-96da-33abaf5935ec";
const PERFBOOSTMODE: &str = "be337238-0d82-4146-a960-4f3749d470c7";
const PERFEPP: &str = "36687f9e-e3a5-4dbf-b1dc-15eb381c6863";
const CPMINCORES: &str = "0cc5b647-c1df-4637-891a-dec35c318583";
const SYSCOOLPOL: &str = "94d3a615-a899-4ac5-ae2b-e4d8f634367f";
const ASPM: &str = "ee12f906-d277-404b-b6da-e5fa1a576df5";
const VIDEOIDLE: &str = "3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e";
const VIDEONORMALLEVEL: &str = "aded5e82-b909-4619-9949-f5d71dac0bcb";
const VIDEODIMLEVEL: &str = "f1fbfde2-a960-4165-9f88-50667911ce96";
const ADAPTBRIGHT: &str = "fbd9aa66-9553-4097-ba44-ed6e9d65eab8";
const STANDBYIDLE: &str = "29f6c1db-86da-48c5-9fdb-f2b67b1f44da";
const ESBATTTHRESHOLD: &str = "e69653ca-cf7f-4f05-aa73-cb833fa90ad4";
const WIRELESS_SAVING: &str = "12bbebe6-58d6-4636-95bb-3217ef867c1a";

/// "Best power efficiency" power mode overlay.
const BEST_EFFICIENCY: &str = "961cc777-2547-4f9d-8174-7d86181b8a7a";

/// One line of the report.
#[derive(Clone, Debug)]
pub struct Row {
    pub name: String,
    pub detail: String,
    pub ok: bool,
}

/// Run every check against the current configuration.
pub fn run(config: &EcoSetupConfig) -> Vec<Row> {
    let mut rows = Vec::new();

    let active = query_string(BASE, "ActivePowerScheme").unwrap_or_default();
    let eco = eco_guid();
    let target = eco.clone().unwrap_or_else(|| active.clone());

    if eco.is_some() {
        rows.push(Row {
            name: "省电方案".to_string(),
            detail: "已创建「MP14 省电」副本方案".to_string(),
            ok: true,
        });
    } else {
        rows.push(Row {
            name: "省电方案".to_string(),
            detail: "未创建副本方案；下面检查的是当前方案".to_string(),
            ok: false,
        });
    }

    let task_on = task_exists("MP14Tools-EcoOn");
    let task_off = task_exists("MP14Tools-EcoOff");
    rows.push(Row {
        name: "计划任务".to_string(),
        detail: if task_on && task_off {
            "EcoOn / EcoOff 都已创建".to_string()
        } else {
            format!("EcoOn={}  EcoOff={}", yes_no(task_on), yes_no(task_off))
        },
        ok: task_on && task_off,
    });

    let dc = |sub: &str, setting: &str| dc_value(&target, sub, setting);

    rows.push(value_row(
        "最大处理器状态",
        config.cpu_max_percent as u32,
        dc(SUB_PROCESSOR, PROCTHROTTLEMAX),
        "%",
    ));
    if config.disable_turbo {
        rows.push(value_row(
            "睿频加速",
            0,
            dc(SUB_PROCESSOR, PERFBOOSTMODE),
            "（0=已关闭）",
        ));
    }
    rows.push(value_row(
        "能效偏好 EPP",
        config.epp_percent as u32,
        dc(SUB_PROCESSOR, PERFEPP),
        "%",
    ));
    if config.core_parking {
        rows.push(value_row(
            "核心停放：最小未停放核心",
            config.core_parking_percent as u32,
            dc(SUB_PROCESSOR, CPMINCORES),
            "%",
        ));
    }
    if config.passive_cooling {
        rows.push(value_row(
            "系统散热方式",
            1,
            dc(SUB_PROCESSOR, SYSCOOLPOL),
            "（1=被动）",
        ));
    }
    rows.push(value_row(
        "显示器亮度档位",
        config.brightness_percent as u32,
        dc(SUB_VIDEO, VIDEONORMALLEVEL),
        "%",
    ));
    rows.push(value_row(
        "关屏时间",
        config.screen_off_seconds,
        dc(SUB_VIDEO, VIDEOIDLE),
        " 秒",
    ));
    if config.dim_seconds > 0 {
        rows.push(value_row(
            "屏幕变暗超时",
            config.dim_seconds,
            dc(SUB_VIDEO, VIDEODIMLEVEL),
            " 秒",
        ));
    }
    if config.adaptive_brightness {
        rows.push(value_row(
            "自适应亮度",
            1,
            dc(SUB_VIDEO, ADAPTBRIGHT),
            "（1=开启）",
        ));
    }
    if config.sleep_seconds > 0 {
        rows.push(value_row(
            "睡眠超时",
            config.sleep_seconds,
            dc(SUB_SLEEP, STANDBYIDLE),
            " 秒",
        ));
    }
    if config.max_pcie_aspm {
        rows.push(value_row(
            "PCIe 链接电源管理",
            2,
            dc(SUB_PCIEXPRESS, ASPM),
            "（2=最大省电）",
        ));
    }
    if config.wifi_max_saving {
        rows.push(value_row(
            "无线网卡省电",
            3,
            dc(SUB_WIRELESS, WIRELESS_SAVING),
            "（3=最高）",
        ));
    }
    // The battery-saver threshold has to live in the scheme that is normally
    // active - the eco copy is only switched to once battery saver fires.
    rows.push(value_row(
        "节电模式自动开启阈值",
        config.saver_threshold_percent as u32,
        dc_value(&active, SUB_ENERGYSAVER, ESBATTTHRESHOLD),
        "%",
    ));

    if config.power_mode_eco {
        let overlay = query_string(BASE, "ActiveOverlayDcPowerScheme");
        let ok = overlay.as_deref() == Some(BEST_EFFICIENCY);
        rows.push(Row {
            name: "电池下电源模式".to_string(),
            detail: if ok {
                "最佳能效（已在电池上生效）".to_string()
            } else if task_on && task_off {
                "已配置：首次在电池上进入节电模式时由任务设置".to_string()
            } else {
                "未设置（计划任务缺失）".to_string()
            },
            ok: ok || (task_on && task_off),
        });
    }

    rows
}

fn value_row(name: &str, expected: u32, actual: Option<u32>, unit: &str) -> Row {
    let (detail, ok) = match actual {
        Some(value) if value == expected => (format!("已生效：{value}{unit}"), true),
        Some(value) => (
            format!("期望 {expected}{unit}，实际 {value}{unit}"),
            false,
        ),
        None => (
            format!("期望 {expected}{unit}，未写入（使用方案默认值）"),
            false,
        ),
    };
    Row {
        name: name.to_string(),
        detail,
        ok,
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "有"
    } else {
        "无"
    }
}

/// GUID of the "MP14 省电" copy, from the helper script's state file.
fn eco_guid() -> Option<String> {
    let path = crate::config::data_dir().join("eco-setup.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let guid = value.get("ecoGuid")?.as_str()?.trim().to_string();
    (!guid.is_empty()).then_some(guid)
}

fn task_exists(name: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("schtasks")
        .args(["/query", "/tn", name])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn setting_path(scheme: &str, sub: &str, setting: &str) -> String {
    format!(r"{BASE}\{scheme}\{sub}\{setting}")
}

/// Battery-side value of one setting, or `None` when it is at its default.
fn dc_value(scheme: &str, sub: &str, setting: &str) -> Option<u32> {
    query_dword(&setting_path(scheme, sub, setting), "DCSettingIndex")
}

fn query_dword(path: &str, name: &str) -> Option<u32> {
    unsafe {
        let mut key = HKEY::default();
        let path_w = crate::win::wide(path);
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            crate::win::pcw(&path_w),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return None;
        }

        let name_w = crate::win::wide(name);
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut kind = REG_DWORD;
        let status = RegQueryValueExW(
            key,
            crate::win::pcw(&name_w),
            None,
            Some(&mut kind),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        (status == ERROR_SUCCESS && kind == REG_DWORD).then_some(value)
    }
}

fn query_string(path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut key = HKEY::default();
        let path_w = crate::win::wide(path);
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            crate::win::pcw(&path_w),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return None;
        }

        let name_w = crate::win::wide(name);
        let mut buffer = [0u16; 128];
        let mut size = (buffer.len() * std::mem::size_of::<u16>()) as u32;
        let mut kind = REG_SZ;
        let status = RegQueryValueExW(
            key,
            crate::win::pcw(&name_w),
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if status != ERROR_SUCCESS {
            return None;
        }
        let text = crate::win::from_wide(&buffer);
        (!text.is_empty()).then_some(text)
    }
}
