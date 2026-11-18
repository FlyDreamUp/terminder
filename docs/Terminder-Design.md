# 架构与运行机制

Terminder 由一个 GPUI 桌面程序和一个 Rust 会话库组成。用户界面、终端解析和操作系统资源分别有明确的所有者，全部通过进程内接口协作。

## 模块

| 模块 | 职责 |
| --- | --- |
| [main.rs](../apps/desktop/src/main.rs) | 参数、平台检查、数据目录和启动错误 |
| [native/workspace.rs](../apps/desktop/src/native/workspace.rs) | 标签、窗格、焦点、配置、恢复和退出编排 |
| [native/view.rs](../apps/desktop/src/native/view.rs) / chrome.rs | 欢迎页、标题栏、菜单、对话框和主题 |
| [native/pane.rs](../apps/desktop/src/native/pane.rs) | 终端绘制、输入、选择、滚动和会话消息消费 |
| [native/input.rs](../apps/desktop/src/native/input.rs) / text.rs | 文本框、UTF-16 与 UTF-8 边界、组合输入 |
| [screen.rs](../apps/desktop/src/screen.rs) | alacritty_terminal 网格、ANSI 解析、搜索和输入编码 |
| [bridge.rs](../apps/desktop/src/bridge.rs) | UI 与会话内核之间的有界邮箱、适配线程和退出等待 |
| [layout.rs](../apps/desktop/src/layout.rs) | 布局树、删除折叠、比例和方向邻居 |
| [config](../apps/desktop/src/config/model.rs) | 当前配置、工作区模型和受保护的文件写入 |
| [supervisor.rs](../crates/session-core/src/supervisor.rs) | 会话状态、请求校验、限流、清理和线程所有权 |
| [backend](../crates/session-core/src/backend/mod.rs) | 有界 I/O 队列、后台读写和 Windows 资源实现 |

~~~mermaid
flowchart LR
  Workspace --> Pane
  Pane --> Screen[Screen / alacritty_terminal]
  Pane --> Bridge
  Bridge --> Supervisor
  Supervisor --> Backend[Windows backend]
  Backend --> ConPTY[conpty.dll / OpenConsole.exe]
  Workspace --> Store[配置与工作区文件]
~~~

## 会话所有权

一个窗格持有 Bridge，一个 Bridge 拥有自己的有界输入邮箱和适配线程。适配线程持有 SessionHandle，通过 close 或 view_failed 请求结束会话；清理完成后调用 Supervisor.forget 移除元数据。

Runtime 拥有全部适配线程。Supervisor 在独立线程上串行处理会话状态，每次启动可创建一个短期创建线程；Windows 后端持有 Job、进程、伪控制台、管道及读写线程。构造失败、显式关闭和应用退出均有对应释放路径。

标签删除只释放所属窗格，不在 UI 线程等待进程或线程退出。关闭最后一个标签保留 Runtime 和窗口。应用窗口关闭时停止会话、后台保存快照，等待适配线程及 Supervisor 结束后再移除窗口。

## 状态与边界

常规状态顺序为 Preparing → Starting → Running → Draining → Closing → Finalizing → Finished；主动关闭或错误可提前进入清理。Finished 表示后台资源完成回收，UI 可继续保留输出并提供重启。

请求校验 owner、session_id 和 view_epoch，防止失效绑定操作其他会话。输入序列连续，重复的最后一包只在内容完全相同时按幂等处理；resize 使用递增修订号。未知结果不自动重发输入。

输出以字节块传递，终端解析器消费后才发送累计 ACK。OutputEnd 的序列和字节数必须等于实际消费前缀；输出完整性和进程退出码独立记录。视图失败会释放未确认输出的预算，后台继续回收进程。

| 资源 | 上限 |
| --- | --- |
| 会话 | 16；配置可进一步降低 |
| 一个标签的窗格 | 8 |
| 单个输入 / 输出块 | 16 KiB / 32 KiB |
| 后端输入预算 | 512 KiB，整次粘贴先预约 |
| UI 输入邮箱 | 1 MiB、128 项 |
| 后端待发输出 | 1 MiB、128 块 |
| 单会话未确认输出 | 256 KiB、64 块 |
| 全局输出预算 | 16 MiB |
| 终端网格 | 每边不超过 1000，总单元格不超过 250000 |

控制请求等待最多 3 秒；准备、创建、排空和清理分别受状态期限约束。清理超时保留诊断并继续回收，不通过提高队列或无限重试掩盖失败。具体期限集中在 Supervisor 和 Bridge。

## Windows 后端

后端只加载程序目录中的固定 conpty.dll，依赖搜索限定该目录和 System32。OpenConsole.exe 是每个会话的控制台宿主。版本、来源、哈希和许可证见 [组件说明](../vendor/microsoft-conpty/README.md)。

进程在挂起状态创建，ConPTY 和 Job 在恢复执行前完成绑定。Job 负责进程树；主动关闭终止 Job，再等待退出、取消阻塞 I/O 并释放伪控制台。自然退出保留真实退出码，并在有界期限内消费输出尾部。

Windows 命令行按参数内容引用，环境变量使用 Windows 大小写比较规则。程序与工作目录必须是存在的绝对路径。窗口尺寸、输入预约、绑定和缓冲区边界在进入相应资源层前验证。

## 原生界面

Workspace 管理布局和模态状态；Pane 管理单个终端网格。标题栏使用 GPUI WindowControlArea 对接 Windows 拖动与系统按钮；逻辑键盘焦点位于零尺寸锚点，避免全窗口焦点命中区域拦截标题栏鼠标事件。

普通启动渲染欢迎页，主页只保留一个「新建终端」下拉按钮。它与标签栏的「+」共用终端类型菜单，选择后才创建标签和会话；菜单状态记录触发入口，贴近对应按钮弹出，空间不足时向内避让或翻到按钮上方。菜单支持鼠标、方向键和 Enter 选择，点击外部或 Escape 收起并恢复焦点。Ctrl+Shift+T 直接创建默认终端。每个窗格有独立关闭按钮，活动窗格通过边框和标题底色区分；终端菜单与搜索浮层不改变网格可用尺寸。标题使用裁切，避免混合 UTF-8 文本省略造成边界问题。

文本输入经 EntityInputHandler 处理，TextBuffer 在 UTF-16 范围和 UTF-8 字节边界之间转换。输入编码遵循终端当前模式；OSC 52 禁用。粘贴限制大小、移除 ESC，并按 bracketed-paste 模式封装。

## 配置与恢复

配置和工作区使用独立文件，只接受当前 schema。解析、动作和键位冲突集中在配置模型校验，绑定阶段消费有效配置。文件写入采用锁、修订比较、临时文件刷新和原子替换，并保留恢复副本。

快照只保存布局和启动环境。恢复重新解析当前 Profile 并创建新进程；不能恢复输出、命令或进程状态。详细字段和失败行为见 [配置参考](configuration.md)。

## 验证边界

规则测试覆盖配置、文本、布局和终端解析；生命周期测试使用可控后端验证时序；Windows 契约测试覆盖真实进程与资源；原生窗口检查验证 GPUI、ConPTY 和文件保存的协作。覆盖范围见 [验证说明](verification/README.md)，物理输入和显示设备见 [人工验收](verification/manual.md)。
