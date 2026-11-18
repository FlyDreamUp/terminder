# 配置与工作区

配置文件为 UTF-8 TOML，路径是 %APPDATA%\Terminder\native.toml。规则由 [config/model.rs](../apps/desktop/src/config/model.rs) 和 [profile.rs](../crates/session-core/src/profile.rs) 定义，读写由 [store.rs](../apps/desktop/src/config/store.rs) 实现。

## 编辑与生效

设置窗口提供主题、字体、字号和历史行数。点击“编辑完整配置”打开记事本；手动保存后点击“重新加载”。外观和快捷键立即生效，Shell 程序、参数、环境及目录用于新建会话，启动恢复选项在下次启动时生效。

缺失字段采用默认值，未知字段或未知快捷键动作报错。显式提供 profiles 或 shortcuts 会替换整张列表或表，不逐项合并。无配置文件时直接使用默认配置，打开完整配置或保存外观时才创建文件。

以下配置省略了 profiles 和 shortcuts，因此使用完整内置列表：

~~~toml
schema_version = 2
default_profile = "auto"

[terminal]
font_family = ["Cascadia Mono", "Consolas"]
font_size_px = 14.0
theme = "system"
scrollback_lines = 5000

[window]
restore_geometry = true
restore_tabs = false

[session]
max_sessions = 16
confirm_close_running = true

[clipboard]
confirm_multiline = true
max_paste_bytes = 262144
~~~

## 配置字段

| 字段 | 默认值 | 规则 |
| --- | --- | --- |
| schema_version | 2 | 只接受当前版本；其他版本禁止写回 |
| default_profile | auto | Profile ID 或 auto |
| profiles | 三个内置 Profile | 最多 64 个，ID 不重复 |
| terminal.font_family | Cascadia Mono、Consolas | 1–8 个名称，每项 1–100 字节，不含 NUL 或换行 |
| terminal.font_size_px | 14 | 8–48 px |
| terminal.theme | system | system、dark、light |
| terminal.scrollback_lines | 5000 | 0–20000；与 max_sessions 的乘积不超过 160000 |
| window.restore_geometry | true | 恢复仍在可用屏幕范围内的位置和尺寸 |
| window.restore_tabs | false | 有工作区快照时，在启动前询问是否恢复 |
| session.max_sessions | 16 | 1–16；保留的已结束窗格也占一个名额 |
| session.confirm_close_running | true | 关闭未结束的窗格、标签或窗口前确认 |
| clipboard.confirm_multiline | true | 换行或 Tab 以外控制字符要求再次粘贴确认 |
| clipboard.max_paste_bytes | 262144 | 1–262144，按原始文本的字节数检查 |
| shortcuts | 下表 | 只接受列出的动作和有效 GPUI 单个键组合 |

字体按本机已安装名称选择，并附加 Consolas、Courier New 候选。字号快捷键只临时作用于当前窗口，不改写配置。降低 max_sessions 不会结束现有会话，但会阻止超限新增。

## Shell Profile

auto 按 pwsh → windows-powershell → cmd 的顺序查找可用的内置 ID。它不会自动选择列表中的第一个自定义 Profile。两个 PowerShell Profile 默认仅添加 -NoLogo，不禁用用户 Profile，也不修改执行策略。

PowerShell 7 先查 ProgramFiles\PowerShell\7\pwsh.exe，再查 PATH 中的绝对目录；Windows PowerShell 和 cmd 从 SystemRoot\System32 查找。指定 Profile 缺失、无权限或程序不可用时会显示原因。

自定义示例；使用前将程序和目录改为本机存在的路径：

~~~toml
schema_version = 2
default_profile = "dev"

[[profiles]]
id = "dev"
name = "Development PowerShell"
platforms = ["windows"]
program = 'C:\Program Files\PowerShell\7\pwsh.exe'
args = ["-NoLogo"]
cwd_policy = "fixed"
cwd = 'D:\Workspaces'

[profiles.env]
TERMINDER_WORKSPACE = "development"
~~~

