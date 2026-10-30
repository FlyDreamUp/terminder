# 当前窗格继续分屏（2026-10-28）

## 范围与决策

在已完成的双窗格和加号居中实现上增加嵌套分屏，保留之前的工作区改动。本次修改前的源文件、增量补丁、检查日志及前一版可运行产物保存在 `target/nested-split/`，未创建提交或发布安装包。

- 标签栏菜单拆分当前窗格；每个窗格标题栏也提供独立分屏菜单。“向右分屏”和“向下分屏”各均分当前区域，旧窗格留在左/上，新窗格获得焦点，沿用来源 Profile 的已知启动目录策略。
- 每标签最多 8 个窗格，所有标签仍共享原来的会话上限；已忘记原生绑定但因重启失败保留的终端画面继续计入前端预算。
- 用二叉树表示方向、用户比例和叶子 PaneId，终端组件仍按 PaneId 平铺于同一父容器。树仅计算位置，拆分或折叠不重新挂载已有 xterm；防止组件卸载被解释为视图丢失而触发会话回收。
- 按实际渲染字体的字宽/行高计算 40 列 × 10 行门槛，并计入标题栏、内边距、滚动条。只检查当前窗格在还原布局内的尺寸；最大化不会绕过限制。分隔线边界考虑两侧所有后代，空间不足时保留会话并临时分配尺寸，窗口放大后恢复用户比例。
- 终端挂载区放到 padding 内的独立容器，使 FitAddon 测到实际可用面积；测量 `.xterm-screen` 的网格和公开 cols/rows，不读取 xterm 私有服务。此 DOM 契约绑定当前 xterm 6.0.0，依赖升级须复核。新增区域的 viewport 留白使用当前主题背景。
- 任一级分隔线均支持拖动、未修饰的方向键、Home/End 和 Enter 均分，提供唯一的目标区域、方向和实时比例。方向聚焦按屏幕邻接位置选择，不依赖窗格创建顺序；边缘不循环跳转。
- 关闭窗格等待输出屏障及原生 forget 成功后折叠父节点；其兄弟子树填满父区域，其他区域保持原位置，焦点转移到该子树内最近的窗格。失败保留画面和重试入口。重启只替换原叶子，不改比例或其他会话。
- 最大化保留树及比例。最大化时分屏成功后还原布局；空间不足时保持最大化。切换当前窗格、搜索、复制、粘贴和状态栏延续原有归属规则。
- 工作区继续使用 schema 1：保存每格的启动信息，重启确认后恢复成独立标签；不恢复树、比例、进程或输出，不涉及工作区迁移。

新增动作使用原来的快捷键配置、冲突校验和作用域路由：

| 动作 | 新配置的默认值 |
| --- | --- |
| `split_right` / `split_down` | `Ctrl+Alt+R` / `Ctrl+Alt+D` |
| `focus_left` / `focus_right` / `focus_up` / `focus_down` | `Alt+ArrowLeft` / `Alt+ArrowRight` / `Alt+ArrowUp` / `Alt+ArrowDown` |
| `toggle_pane_zoom` | `Ctrl+Alt+Z` |
| `close_pane` | `Ctrl+Alt+W` |

`Ctrl+Shift+W` 仍关闭整个标签并确认全部窗格数量。已有显式 `[shortcuts]` 表按原逻辑原样读取，不自动合并新键，不改变用户省略动作的语义。已有配置需要手动加入新动作才启用其快捷键；菜单不依赖这些绑定。IME/AltGr 与编辑框仍遵循原有的按键分流。

## 影响面与验证矩阵

| 影响面 / 入口 | 行为断言 | 覆盖 / 限制 |
| --- | --- | --- |
| `state/layout.ts` | 递归仅替换目标叶子；局部折叠/整棵兄弟子树扩展；重启保留几何；后代最小尺寸；小窗口非负面积与原比例恢复；空间邻格 | 6 项纯布局单测通过 |
| `App.svelte`、`TerminalPane.svelte` | 3/4 格嵌套时已有 xterm DOM 对象仍连接，无额外 start/view_failed，旧输出保留，来源会话正确 | 浏览器真实 xterm E2E 通过；原生结果见下文 |
| `PaneDivider.svelte`、`terminal/view.ts` | 子分隔线只 resize 两个后代，父分隔线 resize 全部后代；放大/缩小、字号变化、40×10 门槛 | E2E 通过；原生尺寸检查见下文 |
| `SplitMenu.svelte`、CSS | 全局/本地入口、独立 ID、鼠标与键盘菜单、空间理由、深浅主题、焦点边框和“当前”、图标对齐 | E2E 与截图检查通过；Narrator/物理高对比度未验收 |
| 关闭、重启和失败路径 | 局部折叠保持其他区域；展开兄弟子树仍保留缓冲；取消、重复 finished、输出延迟、forget/close/prepare 失败、原位重启 | 原有与新增 E2E 覆盖；会话内核没有改动 |
| 键盘作用域、资源上限 | 方向聚焦按几何位置；快捷键不输入 Shell；区分关闭窗格/标签；最大化分屏用还原尺寸；8 格及全局预算 | E2E 通过；已有快捷键作用域单测通过 |
| 配置与工作区兼容 | 保持显式映射及冲突检测；schema 1 保存 4 格、旧启动信息恢复、忽略不可恢复项 | Rust 配置测试、前端单测及 E2E 通过 |
| 原生协议、资源回收 | WebView2 + 真实 ConPTY 输入隔离、子树 resize、最大化/还原、局部关闭、重载与窗口关闭 | Windows 10 build 19044 / WebView2 156.0.4314.50 原生验证通过；既有 ConPTY 句柄增长失败单列 |
| 正式平台验证 | 不同 Windows 版本、物理 IME、跨 DPI、屏幕阅读器、长时输出/资源压力、安装卸载 | 本轮未完成完整人工矩阵，不作为正式发布验收 |

