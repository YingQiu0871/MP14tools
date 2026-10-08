# MP14Tools 项目结构

单进程 Windows 托盘工具（Rust + egui + windows-rs）。本文只说明“哪个文件管什么”，功能与使用见 [README](../README.md)。

## 运行时线程

`src/main.rs` 启动后依次拉起：

| 线程 | 模块 | 作用 |
|---|---|---|
| UI | `ui` | 设置窗口（eframe/egui），主线程 |
| 外壳 | `tray` + `osd` + `notice` | 托盘图标、右键菜单、按键反馈提示、两按钮通知 |
| 原始输入 | `touchpad` | 触摸板重按检测（Raw HID） |
| WMI | `oemkeys` | 每个厂商热键事件类一个线程，多数时间阻塞 |
| 触摸板震动 | `haptics` | 通过厂商私有 HID 集合写震动力度 |
| 显示策略 | `power` | 原版电池策略：电池降刷新率、询问关 HDR（默认关闭） |
| 自动档位 | `profiles` | 插电 / 电池 / 节电三档切换刷新率、HDR 与可选命令 |
| 任务栏 | `taskbar` | 任务栏上的 CPU / 网速 / 功耗显示 |
| 效率模式 | `efficiency` | 把后台进程放进 Windows 11 效率模式（EcoQoS），退出时还原 |
| 配置监视 | `main` | 每 1.5 s 检查配置文件内容是否被外部修改 |

线程之间通过 `state` 里的共享状态通信。

## 模块

**配置与状态**
- `config.rs`：`%LOCALAPPDATA%\MP14Tools\config.json` 的读写、默认值回填、数值夹紧；单元测试在文件末尾。
- `state.rs`：各线程共享的运行时状态。
- `catalog.rs`：可映射的按键 / 鼠标键目录。

**输入**
- `touchpad.rs`：重按检测。
- `oemkeys.rs`：厂商热键（WMI 事件订阅）。
- `input.rs`：把动作转成 `SendInput`。
- `haptics.rs`：震动力度。

**省电与显示**
- `display.rs`：显示器枚举、刷新率、电源来源、HDR。
- `power.rs`：原版电池显示策略。
- `profiles.rs`：三档自动切换。
- `battery.rs`：电池状态（容量、功率）。
- `eco_check.rs`：省电体检，对比电源方案实际值与界面设置。
- `efficiency.rs`：后台效率模式。
- `taskbar.rs`：任务栏显示。

**界面与外壳**
- `ui.rs`：设置窗口全部页面。
- `tray.rs`、`osd.rs`、`notice.rs`：托盘、提示、通知。
- `icon.rs`：程序图标（与 `tools/make-icon.ps1` 同一设计）。

**基础设施**
- `autostart.rs`：开机自启（HKCU `Run` 键）及路径自愈。
- `log.rs`：追加写日志文件。
- `console.rs`：可选的控制台窗口。
- `win.rs`：零散 Win32 辅助函数。

## 仓库其他目录

| 路径 | 内容 |
|---|---|
| `build.rs`、`assets/` | 编译时嵌入图标与版本资源 |
| `build.ps1`、`build.cmd`、`.cargo/config.toml` | 本地构建入口；生成物全部在 `build/` |
| `local/install-mp14tools.ps1` | 一键安装：复制 exe、管理员省电设置、写配置 |
| `tools/mp14-eco-setup.ps1`、`tools/mp14-eco-task.ps1` | 电源方案省电设置与对应的计划任务脚本 |
| `tools/mp14-display-test.ps1` | 显示器能力诊断（只读） |
| `tools/diagnose.ps1`、`tools/install-mingw.ps1` | 构建工具链检查与安装 |
| `tools/measure.ps1`、`tools/capture-screenshot.ps1`、`tools/make-icon.ps1` | 资源占用测量、截图、生成图标 |
| `.github/workflows/ci.yml` | 每次 push / PR 构建并测试 |
| `.github/workflows/release.yml` | 推送 `v*` 标签时构建并发布 |
| `docs/releases/` | 各版本发布说明，发布时作为 Release 正文 |
| `docs/archive/` | 早期重构与构建报告、0.2.5 版 README，仅作历史记录 |
