# Terminder

Terminder 是面向 Windows 的本地桌面终端，使用 **Rust、Tauri 2、Svelte 5、TypeScript 和 xterm.js** 构建。通过原生 ConPTY 运行 Shell，在一个窗口中管理多个标签和分屏会话。

当前交付目标为 Windows x64，普通构建的最低运行门槛是 **Windows 10 21H2（build 19044）**，Windows 10 22H2 和 Windows 11 也通过版本检查。项目仍处于开发验证阶段，正式平台验收及已知问题见下文“验证状态”。

## 功能

- **多标签与 Shell 配置**：内置 PowerShell 7、Windows PowerShell 和 cmd 发现规则，支持自定义 Profile、启动参数、目录及环境变量。
- **嵌套分屏**：在当前窗格向右或向下继续分屏；支持拖动分隔线、方向聚焦、临时最大化及单独关闭。每个标签最多 8 个窗格，所有标签共享最多 16 个会话的配置上限。
- **终端交互**：搜索、复制粘贴、多行及控制字符粘贴确认；已退出的会话保留画面，可在原位置重新启动。
- **外观设置**：深色、浅色及跟随系统主题，可配置字体、字号和历史行数；自动使用 WebGL，支持 DOM 兼容模式。
- **桌面窗口**：一体化无边框顶栏、窗口拖动、双击最大化、窗口位置与尺寸恢复。
- **配置与恢复**：TOML 配置、保存冲突检测、备份及原子替换；可选恢复各窗格的启动信息。
- **原生会话管理**：每个会话使用独立 Job Object 管理关联进程，终端输入输出通过二进制 IPC 传输，并设有队列与背压限制。

## 开发环境与启动

在 Windows 上准备以下依赖：

| 依赖 | 要求 |
| --- | --- |
| 操作系统 | Windows 10 21H2 x64（build 19044）或更新版本 |
| Rust | 1.91.0，MSVC 工具链；版本见 [rust-toolchain.toml](rust-toolchain.toml) |
| C++ 构建环境 | Visual Studio C++ Build Tools 与 Windows SDK |
| Node.js | 24.18.0，版本见 [.node-version](.node-version) |
| npm | 11.x；项目声明的包管理器版本为 11.16.0 |
| 桌面运行时 | Microsoft Edge WebView2 Runtime |

以下命令均从仓库根目录执行：

```powershell
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri -- dev
```

Tauri 会自动启动 Vite 开发服务器并打开桌面窗口。安装后的应用不需要 Rust、Node.js 或 C++ 构建工具。

仅预览前端界面时可运行：

```powershell
npm --prefix apps/desktop run dev
```

浏览器预览不具备原生宿主连接，不能启动本机 Shell。当前 Unix 后端返回“不支持”；Linux 可运行共享内核和配置测试，不代表已支持 Linux/macOS 桌面终端。

## 构建安装包

在 Windows 上安装依赖后，执行普通构建：

```powershell
npm --prefix apps/desktop run tauri -- build -- --locked
```

默认输出位置如下。应用程序和安装包的路径会随 `CARGO_TARGET_DIR` 改变，第三方许可及依赖清单固定生成在仓库的 `target/release-notices/` 中：

| 产物 | 默认路径 |
| --- | --- |
| 应用程序 | `target/release/terminder-desktop.exe` |
| NSIS 安装包 | `target/release/bundle/nsis/` |
| 第三方许可及依赖清单 | `target/release-notices/` |

安装包采用当前用户安装模式。默认构建在目标机器缺少 WebView2 时下载 Evergreen bootstrapper；需要在缺少运行时的离线机器上安装时，构建包含离线 WebView2 安装器的变体：

```powershell
npm --prefix apps/desktop run tauri -- build --config src-tauri/tauri.offline.conf.json -- --locked
```

构建流程会生成并打包第三方许可和依赖清单。正式签名证书及发布渠道需由发行方提供，当前仓库未配置自动更新服务。

应用图标源文件为 [terminder.svg](apps/desktop/src/assets/terminder.svg)。修改后运行以下命令，使用项目锁定的 Tauri CLI 更新 Windows ICO：

```powershell
npm --prefix apps/desktop run icons
```

## 使用说明

### 标签与窗格

