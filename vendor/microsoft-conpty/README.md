# Microsoft ConPTY 运行组件

此目录固定携带 Microsoft.Windows.Console.ConPTY 1.25.261109001 的 Windows x64 conpty.dll 与 OpenConsole.exe。许可为 MIT，原文见 [LICENSE](LICENSE)。官方 NuGet 来源、包哈希、文件哈希、大小和已记录的 Microsoft Authenticode 签名信息见 [manifest.json](manifest.json)。

## 加载与分发

应用从可执行文件所在目录加载 conpty.dll，依赖搜索只允许该目录与 System32，不选择系统中另一份 ConPTY。OpenConsole.exe 是会话的控制台宿主，不是常驻服务。

[内核构建脚本](../../crates/session-core/build.rs)核验 SHA-256，再将两个文件复制到 Cargo profile 和 deps 目录；[打包脚本](../../scripts/package.ps1)再次核验，并将组件、清单及许可证一并放入 ZIP。构建不下载组件，仅支持 Windows x64。

用户应完整解压便携包，保持主程序与两个组件同目录。组件缺失会报告 conpty_missing；不能只复制 terminder-desktop.exe。

## 更新

升级时成对替换 DLL 与宿主，核对官方包、文件、签名和许可材料，并更新 manifest.json。随后运行普通 Shell、Windows 原生契约、ConPTY、GPUI、恢复和发布包检查，特别关注主动关闭、创建期间关闭、Job 后代、完整尾部及重复创建关闭的资源计数。

固定版本的目的在于使构建与运行使用同一组组件。清单中的签名记录是对应文件的验证记录，不表示本项目会在每次启动重新查询签名或在线更新组件。
