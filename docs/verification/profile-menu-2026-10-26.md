# 新建标签与终端类型菜单（2026-10-26）

## 行为与范围

按用户要求合并标签栏的 `+` 与类型选择箭头：只保留 `+`，点击先显示已配置的终端类型，选择后通过原 `newTab(profile.id)` 路径创建标签。选择前不发出 `prepare_session`，取消菜单不创建会话。启动时的默认终端和 Ctrl+Shift+T 的默认类型行为不变。

- 菜单保留本机可用性提示和配置管理入口；没有写死可选终端列表。
- 菜单贴近 `+` 展开，限制在窗口边界内；列表较长时可滚动。深浅主题沿用应用配色。
- 支持方向键/Home/End 导航、Enter 选择、Esc 返回按钮、Tab 离开以及点击外部或再次点击 `+` 关闭。窗口尺寸变化时关闭菜单。
- 使用菜单语义和展开状态提示，移除单独的下拉箭头及其样式。没有新增依赖，未改动后端协议、Profile 配置或会话生命周期。

## 验证

| 检查 | 结果 |
| --- | --- |
| `npm --prefix apps/desktop run typecheck` | 通过，Svelte 0 错误/0 警告 |
| `npm --prefix apps/desktop run lint` | 通过 |
| `npm --prefix apps/desktop run test:e2e` | 16 项通过，包括选择类型后创建、快捷键、取消、键盘导航、配置入口、窄窗口及原有标签/输入/窗口回归 |
| `npm --prefix apps/desktop run build` | 通过；保留现有 500 kB 主包体积提示 |
| Windows WebView2 原生流程 | 最终 10 项检查通过，菜单选择前后会话数量及 Profile ID 符合预期 |
| 普通 Windows 安装包 | 构建通过，正式二进制未检出诊断标记，应用和安装器的 6 种图标资源保留 |

浏览器夹具增加第二种终端类型，并根据请求的 Profile ID 生成对应会话，避免固定返回 PowerShell 掩盖错误选择。原生脚本同步为“点击 `+` → 断言仍只有原会话 → 选择类型 → 验证会话 Profile ID”的路径。浏览器截图位于 `apps/desktop/test-results/profile-menu-dark.png` 与 `profile-menu-light.png`；本轮日志保存在 `target/profile-menu/`。

原生最终报告为 `target/native-smoke/2026-10-26T07-36-37-331Z/report.json`（Windows 10 build 19044 / WebView2 156.0.4314.42），同目录 `profile-menu.png` 保存展开菜单的实际画面。本次关闭耗时 162 ms 仅为单次观察。

第一次原生联调在进入菜单流程前，窗口最小化断言超时，报告保留在 `target/native-smoke/2026-10-26T07-29-47-690Z/report.json`；没有页面异常。加入点击目标与窗口事件采集后复测通过，尚不足以确认第一次失败的根因。随后增加还原后的原生窗口状态/尺寸前置检查，并隐藏探测用 PowerShell 窗口，避免只依赖按钮文字变化继续操作；最终完整检查通过。保留首次失败，不以重跑通过证明不存在时序问题。

## 本地产物

普通构建命令为 `npm --prefix apps/desktop run tauri -- build -- --locked --offline`。

| 产物 | 字节数 | SHA-256 |
| --- | --- | --- |
| `target/windows/release/terminder-desktop.exe` | 10,937,856 | `9e3fe32f7db616e4a99ba49f91c4f4d83c45b957f2f6325e0c61bfccafef2de9` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2,613,944 | `cff93755a2fbea8d58e0b04b22d934cba916ed335296a7134af76dc755027420` |

## 恢复与边界

本轮修改前的文件保存在 `target/profile-menu/before/`，最终差异保存在 `target/profile-menu/changes.patch`，可独立恢复本轮交互而保留此前审查和外观修改。没有用户数据迁移。先前记录的 ConPTY 句柄增长问题与本次 UI 改动独立，仍见[工程审查记录](review-2026-10-26.md)。
