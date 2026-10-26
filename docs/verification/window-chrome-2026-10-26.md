# 图标与一体化窗口（2026-10-26）

> 本记录保存图标和一体化窗口阶段的证据；随后新建标签菜单的交互验证与更新后的安装包哈希见[终端类型菜单](profile-menu-2026-10-26.md)。

## 目标与范围

按用户反馈替换原应用图标，移除白色系统标题栏及外框，将窗口按钮整合到现有顶栏。保留上一轮工程审查的全部改动；本轮之前的触达文件快照位于 `target/window-chrome/before/`。

- 图标采用薄荷色圆角底及深色 `>_` 符号。可编辑源为 `apps/desktop/src/assets/terminder.svg`，应用顶栏直接引用；`npm --prefix apps/desktop run icons` 通过已安装的 Tauri CLI 生成 Windows ICO。
- ICO 包含 16、24、32、48、64、256 像素的 32 位图像，使用同一源生成任务栏、EXE 与安装包图标。生成脚本只复制 Windows 产物，清理其独立临时目录。NSIS 的 `installerIcon` 和 `uninstallerIcon` 也显式指向同一 ICO，避免安装器继续采用默认图标。
- 原生窗口设置 `decorations(false)` 和 `shadow(false)`；顶栏高 46 px，使用现有主题色，避免额外系统白边。顶栏空白和品牌区可拖动/双击，搜索、设置及窗口按钮明确排除拖动。
- `WindowControls.svelte` 使用 Tauri 当前窗口 API 执行最小化、最大化/还原及关闭，并监听窗口尺寸事件更新按钮状态。异步订阅在组件销毁后释放，浏览器预览不显示不可用窗口按钮。
- 能力清单仅增加上述窗口动作、拖动/双击、最大化状态读取及事件订阅/解除订阅；仍限于 `main` 窗口，没有授予 destroy、创建窗口或外部进程权限。
- 关闭调用 `Window.close()`，继续进入原来的 `CloseRequested` → 确认 → Supervisor 回收 → 放行退出流程。未改变会话状态机、用户配置格式、最低 Windows 版本或现有依赖。

## 影响与验证

| 影响面 | 独立预期 | 结果 |
| --- | --- | --- |
| 图标源和 ICO | 小尺寸符号清晰，各尺寸图像范围有效 | 6 个 ICO 尺寸检查通过，16/32/256 px 预览已生成并检查 |
| 前端窗口操作 | 最大化状态正确切换；最小化/关闭调用原生动作，不直接删除会话 | 浏览器测试通过 |
| 主题与可访问性 | 深浅主题、640×400 视口、按钮标签及焦点、普通浏览器无不可用控件 | 截图检查及浏览器测试通过 |
| 前端类型/静态检查 | 无类型错误或新 lint 问题 | typecheck、lint 通过，Svelte 0 错误/0 警告 |
| 原终端规则与前端流程 | 既有输入/输出、标签和配置行为不回归 | 18 项单测、13 项浏览器 E2E 通过 |
| Rust 窗口配置/权限 | 固定工具链编译，能力名有效 | Rust 1.91.0 fmt、Clippy `-D warnings` 通过 |
| 原生无边框/缩放 | 客户区与窗口外矩形重合；四条边返回正确原生 resize 命中结果 | 通过；Windows 10 build 19044 / 真实 WebView2 |
| 原生窗口按钮/双击 | 最大化、还原、最小化和双击均改变真实窗口状态 | 通过；Windows 10 build 19044 / 真实 WebView2 |
| 原生关闭/终端 | 新关闭按钮仍等待会话回收，输入/重载/权限和配置检查通过 | 原生 10 项检查通过，关闭后进程退出码 0 |
| 普通安装包 | 重新生成应用、安装图标和许可资源，诊断功能不进入正式二进制 | 普通 features 构建通过；EXE 和安装器均含全部 6 种图标图像，未检出诊断标记 |

原生报告为 `target/native-smoke/2026-10-26T06-55-14-206Z/report.json`，截图为同目录 `native-terminal.png`。外窗口和客户区均为 1100×720，原点重合；四边命中结果分别为 `HTLEFT/HTRIGHT/HTTOP/HTBOTTOM`。最小化检测使用首次取得的 HWND 并检查进程所有权：原脚本每次重新读取 `Process.MainWindowHandle`，窗口最小化后选中了 14×14 的辅助窗口，导致误报；修正取样后 `IsIconic` 为真，未改动或放宽产品断言。真实关闭耗时 282 ms 仅为本次观察。

验证日志保存在 `target/window-chrome/`。浏览器截图为 `apps/desktop/test-results/window-dark.png` 和 `window-light.png`；原生证据保存在 `target/native-smoke/`。收尾检查未发现 Terminder/pty-fixture 进程，1420 与 9427 端口均未监听。

## 本地产物

命令 `npm --prefix apps/desktop run tauri -- build -- --locked --offline` 成功。检查了 EXE 和安装器中各尺寸图标的原始图像数据，以及 NSIS 安装/卸载图标配置和许可资源；检查结果位于 `target/window-chrome/artifacts.json`。安装包目录的 `SHA256SUMS.txt` 已同步。

| 产物 | 字节数 | SHA-256 |
| --- | --- | --- |
| `target/windows/release/terminder-desktop.exe` | 10,937,344 | `cce5de295c32ec6f3a1e42266e7a1183a140f09c26419cc189bf4598fdb3c05d` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2,611,684 | `9d96ce5d7bed3bb73deb2e6c8f73fbb546282cfaf459f23a5955968de42b4dc5` |

## 边界与恢复

Windows 11 最大化按钮悬停的系统吸附布局、跨屏不同 DPI、远程桌面及辅助技术完整矩阵仍须对应实机验收；没有以浏览器夹具声称这些功能通过。当前已知的 Windows 10 ConPTY 句柄增长问题与本次外观修改独立，仍见[完整审查记录](review-2026-10-26.md)。前端构建保留大于 500 kB 的现有体积提示。

恢复本轮修改应以 `target/window-chrome/before/` 和`target/window-chrome/changes.patch` 为依据，不能回退上一轮审查改动。新增组件、SVG、生成脚本和测试均可单独撤销；没有用户数据迁移。安装包只生成在本地，没有安装、发布或签名。