- 点击标签栏的 `+` 选择终端类型；新建标签快捷键直接启动默认 Profile。双击标签，或在标签按钮获得焦点时按 `F2`，可重命名。
- 标签栏及各窗格的分屏菜单均可向右或向下拆分。新窗格使用来源窗格的 Profile，并按其目录策略选择已知启动目录；不会跟踪 Shell 内后续执行的 `cd`。
- 新建分屏要求拆分后的每格至少容纳 40 列 × 10 行，实际空间要求随字体及字号变化。达到每标签窗格上限、全局会话上限或空间不足时，菜单会说明原因。
- 拖动分隔线可调整比例；聚焦分隔线后可用对应方向键调整，`Home` / `End` 移至允许边界，`Enter` 恢复均分。
- 搜索、复制和粘贴作用于当前窗格。关闭窗格只结束该窗格，关闭标签会处理标签中的全部窗格。

默认自动发现顺序为 **PowerShell 7 → Windows PowerShell → cmd**。显式选择某个 Profile 后，启动失败会报告错误，不会改用其他 Shell。内置 Profile 尊重 Shell 启动脚本，不自动注入 `-NoProfile` 或执行策略绕过参数。

关闭会话会结束其实际关联的 Job 成员，包括由它启动且未通过外部代理创建的 GUI 程序。默认对仍在运行的会话显示关闭确认。

### 默认快捷键

以下为新配置的默认绑定，可在 TOML 的 `[shortcuts]` 中修改对应动作：

| 操作 | 快捷键 | 配置动作 |
| --- | --- | --- |
| 新建默认终端标签 | `Ctrl+Shift+T` | `new_tab` |
| 关闭当前标签及全部窗格 | `Ctrl+Shift+W` | `close_tab` |
| 关闭当前窗格 | `Ctrl+Alt+W` | `close_pane` |
| 向右 / 向下分屏 | `Ctrl+Alt+R` / `Ctrl+Alt+D` | `split_right` / `split_down` |
| 聚焦左 / 右窗格 | `Alt+←` / `Alt+→` | `focus_left` / `focus_right` |
| 聚焦上 / 下窗格 | `Alt+↑` / `Alt+↓` | `focus_up` / `focus_down` |
| 最大化 / 还原当前窗格 | `Ctrl+Alt+Z` | `toggle_pane_zoom` |
| 下一个 / 上一个标签 | `Ctrl+Tab` / `Ctrl+Shift+Tab` | `next_tab` / `previous_tab` |
| 复制 / 粘贴 | `Ctrl+Shift+C` / `Ctrl+Shift+V` | `copy` / `paste` |
| 搜索当前终端 | `Ctrl+Shift+F` | `find` |
| 打开设置 | `Ctrl+,` | `settings` |
| 增大 / 减小 / 重置字号 | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | `zoom_in` / `zoom_out` / `zoom_reset` |

方向键在配置中写作 `ArrowLeft`、`ArrowRight`、`ArrowUp`、`ArrowDown`。例如：`focus_left = "Alt+ArrowLeft"`。

已有显式 `[shortcuts]` 表按原样读取，不自动补齐新增动作；要在旧配置中使用分屏快捷键，需要手动添加对应映射。默认情况下，`Ctrl+C`、`Ctrl+D`、`Ctrl+Z` 保留终端语义。

### 配置

通过设置中的“外观”调整主题、字体及渲染器，通过“Profiles 与配置”编辑完整 TOML。下面展示常用字段及默认值；修改已有文件时编辑对应字段即可：

```toml
schema_version = 1
default_profile = "auto"

[terminal]
font_family = ["Cascadia Mono", "Consolas", "monospace"]
font_size_px = 14.0
theme = "system"
scrollback_lines = 5000
renderer = "auto"

[window]
restore_geometry = true
restore_tabs = false

[session]
max_sessions = 16
confirm_close_running = true

[clipboard]
confirm_multiline = true
max_paste_bytes = 262144
```

`theme` 可设为 `system`、`dark` 或 `light`，`renderer` 可设为 `auto` 或 `dom`。字号范围为 8–48，历史行数为 0–20000，会话上限为 1–16，且“会话上限 × 历史行数”不得超过 160000。

Profile 可配置程序、参数、环境变量和 `cwd_policy`（`inherit_known`、`home`、`fixed`）。自定义程序必须使用绝对路径；`resolver` 与 `program` 必须且只能指定一个，固定目录策略必须提供 `cwd`。Shell、参数和目录的修改只影响新会话，外观设置保存后应用到已打开的终端。

### 用户数据与恢复