## 自动化记录

命令使用项目锁定的 Windows Node 24.18.0 与 Rust 1.91.0，Cargo 采用仓库缓存离线运行。脚本及逐项日志见 `target/nested-split/run.mjs`、同目录的 `.log` / `.json`。

| 检查 | 结果 |
| --- | --- |
| `npm --prefix apps/desktop run typecheck` | 通过，Svelte 0 错误、0 警告 |
| `npm --prefix apps/desktop run lint` | 通过 |
| `npm --prefix apps/desktop run test` | 27 项通过 |
| `npm --prefix apps/desktop run test:e2e` | 完整套件 35 项通过，其中分屏 19 项（嵌套新增 8 项）；随后补充的 Home/End 极限 10 行断言定向通过 |
| `npm --prefix apps/desktop run build` | 通过；保留既有主包超过 500 kB 的提示 |
| `npm --prefix apps/desktop run test:notices` | 6 项通过 |
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets --features terminder-desktop/test-support --locked --offline -- -D warnings` | 通过 |
| `cargo test --workspace --features terminder-desktop/test-support --locked --offline -- --test-threads=1` | 桌面 14 项通过；内核 14 项通过、1 项既有失败；该失败阻断后续集成套件，另行独立执行 |
| 独立生命周期 / 协议 / Windows 契约 / 默认构建测试 | 生命周期 10、协议 1、真实默认 Shell 1 项通过；Windows 契约 7 项通过、1 项既有句柄增长失败 |
| 原生诊断构建 / WebView2 smoke | 通过，13 项检查；真实 4 窗格独立输入、所有后代尺寸、几何焦点、比例保留及局部折叠均通过 |
| 普通应用 / NSIS 安装包 | 离线构建通过；普通程序中未检出诊断数据目录/CDP 环境标记（UTF-8 / UTF-16） |

完整 Rust 命令未通过：`backend::windows::tests::bare_conpty_creation_releases_its_handles` 报告句柄 `65 → 69`。独立执行后续套件时，`repeated_create_close_leaves_no_worker_growth` 同样复现句柄 `71 → 83`，增长项为 Process 句柄。这些问题已在[此前工程审查](review-2026-10-26.md)记录，本次未修改 ConPTY 内核，也未跳过或放宽断言。不能据浏览器或原生交互通过声称资源稳定性已完成。

首次分屏回归有 1 个测试仍按旧固定像素门槛在 900×600 窗口期待两个方向都可用。实际字体门槛正确禁用了向下分屏；将用于键盘菜单导航的视口改为 900×720，并保留 480×350 双方向禁用、字号变化和最大化不能绕过门槛的独立断言，之后专项 19 项及完整 35 项通过。

补充极限尺寸断言时，测试原有的 `sessionIds.sort()` 原地修改了会话创建顺序，导致新增断言误取另一窗格的 28 行。改为排序副本，保留目标窗格恰为 10 行的 Home/End 断言后通过；失败日志留存 `target/nested-split/e2e-bounds-before-order-fix.log`，最终结果为 `e2e-bounds.log`。此修正只影响测试，不改变已验证的生产代码。

原生报告为 `target/native-smoke/2026-10-28T03-36-58-851Z/report.json`，另复制到 `target/nested-split/native-report.json`。环境：Windows 10 build 19044 x64、WebView2 156.0.4314.50。真实应用截图 `screenshots/native-nested-four.png`、`native-nested-collapse.png` 及浏览器深浅主题截图保存在 `target/nested-split/`。窗口退出本次观察为 149 ms，仅是一次测量，不作性能承诺。原生脚本正常结束，测试进程已退出。

## 本地产物

以下为嵌套分屏首版的构建快照，包含加号居中和主题留白修正。后续关闭窗格 X 对齐修正及当前产物见下一节。

| 产物 | 字节数 | SHA-256 |
| --- | --- | --- |
| `target/windows/release/terminder-desktop.exe` | 10942976 | `5dd6130d021d8659d9c974e711fc7921c447bfa56cbe3dac1cd8bddb488a7246` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2619459 | `7269a8c1110ab993bf88d94217615327f9e5b5767015ff543951c0bbb24b9646` |

元数据及前端资源校验值见 `target/nested-split/artifacts.json`；普通版与本次原生诊断构建使用相同的 `index-DdmplIzM.js`、`index-D1YPQ9KA.css` 前端资源。

## 同日关闭窗格 X 对齐修正

关闭窗格按钮原已使用 flex 居中，但内部文字 `×` 的可见笔画受字体基线影响偏下。将其改为对称 SVG 叉号，与相邻分屏/最大化按钮共用图标样式。此次只修改关闭窗格图标，不改变按钮可访问名称、点击区域和关闭逻辑。

本次类型检查（0 错误/0 警告）、lint、3 项既有 E2E（四格布局、关闭后局部补位、窄窗口与浅色主题）均通过；深浅主题截图确认图标居中。此次为显示修正，未重复 Rust 或原生会话检查，上文原生报告对应此修正前的代码。修正前源码、截图、检查日志和前版产物保存在 `target/pane-close-centering/`。

普通构建已经完成代码编译，但复制到原 `target/windows/release/terminder-desktop.exe` 时 Windows 返回拒绝访问，因此完整 `tauri build` 命令失败。未终止用户进程或强制覆盖旧文件；核对新编译的 `release/deps/terminder_desktop.exe` 包含 `index-03a1cKNp.js` 修正资源且不含诊断标记后，将其复制到独立目录，并使用 Tauri 官方 `bundle` 命令成功生成 NSIS 包。当时原路径的旧程序/安装包未更新，修正版位于下表路径；下方复查记录说明原路径的后续同步，未执行安装。

| 当前修正版产物 | 字节数 | SHA-256 |
| --- | --- | --- |
| `target/pane-close-centering/windows/release/terminder-desktop.exe` | 10942976 | `d8feafb605d7680d4c63d2dfbb74d6f548bdd26db4e20b5c9137816baec65c68` |
| `target/pane-close-centering/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2618917 | `08a37fbb83912a94440290dd9ffe1aa9b0c7e270f3571df74807f6c6b7812f10` |

