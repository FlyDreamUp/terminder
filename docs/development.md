# 开发、测试与打包

## 工程环境

Cargo 工作区包含 apps/desktop 和 crates/session-core，默认运行桌面程序。rust-toolchain.toml 固定 Rust 1.91.0，Cargo.lock 固定依赖版本。当前桌面依赖 GPUI 0.2.2 和 alacritty_terminal 0.25.1。

Windows x64 开发需要 MSVC C++ 工具链和 Windows SDK。GPUI 使用 fxc.exe 编译着色器；桌面 build.rs 使用 rc.exe 生成图标与版本资源。rc.exe 先从 PATH 查找，再从 WindowsSdkDir 或 Windows Kits 安装目录查找。

~~~powershell
cargo run --locked
cargo run --locked -- --profile cmd --cwd C:\Windows
~~~

非 Windows 环境可以验证可移植模型和内核逻辑，不能运行桌面或真实 ConPTY 后端。诊断构建与正式构建都执行相同的 Windows 版本检查。

## 日常检查

~~~powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
~~~

普通 Windows 测试包含 windows_default_build，它直接运行系统 Shell，检查未启用测试特性的实际后端。涉及进程级句柄计数的用例内部使用互斥，防止同一测试进程中的其他用例干扰计数；PTY 读写和关闭线程仍然并发。

## 原生诊断

terminder-session-core/test-fixture 构建 tests/fixtures/pty_fixture.rs。terminder-desktop/test-support 开启原生诊断入口并启用 fixture；这些入口不进入正式发布程序。

~~~powershell
cargo clippy --workspace --all-targets --features terminder-desktop/test-support --locked -- -D warnings
cargo test -p terminder-session-core --test windows_default_build --locked
cargo test --workspace --features terminder-desktop/test-support --locked --no-fail-fast -- --test-threads=1
cargo build --workspace --features terminder-desktop/test-support --locked
$fixture = (Resolve-Path target/debug/pty-fixture.exe).Path
& target/debug/terminder-desktop.exe --smoke $fixture "$PWD/target/native-gpui/conpty-report.json"
& target/debug/terminder-desktop.exe --ui-smoke $fixture "$PWD/target/native-gpui/ui-report.json"
& target/debug/terminder-desktop.exe --restore-smoke "$PWD/target/native-gpui/ui-report.json" "$PWD/target/native-gpui/restore-report.json"
~~~

每条命令都必须检查退出码；后续成功不能覆盖前面失败。windows_default_build 需单独运行，不能与 test-fixture 合并成同一次 Cargo 调用。

| 入口 | 覆盖 |
| --- | --- |
| --smoke | Bridge、终端解析、真实 ConPTY、输入隔离、resize、大量输出和回收 |
| --ui-smoke | 实际 GPUI 窗口、欢迎页、原生按钮消息、输入、选区复制与剪贴板占用恢复、菜单、分屏、搜索、设置和关闭 |
| --restore-smoke | 第二进程恢复、取消、失效 Profile、关闭全部标签和空快照保存 |

UI 检查在报告父目录下创建隔离的 UUID 数据目录，不读取用户配置。复制检查向系统剪贴板写入 fixture 文本，验证完成或断言失败后恢复读取到的文本或图片；占用检查通过临时线程持有剪贴板，退出时释放并回收线程。恢复检查依赖前一份 UI 报告中的 state_path，最后会保存空快照；重新运行时先产生新的 UI 报告。

窗口检查需要可用的 Windows 桌面与 GPU。它通过实体接口和发给自身 HWND 的鼠标消息检查状态及命中区域，不枚举其他窗口，不注入全局输入；物理键鼠、IME、读屏和截图仍另行验收。

## ConPTY 组件

crates/session-core/build.rs 按 vendor/microsoft-conpty/manifest.json 核验固定二进制的 SHA-256，并复制到 Cargo profile 及 deps 目录，使程序与测试能从自身目录加载组件。构建不下载组件。

应用只从程序目录加载 conpty.dll；OpenConsole.exe 必须同目录。组件来源、版本及维护步骤见 [ConPTY README](../vendor/microsoft-conpty/README.md)。

## 打包

~~~powershell
./scripts/package.ps1
~~~

package.ps1 执行不启用 test-support 的 release 构建，调用 release-notices.ps1，组装便携包并生成 SHA-256。它不安装、不签名、不上传。

| 输出 | 内容 |
| --- | --- |
| target/package/Terminder-0.1.0-windows-x64.zip | 主程序、ConPTY 两个组件、组件清单和许可证、项目许可证、README、第三方许可汇总、依赖清单 |
| 同名 .zip.sha256 | ZIP 校验值 |
| target/release-notices/ | THIRD-PARTY-NOTICES.txt、dependency-inventory.json |

-TargetDirectory 指定 Cargo 构建缓存目录，包仍输出到 target/package。-CargoArguments 只接受缓存和配置选项，如 --offline、--locked、--frozen、--config；不允许借此开启诊断特性。

验包应核对九项内容、ZIP 校验值、程序与 release 构建的一致性、ConPTY 清单哈希以及诊断入口未进入正式程序。需要完整解压后测试，单独复制主程序不能验证完整交付。

## CI

[check.yml](../.github/workflows/check.yml)定义两类任务。Linux 检查格式、默认 Clippy 和可移植测试；Windows 检查普通/诊断 Clippy、普通 Shell、诊断工作区测试、ConPTY、原生窗口、恢复、打包及 Cargo.lock 稳定性。Windows 报告和便携包作为 CI artifact 保存。

## 日志与诊断

应用以 INFO 级别写入 %LOCALAPPDATA%\Terminder\logs\terminder.log，达到 10 MiB 时轮换，最多当前文件和四份历史日志。核心错误包含阶段和系统错误码，日志不主动记录终端字节、命令参数或环境内容。

排查顺序为配置与 Profile → Supervisor 状态 → 后端 I/O 和资源 → Bridge 消费 → Pane 绘制。诊断报告中的终端文本来自隔离 fixture。生成物、临时配置、运行日志与本地恢复快照放在 target，不写回说明文档目录。