| 数据 | Windows 路径 |
| --- | --- |
| 配置 | `%APPDATA%\Terminder\config.toml` |
| 工作区启动信息与窗口几何 | `%LOCALAPPDATA%\Terminder\state\workspace.json` |
| 日志 | `%LOCALAPPDATA%\Terminder\logs\` |

配置与工作区保存时会在同目录保留最近有效备份 `config.bak` / `workspace.bak`；修复损坏配置时保留 `config.invalid` 原文。未知 schema 版本不会被覆盖。日志按 10 MiB 阈值轮转，最多保留 5 份文件。

`window.restore_tabs` 默认关闭，启用后启动时仍会确认是否重建。恢复只使用保存的 Profile、启动目录和自定义标签名创建新进程，Shell 启动脚本可能执行；不会恢复旧进程、重放输入或保存终端缓冲。

当前工作区格式将各窗格分别保存为启动项，恢复后成为独立标签，**不恢复分屏树、比例或窗格最大化状态**。

## 项目结构

```text
terminder/
├── apps/desktop/
│   ├── src/
│   │   ├── App.svelte       # 标签、窗格及应用交互
│   │   ├── contracts/      # 前端数据契约
│   │   ├── state/          # 布局、工作区与快捷键
│   │   ├── terminal/       # xterm、渲染与粘贴
│   │   ├── transport/      # Tauri 桥接与输入输出协议
│   │   └── ui/             # 设置、菜单、窗格和窗口控件
│   ├── src-tauri/          # Rust 桌面宿主、配置存储及平台集成
│   └── tests/              # 原生 WebView2 验证及浏览器测试入口
├── crates/session-core/    # 会话生命周期、ConPTY、Job 与流控
├── tests/
│   ├── e2e/                # Playwright 界面与终端测试
│   └── fixtures/           # PTY 与协议测试夹具
├── scripts/                # 图标与第三方许可生成
└── docs/                   # 设计、决策与验收记录
```

## 开发检查与测试

CI 配置见 [.github/workflows/check.yml](.github/workflows/check.yml)。下列命令用于开发验证，具体执行结果以对应环境的报告为准。

### Rust 与前端检查

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test
npm --prefix apps/desktop run test:e2e
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:notices
```

浏览器 E2E 在 Windows 使用已安装的 Edge；其他环境需先安装 Playwright Chromium：

```sh
npm --prefix apps/desktop exec -- playwright install chromium
```

浏览器测试使用宿主夹具和真实 xterm，不能代替真实 Tauri IPC、WebView2 输入法或原生进程回收验证。Linux 上的 Rust 工作区检查不依赖 GTK/WebKitGTK。

### Windows 原生检查

先验证普通构建的真实 Shell 启动，再运行带测试夹具的完整 Windows 检查：

```powershell
cargo test -p terminder-session-core --test windows_default_build --locked
cargo clippy --workspace --all-targets --features terminder-desktop/test-support --locked -- -D warnings
cargo test --workspace --features terminder-desktop/test-support --locked -- --test-threads=1
```

真实 WebView2 联调使用显式诊断构建：

```powershell
npm --prefix apps/desktop run build
npm --prefix apps/desktop run notices
cargo build -p terminder-session-core --features test-fixture --bin pty-fixture --locked
cargo build -p terminder-desktop --release --features test-support,tauri/custom-protocol --locked
npm --prefix apps/desktop run test:native
```

脚本将配置、日志、截图和报告写入 `target/native-smoke/` 的隔离目录，通过本机诊断端口验证真实 IPC、窗口操作、分屏输入、尺寸同步、重载和关闭。它不替代物理输入法验收。

诊断二进制仅用于测试，不可分发；制作安装包时重新执行普通 `tauri build`。若使用自定义构建目录，可向 `test:native` 依次传入桌面程序和 `pty-fixture.exe` 的路径。

### 验证状态

已有记录覆盖浏览器交互、Rust 规则及 Windows 10 build 19044 上的原生联调；详细范围见 [2026-10-28 嵌套分屏验证记录](docs/verification/nested-split-panes-2026-10-28.md)。

该记录中，重复创建和关闭 ConPTY 的句柄增长仍导致 Windows 资源回收测试失败。完整 Windows 版本矩阵、物理 IME、跨显示器 DPI、屏幕阅读器、长时压力及干净机器安装验收尚未完成。通过版本检查、界面测试或安装包构建，均不代表这些验收已通过。

## 相关文档

- [设计方案](docs/Terminder-Design.md)
- [Windows ConPTY 后端决策](docs/adr/0001-windows-conpty.md)
- [实现与验证记录](docs/verification/implementation.md)
- [原生验收清单](docs/verification/manual.md)
- [嵌套分屏与兼容性记录](docs/verification/nested-split-panes-2026-10-28.md)
- [工程审查记录](docs/verification/review-2026-10-26.md)
- [第三方许可补充说明](docs/third_party/README.md)

## 许可证

本项目使用 [MIT License](LICENSE)。第三方依赖的许可及来源由构建流程生成，并随安装包分发。
