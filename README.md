# MP14Tools

[中文说明](#中文说明) | [English](#english)

为小米book pro 14 2026定制

一个单进程、低占用的 Windows 小工具：重新映射触摸板的重按与厂商（OEM）热键动作，并可调节按压触发力度阈值。
省电方面：按「插电 / 电池 / 节电模式」三档自动切换刷新率与 HDR，一键写入 Windows 电源方案的省电设置，
把后台程序压进 Windows 11 效率模式，并在任务栏实时显示 CPU、网速与功耗。

> 📥 **下载**：[最新版本 Releases](https://github.com/YingQiu0871/MP14tools/releases/latest)
> — 单文件 `mp14tools.exe`（免安装）或 `.msi` 安装包，均无需管理员权限。
>
> 🗂️ **代码结构**：见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

---

## 中文说明

### ✨ 功能

| 功能 | 说明 |
|---|---|
| 触摸板重按映射 | 触摸板重按 → 任意键盘键、鼠标键或组合键。两级压力阈值都是整数，可在界面上拖动或输入精确值，也可一键切到「使用出厂值」（125 / 500） |
| 震动力度调整 | 调整触摸板的**轻触反馈**与**重按反馈**两级力度：范围 `0–128`、步进 `8`，默认即出厂值（80 / 104）。**默认关闭**，不启用时本工具不写触摸板 |
| OEM 热键映射 | 捕获厂商热键并映射成任意键盘键、鼠标键或组合键；每个热键用「报告前缀」决定匹配哪个键 |
| 输出动作 | 键盘按键（字母、数字、功能键、媒体键等）、鼠标左/右/中/侧键，均可带 Ctrl / Shift / Alt / Win 修饰键 |
| 自动档位切换 | 三档：**插电 / 电池 / 节电模式**。每档可设内屏刷新率（不动 / 60 / 120 Hz）、HDR（不动 / 开 / 关）、是否包含外屏，以及进入该档时执行的一条命令。「切换依据」可选节电模式开关或插拔电源。**默认开启**（配置段 `profiles`） |
| Windows 省电设置 | 处理器上限、亮度、关屏/变暗/睡眠时间、节电模式阈值、关闭睿频、PCIe 与无线网卡省电、EPP、核心停放、被动散热、电池下「最佳能效」电源模式等。点「应用省电设置」时以管理员身份调用 `mp14-eco-setup.ps1` 写入电源方案（弹一次 UAC），「撤销」恢复原方案（配置段 `eco_setup`） |
| 电池卡片 / 省电体检 | 显示供电状态、电量、充放电功率、预计剩余时间与电池容量；「省电体检」对照系统实际存储的电源方案值逐项检查，包括 `powercfg` 不显示的隐藏项 |
| 后台进程效率模式 | 不在前台、有可见窗口的程序进入 Windows 11「效率模式」（EcoQoS），回到前台立即恢复；系统关键进程永不处理，可加忽略列表，可选只在电池时启用。免管理员，**默认开启**（配置段 `efficiency`） |
| 任务栏显示 | 在任务栏上显示 CPU 占用、网速（↓↑）、功耗（放电 W / 充电 +W / AC），三项可单独开关；位置默认在天气挂件与开始按钮之间居中，作为任务栏子窗口显示，开始菜单打开时仍可见，截图框选时自动隐藏（配置段 `taskbar`） |
| 内置电池显示策略（高级） | 原版逻辑，**默认关闭**，不要与「自动档位切换」同时开启。电池供电时把刷新率切到设定档位：**内屏**可选 60 / 120 Hz，**外屏**可选最高档 / 60 Hz；插回电源恢复原档位（切换前的档位会存到磁盘，重启后仍然有效）。**程序启动时也会按当前电源状态先执行一次**，不会因为「启动时已经是电池档位」而一直不动。行为可选**不处理 / 通知确认 / 直接切换** |
| HDR 检测（高级） | 属于上面的内置电池显示策略，**只针对内屏**。切电源与切刷新率时检查，若处于开启状态，通知的第二个按钮提供「关闭 HDR」；电池模式下**每次唤醒**也会检查一次 |
| 内屏 / 外屏开关 | 两个独立开关，决定显示调节作用于哪些显示器 |
| 托盘图标 | 打开设置 / 暂停映射 / 命令提示符 / 开机自启 / 退出 |
| OSD 提示 | 触发时在屏幕底部显示圆角提示条，不抢焦点、不挡点击；停留 5 秒后在 1 秒内淡出消失（停留时长可调） |
| 通知窗口 | 需要用户决定时弹出的置顶小窗，文字与按钮居中；略带半透明，动作按钮最多两个，另有常驻的「忽略」按钮可随时关掉；**5 秒无操作后 1 秒淡出收起** |
| 命令提示符 | 可选：随程序弹出控制台窗口实时显示日志；默认关闭 |
| 日志位置 | 日志目录可改（带目录选择器），改动立即生效 |
| 配置热重载 | 直接编辑 `config.json` 保存即生效；配置文件被外部修改时按文件加载且**不覆盖**，并在界面标出 |
| 开机自启 | 单个 `HKCU\...\Run` 值，关掉开关即删除 |

### 🧱 运行要求

- Windows 10 1809+ / Windows 11（x64）；后台进程效率模式需要 Windows 11
- 无需管理员权限（只有「应用省电设置」那一步会弹一次 UAC）
- 无需安装运行时（除系统自带 CRT 外无外部依赖）

### 🖱 使用

1. 运行 `mp14tools.exe`，设置窗口打开（关闭窗口后仍在托盘运行）。
2. **触摸板**页：勾选启用，按住触摸板观察「当前压力」读数与状态，据此设置阈值；
   滑条与数值框都支持精确的整数，按「确定」后生效。勾选「使用出厂值」则固定用 125 / 500。
   再选目标按键。震动力度在同一页，启用后立即写入触摸板（默认关闭）。
3. **OEM 按键**页：为每个厂商热键选目标按键；`报告前缀` 决定匹配哪个键。
4. **显示与省电**页：
   - 「自动档位切换」卡片：选切换依据，为插电 / 电池 / 节电模式三档分别设刷新率、HDR 和外屏；
   - 「Windows 省电设置」卡片：调好各项后点「应用省电设置」（弹一次 UAC），用「省电体检」确认结果，
     需要时点「撤销」恢复原方案；
   - 「后台进程效率模式」卡片：开关、只在电池时启用、忽略列表；
   - 「高级：内置电池显示策略」默认折叠，一般不需要打开。
5. **日志**页：需要时打开命令提示符，或把日志文件换到别的目录。
6. **通用**页：开机自启、托盘图标、OSD，以及任务栏显示的项目、位置、字号与微调。
7. 托盘菜单可暂停映射、打开设置、开命令提示符、开关自启或退出。

**MSI 安装包（推荐普通用户）**：到 [Releases](https://github.com/YingQiu0871/MP14tools/releases/latest)
下载 `mp14tools-<版本>-x64.msi` 双击安装（Windows 标准 Windows Installer 格式，**不需要管理员权限**）。
- 安装到 `%LOCALAPPDATA%\Programs\MP14Tools\`，开始菜单有快捷方式，「设置 → 应用」里可卸载；
- 安装向导里可选：**开机自启**、**安装后应用省电方案**（会弹一次 UAC，等同下面的一键安装脚本）；
- 升级：直接运行新版 MSI，会自动结束旧进程并覆盖；降级会被拒绝；
- 静默安装：`msiexec /i mp14tools-0.8.2-x64.msi /qn ADDLOCAL=FeatMain,FeatAutostart`（再加 `,FeatEco` 同时应用省电方案）；
  卸载：`msiexec /x mp14tools-0.8.2-x64.msi /qn`；
- 卸载只删程序文件和自启项，`%LOCALAPPDATA%\MP14Tools\` 下的配置与日志会保留；如果装过省电方案，卸载前先在程序「省电」页点「撤销」；
- 勾了「应用省电方案」时，安装完成前会停留十几秒并弹 UAC（静默安装 `/qn` 也会弹）；该方案只在首次安装时应用，升级不会重复执行，也不会覆盖你改过的配置；
- 如果是在程序里（而不是安装向导里）打开的开机自启，卸载前请先在程序里关掉，否则启动项会残留；
- 目前**没有代码签名**，首次运行 Windows SmartScreen 可能提示「已保护你的电脑」：点「更多信息 → 仍要运行」即可。可对照 Release 页面给出的 SHA256 校验文件。
- 本地构建 MSI：装好 WiX（`dotnet tool install --global wix --version 5.0.2`，再 `wix extension add --global WixToolset.UI.wixext/5.0.2` 与 `WixToolset.Util.wixext/5.0.2`），先 `.\build.ps1 -Release`，再 `.\installer\build-msi.ps1`，产物在 `build\msi\`。

**一键安装（可选）**：在仓库目录用普通权限运行 `local\install-mp14tools.ps1`。它把 exe 复制到
`%LOCALAPPDATA%\Programs\MP14Tools\`（自启路径不会失效），以管理员身份跑一次
`tools\mp14-eco-setup.ps1` 建好「MP14 省电」方案和两个计划任务，再把三档的 `command`
填成对应的 `schtasks /run` 命令。加 `-DryRun` 只打印计划不动系统。

### ⚙️ 配置文件

`%LOCALAPPDATA%\MP14Tools\config.json`

```jsonc
{
  "version": 1,
  "log": {
    "console": false,              // 是否随程序弹出命令提示符窗口
    "directory": ""                // 日志目录，留空 = %LOCALAPPDATA%\MP14Tools
  },
  "start_with_windows": false,
  "show_tray_icon": true,
  "osd": {
    "enabled": true,
    "duration_ms": 5000            // OSD 停留时长（毫秒），之后 1 秒淡出
  },
  "taskbar": {
    "enabled": true,
    "show_cpu": true,
    "show_network": true,
    "show_power": true,
    "position": "auto",            // auto（天气挂件与开始按钮之间）| tray（托盘左侧）| widget（天气挂件后）
    "font_size": 12.0,             // 9–20
    "offset_x": 0,                 // 水平微调，-200–400
    "offset_y": 0,                 // 垂直微调，-20–20
    "update_ms": 1000              // 刷新间隔，500–10000
  },
  "efficiency": {
    "enabled": true,
    "only_on_battery": false,      // true = 只在电池供电时节流
    "ignore": [],                  // 永不处理的进程名，可省略 .exe
    "interval_ms": 2000            // 1000–30000
  },
  "touchpad": {
    "enabled": true,
    "factory_values": false,       // true = 固定用出厂阈值，忽略下面两个值
    "light_press_threshold": 125,  // 整数，1–500
    "deep_press_threshold": 500,   // 整数，须大于轻按，最大 1000
    "action": { "modifiers": [], "target": "F13" }
  },
  "haptics": {
    "enabled": false,              // 默认关闭：不启用时本工具不写触摸板
    "factory_values": true,        // true = 固定用出厂力度
    "normal_strength": 80,         // 0–128，步进 8
    "deep_press_strength": 104,    // 不低于轻触
    "device_marker": "hid#bltp7853&col05"
  },
  "display": {
    "enabled": false,              // 高级：原版电池策略，默认关闭，不要与 profiles 同时开启
    "battery_action": "notify",    // off | notify | force
    "internal_refresh_rate": 60,   // 内屏档位：60 | 120
    "external_refresh_rate": "60hz", // 外屏档位：highest（最高档）| 60hz
    "hdr_check": true,             // 只检测内屏 HDR
    "internal": true,              // 影响内屏
    "external": true               // 影响外屏
  },
  "profiles": {
    "enabled": true,
    "mode": "battery_saver",       // battery_saver（看节电模式开关）| ac_dc（看插拔电源）
    "high_only_on_ac": true,       // 只有插电时才进 high 档
    "poll_ms": 2000,               // 500–60000
    // 每档：refresh = 0（不动）| 60 | 120；hdr = true | false | null（不动）；
    // command 默认为空，安装脚本会把 eco 填成 "schtasks /run /tn MP14Tools-EcoOn"、
    // high 与 medium 填成 "schtasks /run /tn MP14Tools-EcoOff"
    "high":   { "refresh": 120, "hdr": true,  "external": false, "command": "" },
    "medium": { "refresh": 60,  "hdr": false, "external": false, "command": "" },
    "eco":    { "refresh": 60,  "hdr": false, "external": false, "command": "" }
  },
  "eco_setup": {                   // 「Windows 省电设置」卡片的值，点「应用」才写入电源方案
    "cpu_max_percent": 50,
    "brightness_percent": 40,
    "screen_off_seconds": 60,
    "saver_threshold_percent": 40,
    "disable_turbo": true,
    "max_pcie_aspm": true,
    "wifi_max_saving": true,
    "epp_percent": 80,
    "core_parking": false,
    "core_parking_percent": 25,
    "passive_cooling": false,
    "adaptive_brightness": false,
    "dim_seconds": 30,
    "sleep_seconds": 900,
    "power_mode_eco": true,
    "script": ""                   // 留空 = 配置目录下的 mp14-eco-setup.ps1
  },
  "oem_keys": [
    {
      "name": "Performance mode (Fn+K)",
      "enabled": true,
      "report_hex": "01-28-01",    // 前缀匹配，大小写与连字符随意
      "press_only": true,          // 只在按下时触发
      "action": { "modifiers": [], "target": "KeyP" }
    }
  ]
}
```

- `target` 取值见设置界面下拉列表：鼠标键 `MouseLeft` / `MouseRight` / `MouseMiddle` /
  `MouseX1` / `MouseX2`，键盘键 `KeyA`…`KeyZ`、`Digit0`…、`F1`…`F24`、媒体键等；
  `modifiers` 为 `ctrl` / `shift` / `alt` / `win`。
- `internal_refresh_rate` 只接受 `60` / `120`；`external_refresh_rate` 为 `"highest"`
  （外屏跑它的最高可用档）或 `"60hz"`。切换时取不高于该档位的最高可用档，
  插回电源时恢复切换前的档位。
- 手工改完保存即生效（程序每 1.5 秒比对一次文件内容），越界数值会被自动夹到合法范围。
- 显示调节会把「切换前的档位」写进同目录的 `display_state.json`，插回电源（含重启后）据此恢复；
  删掉这个文件只会让下一次插回电源少一次恢复，没有其它影响。

### 🔨 构建

前置：Rust 工具链（MSVC 或 GNU 均可）。

#### 1. 安装工具链（GNU 方案，无需管理员）

```powershell
$dir = "$env:TEMP\rustup-install"; New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile "$dir\rustup-init.exe"
& "$dir\rustup-init.exe" -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain none
& "$env:USERPROFILE\.cargo\bin\rustup.exe" toolchain install stable --profile minimal
& "$env:USERPROFILE\.cargo\bin\rustup.exe" default stable-x86_64-pc-windows-gnu
```

#### 2. 安装 dlltool/as 工具对（**仅 GNU 工具链需要**）

```powershell
.\tools\install-mingw.ps1
```

> 使用 MSVC 工具链（`stable-x86_64-pc-windows-msvc` + VS Build Tools）时这一步不需要。

#### 3. 构建

```powershell
.\build.ps1              # debug 构建
.\build.ps1 -Release     # release 构建（体积更小）
.\build.ps1 -Run         # 构建并运行
.\build.ps1 -Check       # 仅类型检查
```

若本机执行策略禁止运行脚本，用同参数的批处理入口 `build.cmd`（只在这一次进程里放宽策略）：

```cmd
build.cmd
build.cmd -Release
```

构建前会先结束**正在运行且路径等于本次产物**的实例（Windows 不允许覆盖运行中的映像），
加 `-KeepRunning` 可跳过。

产物：`build\obj\debug\mp14tools.exe` 或 `build\obj\release\mp14tools.exe`。

> ✅ **已验证**：debug 构建已在另一台 Windows x64 电脑上从零跑通
> （`.\build.ps1` → `build\obj\debug\mp14tools.exe`），全程不需要管理员权限，
> 除上面第 1、2 步外没有额外的手工步骤。

#### 4. 自动发布（CI）

仓库带 `.github/workflows/release.yml`：推 `v*` 标签时在 GitHub 的 Windows runner 上按上面同样的
步骤构建，并校验「`Cargo.toml` 版本 = 标签 = 内嵌 `FileVersion`」，三者不一致就直接失败（这样不会
因为 runner 上缺 windres 而发出去一个没有图标、没有版本属性的 exe）。通过后自动创建 Release，附上
`mp14tools.exe`，发布说明取自 `docs/releases/<标签>.md`（没有该文件时只写一行标题）。
已经推过的标签可以在 Actions 页面用 **Run workflow** 补发，把标签名填进输入框即可。

另有 `.github/workflows/ci.yml`：每次 push 和 PR 都在 Windows runner 上跑 `cargo build` 与
`cargo test`。本地同样可以用 `cargo test` 跑单元测试。

### ⚠️ 已知限制

- OEM 热键前缀默认按参考机型提供，其它机型需要在 `config.json` 里改成自己的前缀；
  只有走系统事件上报的厂商键能作为触发源（不安装全局键盘钩子）。
- 震动力度**无法读回**：因此「其他软件静默改了硬件力度」检测不到；能检测的是设备不再应答与配置文件被外部修改。
- 显示调节只做刷新率与 HDR，不改分辨率；HDR 只针对内屏，且需要驱动支持。
- 显示调节需要机器**报告得出电池状态**：台式机、报告不出电源来源的虚拟机里该功能不适用，
  程序会在日志里写明并直接跳过，不会去猜一个档位。
- 外接显示器的可用档位由它自己上报，超出范围的档位不会出现在选择里（外屏只有「最高档 / 60 Hz」两种目标）。
- OSD 文本使用 Microsoft YaHei UI；缺少该字体的系统上中文会退化为方框。
- 程序未签名，首次运行可能触发 SmartScreen 提示。

### 📄 许可

以 **GNU GPL-3.0** 发布，`LICENSE` 为许可证原文。本项目是
[`Meow-Box`](https://github.com/leehyukshuai/Meow-Box) 的衍生作品；
分发本程序或其修改版时请一并保留 `LICENSE` 与署名。

### 🔏 代码签名政策

免费代码签名由 [SignPath.io](https://signpath.io) 提供，证书由 [SignPath Foundation](https://signpath.org) 颁发。

- 提交者与审核者（Committers and reviewers）：[YingQiu0871](https://github.com/YingQiu0871)
- 批准者（Approvers）：[YingQiu0871](https://github.com/YingQiu0871)

隐私声明：除非用户或安装、操作本程序的人明确要求，本程序不会向其他联网系统传输任何信息。
（This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.）

---

## English

Customized for the Xiaomi Book Pro 14 2026.

A small single-process, low-footprint Windows utility: it remaps the touchpad deep press and the
vendor (OEM) hotkeys to custom actions, and it can adjust the pressure trigger thresholds.
For battery life it switches the refresh rate and HDR across three profiles (plugged in / on battery /
battery saver), writes power-saving settings into the Windows power plan in one click, puts background
programs into Windows 11 efficiency mode, and shows CPU, network speed and power draw on the taskbar.

> 📥 **Download**: [latest release](https://github.com/YingQiu0871/MP14tools/releases/latest)
> — a single `mp14tools.exe` (portable) or an `.msi` installer, neither needs admin rights.

### ✨ Features

| Feature | Description |
|---|---|
| Touchpad deep press mapping | Touchpad deep press → any keyboard key, mouse button or chord. Both pressure thresholds are integers and can be dragged or typed exactly in the UI, and one switch falls back to the "use factory values" setting (125 / 500) |
| Haptic strength | Adjusts the touchpad's **light press feedback** and **deep press feedback** levels: range `0–128`, step `8`, defaults equal the factory values (80 / 104). **Off by default**; while it is off this tool does not write to the touchpad |
| OEM hotkey mapping | Captures vendor hotkeys and maps them to any keyboard key, mouse button or chord; the "report prefix" of each entry decides which key is matched |
| Output actions | Keyboard keys (letters, digits, function keys, media keys, …), mouse left/right/middle/side buttons, each optionally with Ctrl / Shift / Alt / Win modifiers |
| Automatic profiles | Three profiles: **plugged in / on battery / battery saver**. Each sets the internal refresh rate (leave / 60 / 120 Hz), HDR (leave / on / off), whether external displays follow, and a command run on entering it. The switch is either the battery-saver flag or the power source. **On by default** (`profiles` section) |
| Windows power settings | Max processor state, brightness, screen off / dim / sleep timeouts, battery-saver threshold, turbo off, PCIe and Wi-Fi power saving, EPP, core parking, passive cooling, "best power efficiency" mode on battery, and more. "Apply" runs `mp14-eco-setup.ps1` elevated (one UAC prompt) to write them into the power plan; "Undo" restores the original plan (`eco_setup` section) |
| Battery card / power check | Shows the power source, charge, charge/discharge power, estimated time left and battery capacity; the "power check" compares every value against what the system actually stores, hidden settings `powercfg` does not show included |
| Background efficiency mode | Programs with a visible window that are not in the foreground go into Windows 11 "Efficiency mode" (EcoQoS) and are restored the moment they come back to the front; system-critical processes are never touched, an ignore list is available, and it can be limited to battery power. No admin rights, **on by default** (`efficiency` section) |
| Taskbar overlay | CPU load, network speed (↓↑) and power (discharge W / charge +W / AC) on the taskbar, each switchable; by default centred between the weather widget and the Start button, drawn as a child of the taskbar so it stays visible while the Start menu is open, hidden during a screenshot selection overlay (`taskbar` section) |
| Built-in battery display policy (advanced) | The original logic, **off by default**; do not run it together with the automatic profiles. On battery the refresh rate is switched to the configured mode: the **internal** panel offers 60 / 120 Hz, an **external** display offers its highest mode / 60 Hz; the mode from before the switch is restored when power is plugged back in (it is written to disk, so it survives a restart). **The policy also runs once right after start-up**, so a machine that starts on battery does not simply sit on the wrong mode. The behaviour can be **off / notify / force** |
| HDR check (advanced) | Part of the built-in battery display policy above. **Internal panel only.** It is checked when the power source or the refresh rate changes; if HDR is on, the second button of the notification offers "turn HDR off". On battery it is checked once more **on every resume** |
| Internal / external switches | Two independent switches decide which displays the display policy applies to |
| Tray icon | Open settings / pause mapping / command prompt / run at logon / exit |
| OSD | On a trigger it shows a rounded hint bar at the bottom of the screen, without stealing focus or blocking clicks; it stays for 5 seconds and then fades out within 1 second (the hold time is configurable) |
| Notice window | An always-on-top small window shown when the user has to decide something, with the text and buttons centred; it is slightly translucent, offers at most two action buttons plus an always-present "ignore" button, and **fades out over 1 second after 5 seconds without input** |
| Command prompt | Optional: a console window showing the log in real time along with the program; off by default |
| Log location | The log directory can be changed (with a folder picker) and takes effect immediately |
| Live config reload | Editing `config.json` and saving it applies immediately; when the file is modified externally it is loaded from the file and **not overwritten**, and the UI marks it |
| Run at logon | A single `HKCU\...\Run` value, deleted as soon as the switch is turned off |

### 🧱 Requirements

- Windows 10 1809+ / Windows 11 (x64); background efficiency mode needs Windows 11
- No admin rights (only "Apply" on the Windows power settings shows one UAC prompt)
- No runtime installation (no external dependency besides the CRT shipped with the system)

### 🖱 Usage

1. Run `mp14tools.exe`; the settings window opens (closing the window keeps the tool running in the tray).
2. **Touchpad** page: tick the switch, hold the touchpad to watch the "current pressure" reading and
   state, and set the thresholds from that; both the slider and the number box accept exact integers
   and take effect after the matching "OK". "Use factory values" pins them to 125 / 500.
   Then choose the target key. Haptic strength is on the same page and is written to the touchpad
   immediately once enabled (off by default).
3. **OEM keys** page: choose a target key for each vendor hotkey; the `report prefix` decides which key matches.
4. **Display & power** page:
   - "Automatic profiles": choose what drives the switch, then set refresh rate, HDR and external
     displays for the plugged-in / battery / battery-saver profiles;
   - "Windows power settings": adjust the values, press "Apply" (one UAC prompt), confirm with the
     power check, and use "Undo" to restore the original plan if needed;
   - "Background efficiency mode": the switch, battery-only option and ignore list;
   - "Advanced: built-in battery display policy" is collapsed by default and rarely needed.
5. **Log** page: open the command prompt if needed, or move the log file to another directory.
6. **General** page: run at logon, tray icon, OSD, and the taskbar overlay's items, position, font size and offsets.
7. The tray menu can pause the mapping, open the settings, open the command prompt, toggle autostart or exit.

**MSI installer (recommended)**: download `mp14tools-<version>-x64.msi` from
[Releases](https://github.com/YingQiu0871/MP14tools/releases/latest) and double-click it (standard
Windows Installer package, **no admin rights needed**).
- Installs to `%LOCALAPPDATA%\Programs\MP14Tools\`, adds a Start menu shortcut, and uninstalls from Settings → Apps.
- The wizard offers two options: **run at logon** and **apply the power-saving plan after install** (one UAC prompt, same as the one-step install script below).
- Upgrade by running a newer MSI (the old process is closed and replaced); downgrades are refused.
- Silent install: `msiexec /i mp14tools-0.8.2-x64.msi /qn ADDLOCAL=FeatMain,FeatAutostart` (add `,FeatEco` to also apply the power plan); uninstall: `msiexec /x mp14tools-0.8.2-x64.msi /qn`.
- Uninstalling removes the program files and the autostart entry but keeps `%LOCALAPPDATA%\MP14Tools\` (config, logs). If you applied the power plan, click "Undo" on the program's power page before uninstalling.
- Applying the power plan makes the installer pause for 10+ seconds and show a UAC prompt (also with `/qn`); it only runs on first install, so upgrades neither repeat it nor overwrite your edited config.
- If you turned on autostart inside the program (not in the wizard), turn it off there before uninstalling, or the startup entry stays behind.
- The package is **not code-signed**, so Windows SmartScreen may show "Windows protected your PC" on first run: click "More info → Run anyway". Compare the SHA256 on the release page if you want to verify the file.
- Build it locally: install WiX (`dotnet tool install --global wix --version 5.0.2`, then `wix extension add --global WixToolset.UI.wixext/5.0.2` and `WixToolset.Util.wixext/5.0.2`), run `.\build.ps1 -Release`, then `.\installer\build-msi.ps1`; the result is in `build\msi\`.

**One-step install (optional)**: run `local\install-mp14tools.ps1` from the repository without admin
rights. It copies the exe to `%LOCALAPPDATA%\Programs\MP14Tools\` (so the autostart path stays
valid), runs `tools\mp14-eco-setup.ps1` elevated once to create the "MP14 省电" plan and two
scheduled tasks, and fills each profile's `command` with the matching `schtasks /run` call. Add
`-DryRun` to print the plan without changing anything.

### ⚙️ Configuration

`%LOCALAPPDATA%\MP14Tools\config.json`

```jsonc
{
  "version": 1,
  "log": {
    "console": false,              // show a console window along with the program
    "directory": ""                // log directory, empty = %LOCALAPPDATA%\MP14Tools
  },
  "start_with_windows": false,
  "show_tray_icon": true,
  "osd": {
    "enabled": true,
    "duration_ms": 5000            // how long the OSD stays, in milliseconds; then a 1 s fade
  },
  "taskbar": {
    "enabled": true,
    "show_cpu": true,
    "show_network": true,
    "show_power": true,
    "position": "auto",            // auto (between weather widget and Start) | tray | widget
    "font_size": 12.0,             // 9–20
    "offset_x": 0,                 // horizontal nudge, -200–400
    "offset_y": 0,                 // vertical nudge, -20–20
    "update_ms": 1000              // refresh interval, 500–10000
  },
  "efficiency": {
    "enabled": true,
    "only_on_battery": false,      // true = only throttle on battery power
    "ignore": [],                  // process names never touched, ".exe" optional
    "interval_ms": 2000            // 1000–30000
  },
  "touchpad": {
    "enabled": true,
    "factory_values": false,       // true = always use the factory thresholds, ignoring the two below
    "light_press_threshold": 125,  // integer, 1–500
    "deep_press_threshold": 500,   // integer, must be above the light press, max 1000
    "action": { "modifiers": [], "target": "F13" }
  },
  "haptics": {
    "enabled": false,              // off by default: while it is off this tool does not write to the touchpad
    "factory_values": true,        // true = always use the factory strengths
    "normal_strength": 80,         // 0–128, step 8
    "deep_press_strength": 104,    // not below the light press
    "device_marker": "hid#bltp7853&col05"
  },
  "display": {
    "enabled": false,              // advanced: the original battery policy, off by default; do not combine with profiles
    "battery_action": "notify",    // off | notify | force
    "internal_refresh_rate": 60,   // internal mode: 60 | 120
    "external_refresh_rate": "60hz", // external mode: highest | 60hz
    "hdr_check": true,             // checks the internal panel's HDR only
    "internal": true,              // affects the internal display
    "external": true               // affects the external displays
  },
  "profiles": {
    "enabled": true,
    "mode": "battery_saver",       // battery_saver (battery-saver flag) | ac_dc (power source)
    "high_only_on_ac": true,       // only enter "high" while plugged in
    "poll_ms": 2000,               // 500–60000
    // per profile: refresh = 0 (leave) | 60 | 120; hdr = true | false | null (leave);
    // command is empty by default - the installer sets eco to "schtasks /run /tn MP14Tools-EcoOn"
    // and high / medium to "schtasks /run /tn MP14Tools-EcoOff"
    "high":   { "refresh": 120, "hdr": true,  "external": false, "command": "" },
    "medium": { "refresh": 60,  "hdr": false, "external": false, "command": "" },
    "eco":    { "refresh": 60,  "hdr": false, "external": false, "command": "" }
  },
  "eco_setup": {                   // values of the "Windows power settings" card; written only on "Apply"
    "cpu_max_percent": 50,
    "brightness_percent": 40,
    "screen_off_seconds": 60,
    "saver_threshold_percent": 40,
    "disable_turbo": true,
    "max_pcie_aspm": true,
    "wifi_max_saving": true,
    "epp_percent": 80,
    "core_parking": false,
    "core_parking_percent": 25,
    "passive_cooling": false,
    "adaptive_brightness": false,
    "dim_seconds": 30,
    "sleep_seconds": 900,
    "power_mode_eco": true,
    "script": ""                   // empty = mp14-eco-setup.ps1 in the configuration directory
  },
  "oem_keys": [
    {
      "name": "Performance mode (Fn+K)",
      "enabled": true,
      "report_hex": "01-28-01",    // prefix match, case and hyphens are free
      "press_only": true,          // trigger on press only
      "action": { "modifiers": [], "target": "KeyP" }
    }
  ]
}
```

- The `target` values are the ones in the settings drop-down: mouse buttons
  `MouseLeft` / `MouseRight` / `MouseMiddle` / `MouseX1` / `MouseX2`, keyboard keys
  `KeyA`…`KeyZ`, `Digit0`…, `F1`…`F24`, media keys, and so on;
  `modifiers` are `ctrl` / `shift` / `alt` / `win`.
- `internal_refresh_rate` only accepts `60` / `120`; `external_refresh_rate` is either `"highest"`
  (the external display runs at its highest available mode) or `"60hz"`. The switch lands on the
  highest available mode not above the chosen one, and the mode from before the switch is restored
  when power is plugged back in.
- Saving a manual edit applies immediately (the program compares the file content every 1.5 seconds),
  and out-of-range numbers are clamped to the valid range.
- The display policy writes the mode from before a switch to `display_state.json` next to the
  configuration, so plugging the charger back in (even after a restart) restores it. Deleting that
  file only costs one restore, nothing else.

### 🔨 Build

Prerequisite: a Rust toolchain (MSVC or GNU are both fine).

#### 1. Install the toolchain (GNU route, no admin)

```powershell
$dir = "$env:TEMP\rustup-install"; New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile "$dir\rustup-init.exe"
& "$dir\rustup-init.exe" -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain none
& "$env:USERPROFILE\.cargo\bin\rustup.exe" toolchain install stable --profile minimal
& "$env:USERPROFILE\.cargo\bin\rustup.exe" default stable-x86_64-pc-windows-gnu
```

#### 2. Install the dlltool/as pair (**GNU toolchain only**)

```powershell
.\tools\install-mingw.ps1
```

> This step is not needed with the MSVC toolchain (`stable-x86_64-pc-windows-msvc` + VS Build Tools).

#### 3. Build

```powershell
.\build.ps1              # debug build
.\build.ps1 -Release     # release build (smaller)
.\build.ps1 -Run         # build and run
.\build.ps1 -Check       # type check only
```

If the local execution policy blocks scripts, use the batch entry point with the same arguments
(the policy is relaxed for that one process only):

```cmd
build.cmd
build.cmd -Release
```

Before building, any instance **running from exactly this output path** is ended (Windows does not
allow overwriting a running image); add `-KeepRunning` to skip that.

Output: `build\obj\debug\mp14tools.exe` or `build\obj\release\mp14tools.exe`.

> ✅ **Verified**: the debug build has been run through from scratch on another Windows x64 machine
> (`.\build.ps1` → `build\obj\debug\mp14tools.exe`), with no admin rights and no manual steps
> beyond steps 1 and 2 above.

#### 4. Automatic releases (CI)

`.github/workflows/release.yml` builds on a GitHub Windows runner with the same steps as above whenever
a `v*` tag is pushed, and refuses to publish unless the manifest version, the tag and the embedded
`FileVersion` all agree — that is what keeps a runner without windres from shipping an executable with
no icon and no version properties. It then creates the release with `mp14tools.exe` attached, using
`docs/releases/<tag>.md` as the body (falling back to a one-line title). For a tag that was
pushed earlier, run the workflow manually and pass the tag.

`.github/workflows/ci.yml` runs `cargo build` and `cargo test` on a Windows runner for every push and
pull request. Locally, `cargo test` runs the same unit tests.

### ⚠️ Known limitations

- The OEM hotkey prefixes are shipped for the reference model; other models need their own prefixes in
  `config.json`; only vendor keys reported through system events can act as a trigger (no global
  keyboard hook is installed).
- The haptic strength **cannot be read back**: a strength silently changed by another tool is therefore
  not detected; what can be detected is a device that no longer answers and an externally modified
  configuration file.
- The display policy only handles the refresh rate and HDR, not the resolution; HDR is handled for the
  internal panel only, and needs driver support.
- The display policy needs the machine to **report a battery state**: on a desktop, or in a VM that
  cannot tell the power source apart, the feature does not apply — the log says so, and no mode is
  guessed or touched.
- The modes available on an external display are the ones it reports itself, so modes outside that
  range never appear in the choice (an external display only has the two targets "highest / 60 Hz").
- OSD text uses Microsoft YaHei UI; on systems without that font Chinese degrades to boxes.
- The program is unsigned, so the first run may trigger a SmartScreen prompt.

### 📄 License

Released under the **GNU GPL-3.0**; `LICENSE` is the verbatim license text. This project is a
derivative work of [`Meow-Box`](https://github.com/leehyukshuai/Meow-Box); distributing this program
or a modified version requires keeping `LICENSE` and this attribution.

### 🔏 Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- Committers and reviewers: [YingQiu0871](https://github.com/YingQiu0871)
- Approvers: [YingQiu0871](https://github.com/YingQiu0871)

Privacy statement: This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.
