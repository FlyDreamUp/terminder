# 标签内双窗格分屏（2026-10-27）

> 本文记录 2026-10-27 的实现与构建快照；当前功能和产物以[2026-10-28 嵌套分屏记录](nested-split-panes-2026-10-28.md)为准。

## 范围与行为

按已确认的第一版范围实现标签内左右/上下双窗格。默认单窗格，标签栏分屏菜单新建与当前窗格相同 Profile 的独立会话；目录沿用已有 Profile 策略及已知启动目录，不声称能读取 Shell 内最近的 `cd`。每个标签最多两个窗格，全部窗格计入原有会话总上限。重启失败后保留的结束画面同样计入前端上限，防止后端已释放会话而界面继续积累终端缓冲；原生端仍独立执行会话配额校验。

- 拖动分隔线调整比例；键盘方向键每次调整 5%，Home/End 到允许边界，Enter 恢复均分。分隔线使用可聚焦的 separator 语义和实时范围值。
- 新建左右分屏至少需要 487 px 内容宽度，上下至少需要 247 px 内容高度；每个窗格的目标最小宽/高为 240/120 px。已分屏窗口继续缩小时会限制比例，在极小空间下均分，允许用户最大化一个窗格继续使用。
- 当前窗格通过边框、文字和按下状态标记；点击终端或标题切换焦点。复制、粘贴、搜索及状态栏使用当前窗格；异步剪贴板读取完成时再次检查目标，切换窗格后不转发待处理粘贴。
- 临时最大化/还原、切换标签和调整比例保留终端实例、会话标识与缓冲。两个可见窗格都同步尺寸；隐藏窗格继续解析输出及 ACK。窗口仍最多拥有一个 WebGL context，由当前窗格使用，其他可见窗格使用 DOM。
- 单独关闭只结束对应窗格；关闭标签确认一次后处理全部窗格，`Ctrl+Shift+W` 保留关闭整个标签的含义。资源回收及输出完成后才移除视图；失败保留错误与重试入口。
- 已结束窗格可在原位置重启。创建替代会话失败时保留结束画面供重试，不重复忘记旧绑定，也不重启其他窗格。
- schema 1 保持不变：每个有效窗格的 Profile、启动目录和标签自定义名称分别保存为标签项，重启确认后恢复为独立标签。第一版不恢复分屏方向、比例或最大化状态；不新增配置字段、依赖或后端协议。

## 影响面与验证矩阵

| 影响面 / 文件 | 验证维度与断言 | 覆盖状态 |
| --- | --- | --- |
| `App.svelte`、`state/workspace.ts`：标签与窗格分离 | 左右/上下各创建一个独立会话；失败不插入空窗格；第三窗格禁用；原会话上限有效 | 浏览器 E2E 已覆盖 |
| `TerminalPane.svelte`、`terminal/view.ts`：可见性与焦点分离 | 两窗格输入隔离，搜索和粘贴跟随当前窗格，延迟粘贴不串窗格，两个可见会话都 resize | 浏览器真实 xterm + 宿主夹具及 Windows 原生验证已覆盖 |
| `PaneDivider.svelte`、样式、分屏菜单 | 鼠标拖动、键盘调整、焦点状态、最小视口、最大化还原、深浅主题 | E2E 与截图检查已覆盖 |
| 关闭与重启 | 单窗格关闭、取消、输出屏障、重复 finished、forget 失败、整标签部分关闭失败、重启准备失败后重试及保留视图的容量限制 | E2E 故障注入已覆盖 |
| 工作区与旧交互兼容 | schema 1 保存全部有效窗格、焦点映射、排除无效路径/已忘记会话；旧格式恢复为独立标签；原标签/Profile/设置/窗口行为 | 单元测试及 E2E 已覆盖 |
| 原生资源与平台边界 | 真实 ConPTY 独立输入、双会话 resize、最大化、单独关闭、页面重载及窗口关闭 | Windows 10 build 19044 / WebView2 156.0.4314.50 原生验证通过 |
| 正式平台与可访问性验收 | 物理中文 IME、Narrator、系统高对比度、跨 DPI、长时间输出及资源压力 | 本轮未执行完整人工矩阵；不作为正式平台验收声明 |

## 验证记录