构建失败日志、独立打包成功日志与二进制校验记录均保存在 `target/pane-close-centering/`，可通过 `changes.patch` 选择性撤回本次图标改动。

## 截图复查与常用产物同步

用户反馈 X 仍偏下。读取用户提供的截图后，在浏览器测试夹具中对照复现了旧文字 `×` 的基线偏移；修正版 SVG 在悬停时没有该偏移。复查深浅主题以及 100%、125%、150% deviceScaleFactor，共 6 种组合，三个按钮的 SVG 中心均与按钮中心一致（相同一行的 y 中心均为 105.5 CSS px）。这验证当前样式的浏览器栅格化效果，不替代真实跨显示器 DPI 验收。复查脚本、报告、原字形对照和修正版截图保存在 `target/pane-close-recheck/`；本轮没有追加产品代码改动。

同时确认常用 `target/windows/release/terminder-desktop.exe` 仍含旧 `index-DdmplIzM.js` 资源（SHA-256 `5dd6130d021d8659d9c974e711fc7921c447bfa56cbe3dac1cd8bddb488a7246`），独立目录修正版则含 `index-03a1cKNp.js`。进程只读查询未返回正在运行的 `terminder-desktop.exe`，无法据此断定用户截图对应的具体运行路径，因此不把推测写成已确认的运行版本。

已经备份并同步常用路径的程序和安装包，逐文件校验与修正版一致：

| 常用路径 | SHA-256 |
| --- | --- |
| `target/windows/release/terminder-desktop.exe` | `d8feafb605d7680d4c63d2dfbb74d6f548bdd26db4e20b5c9137816baec65c68` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | `08a37fbb83912a94440290dd9ffe1aa9b0c7e270f3571df74807f6c6b7812f10` |

元数据位于 `target/pane-close-recheck/artifacts.json`，旧常用路径产物备份在同目录 `previous-build/`。未执行安装、终止用户程序或修改用户配置；安装版须由用户运行更新包并重新打开应用后使用修正。

## 恢复与交付边界

- 本次开始时工作区已有双窗格及加号修正；`target/nested-split/before/` 保存当时相关文件，`baseline.patch` 保存当时 Git 差异，`changes.patch` 是本次相对该快照的增量。只选择性撤销本次差异，不整库回退或覆盖先前工作。
- 前一版普通程序和安装包保存在 `target/nested-split/previous-build/`。应用工作区文件没有迁移，旧版仍能读取其 schema 1 启动信息。
- 新增可选快捷键动作在旧版中属于未知动作。若曾在配置中加入这些动作，降级前备份配置并只移除上表 8 个新动作映射；原有显式表没有自动改写，不需此步骤。新版本创建的默认配置也包含这些新动作，降级时同样处理。
- 代码/安装包恢复不会复活已关闭进程或撤销终端命令；本次验证使用隔离数据和测试 Shell，没有修改用户配置、关闭用户会话或执行安装。
- 原生诊断程序另存于 `target/nested-split/diagnostic/`，不作为可分发产物；最终普通构建检查诊断环境标记不被带入。
