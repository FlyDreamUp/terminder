# Terminder

Terminder 是 Windows x64 原生终端应用，使用 Rust、GPUI 和 alacritty_terminal，随程序分发固定版本的 Microsoft ConPTY 运行组件。

## 使用

完整解压便携包后运行 terminder-desktop.exe。主程序、conpty.dll 和 OpenConsole.exe 必须放在同一目录。

普通启动进入欢迎页。点击主页的「新建终端」或标签栏的「+」展开终端类型菜单，选择后创建新标签；Ctrl+Shift+T 直接创建默认终端。设置入口位于标题栏。关闭最后一个标签或窗格后返回欢迎页；关闭应用窗口才会退出。运行中的会话默认需要确认后才能关闭。

- 支持 PowerShell 7、Windows PowerShell、cmd 和自定义 Shell Profile。
- 支持多标签、标签重命名、嵌套分屏、分隔线拖动和方向焦点。
- 支持终端颜色、滚动历史、选择复制、粘贴确认、正则搜索、鼠标报告及原生文本组合接口。
- 设置可调整主题、字体、字号和历史行数；完整配置可编辑 Shell、快捷键和启动恢复。
- 工作区恢复默认关闭。开启后先确认，再按保存的布局启动新进程；取消恢复留在欢迎页。

恢复不包含终端输出或运行中的任务。显示和保存的是启动目录，不跟踪 Shell 内执行 cd 后的目录。

## 常用快捷键

| 操作 | 默认键位 |
| --- | --- |
| 新建 / 关闭标签 | Ctrl+Shift+T / Ctrl+Shift+W |
| 向右 / 向下分屏 | Ctrl+Alt+R / Ctrl+Alt+D |
| 关闭当前窗格 | Ctrl+Alt+W |
| 移动窗格焦点 | Alt+方向键 |
| 切换标签 | Ctrl+Tab / Ctrl+Shift+Tab |
| 复制 / 粘贴 | Ctrl+Shift+C / Ctrl+Shift+V |
| 搜索 / 设置 | Ctrl+Shift+F / Ctrl+, |
| 重命名标签 / 重启已结束会话 | F2 / Ctrl+Shift+Enter |
| 放大 / 缩小 / 重置字号 | Ctrl+= / Ctrl+- / Ctrl+0 |

在终端中选中文字后可用 Ctrl+C 或右键复制，复制后清除选区；没有选区时 Ctrl+C 仍用于中断，右键用于粘贴。Ctrl+Shift+C 也可复制并保留选区。复制失败会显示原因并保留选区，可再次尝试。复制选区会取消尚未确认的旧粘贴。

含换行或控制字符的粘贴默认需要再次执行粘贴确认，Escape 或失焦取消。终端开启鼠标报告时，可按住 Shift 进行本地选择。

## 构建和启动参数

运行环境为 Windows 10 21H2（build 19044）及以上的 x64 系统，需要系统图形组件与 Microsoft Visual C++ x64 运行库。便携包不包含运行库安装器。

开发需要 Rust 1.91.0 MSVC、Visual Studio C++ Build Tools 和 Windows SDK（rc.exe、fxc.exe）。

~~~powershell
cargo run --locked
cargo run --locked -- --profile cmd --cwd C:\Windows
./scripts/package.ps1
~~~

| 参数 | 行为 |
| --- | --- |
| --profile <id> | 立即启动指定 Profile 或 auto；后续新标签默认使用该选择 |
| --cwd <path> | 立即打开终端，使用已存在的绝对启动目录 |
| --help / -h | 显示参数说明 |

显式传入 Profile 或目录时跳过恢复确认。便携包输出为 target/package/Terminder-0.1.0-windows-x64.zip，并附 SHA-256 校验文件。

## 数据位置

| 内容 | Windows 路径 |
| --- | --- |
| 配置 | %APPDATA%\Terminder\native.toml |
| 工作区 | %LOCALAPPDATA%\Terminder\state\native-workspace.json |
| 日志 | %LOCALAPPDATA%\Terminder\logs\terminder.log |

配置不存在时使用默认值。保存采用文件锁、修订检查和原子替换；有效前一版本保存在 .bak。完整规则见配置参考。

## 文档

- [配置与快捷键](docs/configuration.md)
- [架构与运行机制](docs/Terminder-Design.md)
- [开发、测试与打包](docs/development.md)
- [验证覆盖和当前结果](docs/verification/README.md)
- [人工验收](docs/verification/manual.md)
- [第三方依赖与许可](docs/third_party/README.md)

桌面后端仅支持 Windows。真实输入法、跨 DPI、读屏、像素外观和干净机器运行需单独验收，自动化通过不代表这些项目已完成。