| 命令 | 结果 |
| --- | --- |
| `npm --prefix apps/desktop run typecheck` | 通过，Svelte 0 错误 / 0 警告 |
| `npm --prefix apps/desktop run lint` | 通过 |
| `npm --prefix apps/desktop run test` | 21 项通过 |
| `npm --prefix apps/desktop run test:e2e` | 完整套件 27 项通过，包含最终的保留视图容量回归；分屏专项共 11 项 |
| `npm --prefix apps/desktop run build` | 通过；保留既有主包超过 500 kB 的提示 |
| `cargo fmt --all --check` | 通过 |
| `cargo build -p terminder-desktop --release --features test-support,tauri/custom-protocol --locked --offline` | 通过，诊断程序单独保存在 `target/split-pane/diagnostic/` |
| Windows WebView2 原生验证 | 12 项检查通过，含左右/上下分屏的真实 ConPTY 输入、两个会话的尺寸同步、最大化还原、单独关闭及页面重载回收 |
| `npm --prefix apps/desktop run tauri -- build -- --locked --offline` | 普通 Windows 应用与 NSIS 安装包构建通过；最终程序未检出诊断数据目录或 CDP 环境标记 |

开发验证日志位于 `target/split-pane/`，浏览器截图位于 `apps/desktop/test-results/split-*.png`。本轮没有 Rust 实现改动，Rust 会话契约通过真实原生链路验证；未重复完整 Rust 测试和长期压力矩阵。

首次从小写工作目录启动 E2E 时，Windows 路径大小写造成 Playwright 重复模块实例，测试未进入执行。统一使用文件系统规范路径后，原有及新增测试能够完整执行；没有更改依赖版本或测试断言绕过该环境问题。

后续分屏专项中，小视口用例在 `setViewportSize` 返回后立即打开菜单，尚未送达的 resize 事件按设计关闭菜单，导致焦点断言失败。用例增加等待布局帧完成的前置条件，保留原断言；之后 11 项通过。失败日志保留在 `target/split-pane/e2e-split-before-resize-wait.log`。

原生报告：`target/native-smoke/2026-10-27T09-39-44-798Z/report.json`。同目录 `native-split-columns.png`、`native-split-rows.png` 是实际应用画面；本次关闭窗口 156 ms 为单次观察，不作为性能保证。原生截图检查后修正了一处当前窗格标题的 CSS 对齐问题；该样式修正及随后加入的前端容量上限通过最终完整 E2E 复核；本轮原生报告对应这些前端收尾前的诊断构建，会话协议和原生回收实现没有变更。

资源审查另外复现了重启失败后的视图容量缺口：配置上限为 2 时，已释放原生会话的旧画面仍占一个窗格，原实现允许再新建标签形成 3 个视图。新增独立前端窗格计数后保留在上限内，仍可重试或关闭旧窗格。修复前的失败断言和日志保留在 `target/split-pane/before-view-budget-e2e-budget.log`，修复后纳入完整 E2E。

## 同日加号对齐修正

新建按钮的容器原本已垂直居中，但文字 `+` 受字体基线影响偏下。改用对称 SVG 加号，并在按钮内部使用 flex 居中，使其与相邻分屏图标对齐。菜单行为与可访问性名称保持原有定义。

本次类型检查、lint、新建菜单专项 3 项 E2E 和普通 Windows 安装包构建通过；深浅主题截图确认对齐。检查日志、修正前源文件与 `after-dark.png` / `after-light.png` 位于 `target/plus-centering/`。本次为显示修正，未重复单元测试或原生会话全量检查。

## 本地产物

以下产物已包含加号居中修正。

| 产物 | 字节数 | SHA-256 |
| --- | --- | --- |
| `target/windows/release/terminder-desktop.exe` | 10940416 | `e875bc3b5ca29ff1c1ceacabe2c52e48c42c0eefc68d51e01fa89411d48b2a7f` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2614169 | `498d8843bff8f56ce0431ae11cd577bbe058f3b899d8b938746e704de83ccea6` |

分屏首版差异（包括新增文件）保存在 `target/split-pane/changes.patch`；加号修正补丁位于 `target/plus-centering/changes.patch`，本表产物元数据位于 `target/plus-centering/artifacts.json`。应用和安装包由普通构建生成；诊断程序仅用于隔离联调。

## 恢复与限制

本轮从干净 Git 基线 `0d7e5b63c05c` 实施，可根据 `target/split-pane/changes.patch` 选择性撤销本轮文件改动；没有创建提交，也没有用户数据迁移。旧版本能读取本轮保存的 schema 1 启动信息，分屏窗格会表现为独立标签。终止的进程和已经执行的命令不能通过代码回退恢复。

既有 ConPTY 句柄增长问题仍见[工程审查记录](review-2026-10-26.md)，本轮分屏验证不替代长期资源压力与正式安装验收。