| 字段 | 规则 |
| --- | --- |
| id | 1–64 个 ASCII 字母、数字、下划线或短横线；不能为 auto |
| name | 1–80 个字符 |
| platforms | 非空，允许 windows、macos、linux；应用当前只实现 Windows 后端 |
| resolver / program | 必须二选一；resolver 为 pwsh、windows-powershell 或 cmd；program 必须为绝对路径 |
| args | 最多 128 项，每项最多 8192 字节，不含 NUL |
| cwd_policy | inherit_known（默认）、home、fixed |
| cwd | 可选绝对路径；fixed 必填 |
| env | 环境覆盖表；键最多 1024 字节且不含 = 或 NUL，值最多 32768 字节且不含 NUL；Windows 拒绝大小写等价的重复键 |

目录优先级为显式 --cwd → inherit_known 时来源窗格的启动目录 → 非 home 策略的有效预设 cwd → 用户 Home。显式目录或 fixed 目录无效时直接报错。恢复使用快照中的启动目录；不会查询 Shell 当前目录。

子进程默认获得 TERM_PROGRAM=Terminder、TERM_PROGRAM_VERSION=0.1.0、COLORTERM=truecolor，随后应用 Profile 的环境覆盖。TOML 用字符串表示环境值，没有 null 删除语法。

## 快捷键动作

省略 shortcuts 使用全部默认值；提供该表则仅绑定其中列出的动作。键位使用 GPUI 语法，最长 80 字节，不接受空值、空白或非 ASCII。Windows 会按解析后的键组合检查重复，例如 ctrl-shift-t 与 shift-ctrl-t 不能同时绑定。

| 动作 | 默认值 | 用途 |
| --- | --- | --- |
| new_tab | ctrl-shift-t | 新建默认终端 |
| close_tab | ctrl-shift-w | 关闭当前标签 |
| close_pane | ctrl-alt-w | 关闭当前窗格 |
| split_right / split_down | ctrl-alt-r / ctrl-alt-d | 分屏 |
| focus_left / focus_right | alt-left / alt-right | 左右移动焦点 |
| focus_up / focus_down | alt-up / alt-down | 上下移动焦点 |
| next_tab / previous_tab | ctrl-tab / ctrl-shift-tab | 切换标签 |
| copy / paste | ctrl-shift-c / ctrl-shift-v | 复制 / 粘贴 |
| find | ctrl-shift-f | 正则搜索 |
| settings | ctrl-, | 设置 |
| rename_tab | f2 | 重命名标签 |
| restart | ctrl-shift-enter | 重启已结束的会话 |
| zoom_in / zoom_out / zoom_reset | ctrl-= / ctrl-- / ctrl-0 | 调整终端字号 |

任何未列出的动作都会报错，不做兼容映射。Tab 和 Shift+Tab 单独绑定为终端输入；对话框打开时工作区命令暂停。菜单支持上下键、Home、End、Enter 和 Escape。

## 工作区与写入保护

工作区文件为 %LOCALAPPDATA%\Terminder\state\native-workspace.json，schema_version 为 2，上限 128 KiB。它记录窗口几何、标签名、布局树、活动位置、Profile ID 和启动目录。窗口至少 600×400；工作区最多 16 个窗格，每个标签最多 8 个，窗格 ID 唯一。

关闭窗口时保存快照。启用恢复后，普通启动先询问；传入 --profile 或 --cwd 则直接新建终端。恢复会启动新进程，取消或全部条目失效时留在欢迎页。关闭最后一个标签后再退出会保存空工作区。

配置文件上限 256 KiB。无效配置在界面显示原因并使用默认配置运行，保存外观会拒绝覆盖错误文件。其他版本、无效编码、超限或不可读文件禁止写回；损坏工作区同样保留原文件。

保存使用 .lock 文件锁、SHA-256 内容修订检查和同目录原子替换。有效前一版本写入 .bak；显式修复损坏配置时将原损坏内容保存在 .invalid，保留已有有效备份。外部编辑冲突需要重新加载后再修改。手动恢复前退出应用并保留现有文件副本。
