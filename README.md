# Terminder

使用 Rust、Tauri 2、Svelte 5 和 xterm.js 的本机桌面终端。按 [设计方案](Terminder-Design.md) 重建；普通构建的最低运行要求为 **Windows 10 21H2 x64（build 19044）**，也放行 Windows 10 22H2 和 Windows 11。**实现、自动化检查和正式平台验收分别记录**，未通过对应 Windows 版本、IME、安装与稳定性矩阵前不视为正式发布。

- 原生 ConPTY，每个会话在进程创建时加入独立 Job Object。
- 多标签、重命名、搜索、复制粘贴、深浅主题、字体与 Profile 配置。
- 原始二进制 IPC、连续消费确认、输入顺序和有界背压。
- 保留退出终端，重启创建新会话；退出窗口先完成资源回收。
- TOML 配置、注释保留、冲突检查、备份与原子替换；可选恢复启动信息。

## 开发

可在 Windows 10 21H2 x64（build 19044）及以上系统进行普通构建。安装 Rust 1.91.0（MSVC）、Visual Studio C++ Build Tools、Node.js 24.18.0 / npm 11 和 WebView2 Runtime。安装后的应用不需要 Rust 或 Node.js。

```powershell
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri -- dev
```

仅运行 `npm --prefix apps/desktop run dev` 是浏览器预览，不能启动本机 Shell，不提供模拟终端后门。

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p terminder-session-core --test windows_default_build --locked
cargo test -p terminder-session-core --features test-fixture --locked --test windows_contract -- --test-threads=1
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run test
npm --prefix apps/desktop run test:e2e
npm --prefix apps/desktop run build
```

浏览器 E2E 在 Windows 使用已安装的 Edge，其他环境需先运行 `npm --prefix apps/desktop exec playwright install chromium`。浏览器夹具只验证组件与 xterm 行为，不能证明真实 Tauri IPC、WebView2 IME 或 OS 回收正确。

Linux 可运行共享内核和配置测试，不依赖 GTK/WebKitGTK。当前 Unix 后端明确返回不支持，不作为 Linux/macOS 产品支持声明。

原生 WebView2 联调使用显式诊断构建，配置和进程仅存在于 `target/native-smoke/` 的隔离目录。它验证真实 IPC、权限、终端交互、重载与关闭，不替代物理 IME 验收；诊断二进制不可分发：

```powershell
npm --prefix apps/desktop run build
npm --prefix apps/desktop run notices
cargo build -p terminder-session-core --features test-fixture --bin pty-fixture --locked
cargo build -p terminder-desktop --release --features test-support,tauri/custom-protocol --locked
npm --prefix apps/desktop run test:native
```

## 构建

以下命令使用默认应用 features；Windows 10 不需要启用 `test-support` 或 `test-fixture`。build 19044 已有原生联调证据，但重复创建/关闭 ConPTY 的句柄增长问题仍未解决，正式发布前须通过资源回收验收；版本检查放行不代表稳定性已通过。

```powershell
npm --prefix apps/desktop run tauri -- build
# 同一应用资源的离线 WebView2 依赖安装变体：
npm --prefix apps/desktop run tauri -- build --config src-tauri/tauri.offline.conf.json
```

构建时从锁文件和依赖原文生成第三方许可及依赖清单，并随安装包携带。默认在 `target/release/bundle/nsis/` 输出用户级 NSIS 安装包，缺少 WebView2 时下载官方 Evergreen bootstrapper。离线机器缺少运行时须使用 offlineInstaller 变体。正式签名证书和发布渠道由发行方提供，本仓库不包含证书、自动更新服务或遥测上传。

## 用户数据

- 配置：`%APPDATA%\Terminder\config.toml`。
- 启动信息：`%LOCALAPPDATA%\Terminder\state\workspace.json`。
- 日志：`%LOCALAPPDATA%\Terminder\logs\`，单文件 10 MiB，最多 5 份。
- 配置和工作区最近有效备份使用同目录 `.bak` 文件；修复损坏配置时另存一份 `.invalid` 原文。未知 schema 不写回。

默认只创建默认 Shell；自动选择顺序为 PowerShell 7、Windows PowerShell、cmd。明确选择失败不会切换 Shell。Shell Profile 默认受尊重，不注入 `-NoProfile`、执行策略绕过或 `cd` 命令。

工作区恢复默认关闭，开启后仍会确认重建；不会恢复旧进程、重放输入或保存终端缓冲。关闭会话会结束其实际关联的 Job 成员，包括由它启动且未通过外部代理创建的 GUI 程序。

默认快捷键为 Ctrl+Shift+T/W/C/V/F、Ctrl+Tab / Ctrl+Shift+Tab、Ctrl+, 和 Ctrl+= / Ctrl+- / Ctrl+0。双击标签或按 F2 重命名。Ctrl+C/D/Z 保留终端语义。

[实现与验证记录](docs/verification/implementation.md) · [平台验收清单](docs/verification/manual.md) · [后端决策](docs/adr/0001-windows-conpty.md)
