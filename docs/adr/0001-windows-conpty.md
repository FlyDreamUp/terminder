# ADR 0001：Windows 会话所有权与 ConPTY 后端

状态：实现选择已记录；普通构建下限为 Windows 10 21H2 x64（build 19044），Windows 10 / 11 发布门槛仍需完成原生验收。2026-10-20。

## 问题与证据

设计要求用户 Shell 获得运行机会之前加入会话独占 Job，同时关闭不能被输出 ACK 或阻塞 I/O 锁阻挡。上游 portable-pty Windows 实现的 `WinChild::do_kill` / `WinChildKiller::kill` 使用 `TerminateProcess` 终止根进程，公开 `spawn_command` 不接受 Job List 属性；`ConPtyMasterPty` 以共享 `Mutex<Inner>` 管理 console 与 I/O 句柄。仅使用这些公开接口不能提供设计第 6 节所需的完整契约。

已核对的上游源：

- <https://github.com/wezterm/wezterm/blob/main/pty/src/win/mod.rs>
- <https://github.com/wezterm/wezterm/blob/main/pty/src/win/conpty.rs>
- <https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session>
- <https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute>
- <https://learn.microsoft.com/en-us/windows/console/closepseudoconsole>

以上代码检查说明复用缺口，不等同于对 portable-pty 的实测性能结论。

## 决定

正式 Windows 实现仅保留 `session-core/backend/windows` 一套后端，直接使用 `windows-sys` 调用必要的 ConPTY、Job 与同步 I/O 取消 API。Unix 后端在后续阶段单独实现，不以 Windows 适配为由重写所有平台 PTY。

- `CreateProcessW` 同时使用 `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` 和 `PROC_THREAD_ATTRIBUTE_JOB_LIST`，不事后按 PID 加入 Job，不启用 breakaway。
- Job 设置 `KILL_ON_JOB_CLOSE`，宿主所有内核句柄默认不可继承。
- 每会话 reader/writer 各一个受管线程；创建线程完成后退出，关闭时单独调用 ClosePseudoConsole，常态 2 条专用线程，创建/关闭至多 3 条。
- supervisor 串行拥有可变会话状态；读写 worker 只拥有 I/O 和有界队列。不在锁内进行 OS I/O、Channel 提交或进程等待。
- 取消使用 `CancelSynchronousIo`，并持续检查 worker 退出，避免取消发生在阻塞调用之前的竞态。
- 旧系统 ClosePseudoConsole 可能阻塞，故保留独立控制路径、排空 reader、1.5 秒 Job 终止和总 3.5 秒 I/O 取消时点。不能清理的资源保持 Finalizing。
- Finished 的判定包含主进程退出、Job ActiveProcesses 为零、线程 join 和句柄释放；普通标签切换不改变 OS 生命周期。

## 验证与恢复

跨平台规则测试覆盖迟到创建、关闭/退出竞态、绑定、输入和 ACK。`windows_contract` 覆盖真实 ConPTY、UTF-8/尾部、参数、目录、环境、resize、慢 ACK、后代 Job 和重复关闭句柄计数。`windows_default_build` 另以默认 features 验证版本检查和真实 Windows PowerShell 会话，避免诊断 feature 绕过检查。测试必须在 Windows 执行；CI Server runner 和本机 Windows 10 不能替代完整的 Windows 10 / 11 支持矩阵，build 19044 的句柄增长仍是发布阻塞项。

代码恢复可按 Git diff 回退本次重建，不自动还原用户此前删除的旧实现。v1 用户数据与二进制回退独立；终止进程和执行命令无法回滚。

## 未闭合的 M0 门槛

真实 Windows 10 / 11 WebView2 的 IME、GPU、权限故障注入、长时性能、干净机器安装，以及共享后端的最小 Dioxus 对照仍须依据实际证据判断。当前选择不构成 M0 全项通过，不声称 Tauri 比 Dioxus 更快或更轻。对照仅覆盖单终端、标签切换、桥接与关闭，不进入第二套产品 UI。
