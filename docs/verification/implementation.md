# 重开发验证记录

设计基线：`Terminder-Design.md`（用户工作区版本，2026-10-20）。

本次为 Major 架构重开发，用户已授权按方案重建。开始时旧 `src/`、Cargo 清单及锁文件已被用户删除，设计文档已有用户改动；不恢复旧实现，不改写设计文档，不自动提交。

验收范围：Windows 首发的应用、配置、会话生命周期、二进制 IPC、终端交互、打包与验证入口。macOS/Linux 保留独立平台边界，未接入的后端明确返回不支持。

实现顺序：会话规则和协议 → ConPTY/Job 后端 → Tauri 权限及配置 → Svelte/xterm → 原生契约及工程检查 → 交付证据。

## 影响与验证计划

| 路径 / 符号 | 行为与风险 | 验证入口 |
| --- | --- | --- |
| session-core / Supervisor | 独占状态、创建/关闭竞态、一次性最终结果 | 规则与受控后端测试 |
| backend/windows | 创建时 Job 归属、阻塞 I/O 取消、尾部排空 | Windows 原生契约和故障测试 |
| transport / Flow | 顺序、ACK、队列及全局配额 | 双端夹具、边界测试 |
| desktop / commands | 窗口归属、原始请求、受限能力 | 权限配置及绑定拒绝测试 |
| config | 注释、冲突、备份、失败原文件保留 | 临时目录文件测试 |
| TerminalView / UI | 字节流、IME、稳定挂载、隐藏标签及降级 | 前端测试、浏览器及原生人工矩阵 |

恢复：新实现可用 Git diff 单独审查；用户已有删除不回滚。v1 配置使用独立路径及备份，未知版本不写回。会话结束不可撤销，关闭确认会明确说明。

环境初探：WSL2 Linux Rust 1.94.0；Windows 主机 build 19044（不是 Windows 11 支持验收环境），已安装 Node 24.18.0、Rust 1.91.0 和 VS Build Tools 2022。Windows 11、IME、Narrator、跨屏、签名及干净机器安装必须单独记录真实证据。

## 原生缺陷复现与修复记录

1. Windows build 19044，宿主 stdout 被重定向时，子进程输出绕过 ConPTY。已通过 `STARTF_USESTDHANDLES` + 显式 NULL 标准句柄修正；参数/Unicode/输入/后代契约由失败转为通过。微软 node-pty 使用同一 Win32 启动规则。
2. 根进程退出后，系统 ConPTY 仍持有输出管道。已在 Draining 开始时启动独立的 console 关闭，仍保持正常配额和无损读取，期限到后才切换强制排空；尾部完整性测试由失败转为通过。
3. 本机 build 19044 每次创建/关闭残留一个 Process 句柄。已新增不启动 Shell、不使用 Job、也不经过 supervisor 的最小 `bare_conpty_creation_releases_its_handles` 对照，仍复现 71 → 75（4 次）。集成测试同样复现每次一个 Process 句柄。保留严格失败断言，不读取/修改私有 HPCON 布局，也不关闭归属未知的句柄。

初次实现按设计首发边界将产品会话入口限制为 Windows 11（build 22000+），仅 `test-fixture` 放行更早系统。2026-10-20 按用户要求，普通构建下限改为 Windows 10 21H2（build 19044）；无需诊断 feature 即可进入会话创建。Windows 10 的句柄增长问题仍保留失败，不能推断 Windows 11 无泄漏；两类系统的原生资源验收仍是发布阻塞项。

4. 发现输出 Channel 可以早于 Running 状态到达。前端现以协议窗口（256 KiB / 64 块）暂存启动输出，在收到 Running 后交给 xterm，避免设备查询应答被启动状态丢弃；浏览器夹具刻意反转两条 Channel 的到达顺序。
5. 整次粘贴先预约前端及后端字节/块额度，再发送带预约标识的有序 raw 输入。预约只在当前会话有效，5 秒未提交自动失效；I/O 错误或结果未知不重放。已明确拒绝的繁忙输入允许用户稍后重新输入。


6. 配置修复不能用损坏原文覆盖最近有效备份。回归先失败后通过：有效 `.bak` 保留，损坏原文另存 `.invalid`；备份本身也通过临时文件替换。
7. 工作区在完整序列化后检查 128 KiB 上限，超限保存不触碰原快照，避免生成下次无法读取的状态文件。回归先失败后通过。
8. Windows 环境键改为系统 UTF-16 ordinal 比较，避免 lossy 转码或通用 Unicode 大写把不同键合并。宿主在提交共享运行时前限制为 64 个排队/运行请求，关闭仍走独立路径。

## 工具链与配置定案

Windows 原生检查使用现有 MSVC Rust 1.91.0，依赖组合已编译通过，因此 `rust-toolchain.toml` 与 CI 固定 1.91.0。Linux Rust 1.94.0 为补充验证环境。Node 固定 24.18.0，npm 为 11.16.0；Tauri Rust/npm API 均为 2.12.4，CLI 2.12.3。

方案中的 NSIS `perUser` 在锁定 Tauri 版本的实际 schema 中名为 `currentUser`，已按同一“用户级安装”语义设置；`perUser` 会被构建器拒绝。原设计文件保留不改写。

## 自动化结果（2026-10-20）

| 影响面 × 维度 | 实际命令 / 入口 | 结果与边界 |
| --- | --- | --- |
| 共享规则、生命周期、协议、文件恢复 | Linux `cargo +stable test --workspace --locked --offline` | 25 项通过；Rust 1.94.0，不代替 Windows API 验证 |
| Windows 配置、文件替换、权限规则、宿主配额 | `cargo test -p terminder-desktop --release --features tauri/custom-protocol --lib --locked` | 8 项通过，含损坏配置修复保留有效备份与超限快照拒绝 |
| Windows 环境变量 | `cargo test -p terminder-session-core --features test-fixture --lib windows_env --locked` | 原生 UTF-16 大小写比较通过；Path/PATH 合并，ß/SS 及不同未配对 surrogate 不错误合并 |
| Windows ConPTY / Job 契约 | `cargo test -p terminder-session-core --features test-fixture --test windows_contract --locked -- --test-threads=1` | 7 项通过，1 项失败：build 19044 每次 ConPTY 残留一个 Process 句柄 |
| 取消 / 崩溃 | 上述 Windows 契约中的阻塞写、慢 ACK、宿主强制结束 | 均通过；强制结束测试确认根进程和后代均终止 |
| 前端协议、队列、粘贴、快捷键 | `npm --prefix apps/desktop test` | 12 项通过；控制消息运行时拒绝错版本、错绑定和异常载荷 |
| xterm 与界面 | `npm --prefix apps/desktop run test:e2e` | Edge 中 5 项通过；浏览器测试的宿主为测试夹具 |
| 真实桌面 / 原始 IPC / 权限 / 重载 / 退出 | `npm --prefix apps/desktop run test:native`，显式诊断构建 | 8 项断言通过；真实 ConPTY、Tauri 与 WebView2；程序化 composition，不等同于物理 IME |
| 工程检查 | 双平台 clippy `-D warnings`、fmt、前端 typecheck/lint/build | 通过；前端单块约 565 kB（gzip 157 kB），保留 Vite 大块提示，不声称性能达标 |
| 供应链材料 | `npm --prefix apps/desktop run notices` | 从锁定元数据及本地上游许可原文生成依赖清单和 notices；Cargo 清单为包含构建/测试依赖的保守超集 |

原生联调断言包括：启动到 Running、真实中文输入回显、过期 epoch 拒绝、未授权创建窗口拒绝、工作区元数据落盘、切换标签保留缓冲、页面重载回收旧会话、原生关闭等待回收。首次通过的关闭耗时约 723 ms，最后一轮约 760 ms；仅为单次观察值，不是 p95 性能结论。最后一轮环境为 Windows 10.0.19044 / x64 / WebView2 155.0.4283.62。

诊断构建的 `test-support` 为隔离测试目录提供配置入口，并启用内核 `test-fixture` 诊断能力。普通安装包使用默认 features，检查 Windows 10 21H2 最低 build 19044；本机 Windows 10 不再依赖诊断 feature 绕过版本检查。不会分发诊断构建，也不会让测试改写用户现有配置。

本机原始输出位于 `target/`：`linux-tests.log`、`linux-clippy.log`、`windows-clippy.log`、`windows-host-tests.log`、`windows-environment-test.log`、`native-contract.log`、`windows-core-tests.log`、`native-bare-conpty.log`、`frontend-*.log`、`browser-e2e.log` 以及 `native-smoke/*/report.json` 与截图。这些是本机证据，不进入源码版本控制。

## 仍未通过的验收门槛

- Windows 10 build 19044 / 19045 与 Windows 11 最低/较新 build 上的资源稳定性、100 次创建关闭与长时间压力；本机 Windows 10 的句柄增长必须修复，不能推断为 Windows 11 通过，也不能靠跳过断言消除。
- 实体 IME、AltGr、Narrator、高对比度、GPU context lost、跨屏与远程桌面。
- 独立参考设备上的延迟、吞吐、内存及 30 分钟负载；未提供性能提升结论。
- 设计指定的共享后端最小 Dioxus 对照尚未执行，M0 不能标记全项完成。
- 干净机器安装/升级/卸载、缺失 WebView2、离线依赖变体、发行方签名及发布渠道。

恢复方式：代码为未提交工作区变更；旧源码删除与设计文档修改属于用户原有基线，本次不回退。用户数据使用独立 v1 目录；保存保留有效 `.bak`，修复损坏配置另存一份 `.invalid`，未知版本不覆盖。关闭真实会话的副作用无法通过代码回退撤销。

## 打包过程

普通 release 使用默认应用 features，已核对 Windows x64 GUI PE 格式、应用 features 为空，以及二进制不存在 `TERMINDER_TEST_DATA_ROOT`、`pty-fixture`、诊断 CDP 端口字符串。安装包类型为 NSIS / currentUser，未签名，未执行安装或发布。

Tauri 内置下载器访问 GitHub release 超时，随后从同一官方 release 的 GitHub 资产 API 获取 NSIS 3.13 及 nsis-tauri-utils 0.5.3；同时核对锁定 Tauri CLI 源码中的 SHA-1 与 GitHub 资产 SHA-256，填入官方缓存目录后继续打包。校验记录为本机 `target/packaging-downloads/verified.json`；未降低校验或使用第三方镜像。

安装包携带项目许可、第三方原文通知、273 项锁定依赖元数据及精确源码下载位置。少数注册表包缺失独立许可文件，其固定版本上游通知由 `third_party/notices/sources.json` 记录来源和哈希；两个 npm 包仅发布 README 内的 MIT 声明，原样保留，没有编造版权人或年份。

放宽版本下限前的本机产物（`CARGO_TARGET_DIR=target/windows`；新产物见文末本次验证记录）：

| 产物 | 大小 | SHA-256 |
| --- | --- | --- |
| `target/windows/release/terminder-desktop.exe` | 10,844,672 字节 | `71f016dc0dcacf17927975498267972ee94c843d4f1e59d46a039268e972a841` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2,576,136 字节 | `55e86f816e592aa2fbef42149e364e6828a8dd107cd499242ada1ea4a561af52` |

最终 `tauri build -- --locked` 成功，安装脚本核对为 currentUser，仅包含主程序和许可/依赖材料；不含测试夹具。`Get-AuthenticodeSignature` 返回 `NotSigned`。交付检查未发现残留的 Terminder/pty-fixture 测试进程，1420/9427 测试端口均已释放。校验值另存 `target/windows/release/bundle/SHA256SUMS.txt`。


## 普通构建的 Windows 10 下限（2026-10-20）

按用户要求，将默认会话入口从 build 22000 放宽为 Windows 10 21H2 x64 build 19044；Windows 10 22H2 和 Windows 11 继续通过版本检查。19044 取自已有本机原生联调环境；更早的 Windows 10 仍拒绝，不能从 ConPTY API 可用性推断完整兼容。正式支持仍以平台矩阵验收为准。

改动覆盖 `backend::MIN_WINDOWS_BUILD`、版本错误提示、README、设计平台表、ADR、验收清单及 CI。版本判断抽为内部纯函数，以明确覆盖最低值、低于最低值、Windows 11 和非 Windows；错误码继续使用 `unsupported_windows` / `unsupported_platform`。恢复依据保存在 `target/windows10-baseline/before/`，包含本次编辑前的源码、文档和旧程序/安装包，便于只恢复本次变更。

新增回归先在旧门槛下执行：版本测试 1 通过 / 2 失败，默认 feature 原生测试返回 `unsupported_windows`（实际主机 build 19044）。调整后 3 项版本测试通过；默认 feature 原生测试使用系统 Windows PowerShell、禁用 Profile，在临时目录输出固定标记，断言退出码 0、完整尾部和预期输出。CI 单独运行该测试，避免 `test-fixture` 绕过平台检查。

| 影响面 × 维度 | 本次命令 / 入口 | 结果与边界 |
| --- | --- | --- |
| 版本下限、旧系统拒绝、Windows 11 放行、非 Windows 拒绝 | `cargo test -p terminder-session-core --lib platform_tests --locked --offline` | 3 项通过；19044 / 19045 / 22000 / 26100 放行，0 / 17763 / 19043 拒绝；版本单测不代替各 OS 实机验收 |
| 默认 features 的真实会话 | `cargo test -p terminder-session-core --test windows_default_build --locked --offline` | Windows build 19044 上 1 项通过；真实 PowerShell 启动、输出、退出，不使用诊断 feature |
| 共享规则、协议、文件恢复 | Linux `cargo +stable test --workspace --locked --offline` | 28 项通过；复用 `/tmp/terminder-cargo-home` 和 `/tmp/terminder-linux-target`，非 Windows 环境不执行原生测试 |
| Windows 配置、生命周期、原生 ConPTY / Job | `cargo test --workspace --features terminder-session-core/test-fixture --locked --offline --no-fail-fast -- --test-threads=1` | 38 项通过 / 2 项既有失败；裸 ConPTY 4 次创建关闭句柄 73 → 77，集成 12 次 79 → 91；失败断言保留，命令退出 101 |
| 格式与静态检查 | `cargo fmt --all --check`；双平台 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 通过；本次修改的 Markdown 相对链接检查通过 |

完整 Windows 回归未全绿；句柄增长仍阻塞正式发布。其他 Windows build 的实机、物理 IME、干净机器安装及长时间稳定性仍未验收。本次仅放宽普通构建的运行门槛，不声明上述问题已解决。


普通安装包已在 Windows build 19044 上执行 `npm --prefix apps/desktop run tauri -- build -- --locked` 成功生成。前端构建与 273 项锁定依赖通知生成通过；保留原有 Vite 大块提示。主程序为 x64 GUI PE，应用与 session-core 的构建 features 均为空，二进制不含 `TERMINDER_TEST_DATA_ROOT` 或 `pty-fixture`，包含新的 Windows 10 21H2 要求提示。NSIS 仍为 `currentUser`；本次未运行安装程序，程序和安装包没有嵌入 Authenticode 证书。

| 本次产物 | 大小 | SHA-256 |
| --- | --- | --- |
| `target/windows/release/terminder-desktop.exe` | 10,844,672 字节 | `c4bc61b0964eeeca755f9abfb303041512cdbbec38bff3b6bc6b948cc90bfc6d` |
| `target/windows/release/bundle/nsis/Terminder_0.1.0_x64-setup.exe` | 2,579,129 字节 | `6539149ca0556eb61e7511150eec46c6fb2167800731c1aa690e675e8cb9cfd9` |

本次日志与变更快照位于 `target/windows10-baseline/`：`windows-platform-tests.log`、`windows-default-build-test.log`、`windows-workspace-tests.log`、`windows-fmt.log`、双平台 `*-clippy.log`、`linux-tests.log`、`windows-package.log`、`artifacts.json` 及 `changes.patch`。安装包目录中的 `SHA256SUMS.txt` 已更新；`before/` 中保留旧产物与原校验文件。


## CI 原生联调的 WebView2 启动（2026-10-21）

CI 报告的空 `checks` 和 `native-smoke.mjs` 首个等待超时，定位到 CDP 端点未就绪。GitHub 的 Windows runner 以管理员身份运行；微软说明高权限宿主会忽略 `WEBVIEW2_*` 环境覆盖（含 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`），但仍接受应用通过 WebView2 API 提供的参数。依据：

- <https://docs.github.com/en/actions/reference/runners/github-hosted-runners#administrative-privileges>
- <https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security#for-an-elevated-host-app-use-appropriate-override-flags>

`test-support` 下新增显式 CDP 端口入口：端口限定为 1–65535，要求已验证的隔离数据目录，绑定 127.0.0.1；通过 Tauri 的 `additional_browser_args` 写入 API，保留锁定 Wry 的默认浏览器参数。普通构建不编译此入口。原生脚本主动清除旧的 WebView2 参数环境变量，使测试只能通过新的 API 路径连接；30 秒端点期限保持不变。失败报告增加阶段、进程退出状态及最近 16 KiB 应用日志，CI 收集原始日志。

本机为 Windows build 19044 / WebView2 155.0.4283.62，非管理员进程。移除旧环境参数后，修改前的诊断程序复现端点超时，应用日志同时显示会话已 Running；修改后可连接 CDP。完整联调还定位到测试立即发送 Enter 与 xterm 异步完成 composition 的竞态：现在等待准确的 `native-contract-中文` 输入回显，再发送 Enter，原有 `ECHO:` 中文往返断言继续保留。

本次验证：Windows 诊断及宿主单测 12 项通过（含 4 项新增端口/隔离/注入边界测试），fmt、诊断功能 clippy `-D warnings` 与前端 lint 通过。最终原生联调 8 项断言全部通过，覆盖真实 Shell、程序化中文、权限拒绝、元数据、标签、重载和退出；退出耗时 1064 ms 为单次观察。未在本机模拟管理员令牌，远端管理员 runner 仍须推送后重跑确认；这不替代物理 IME 或完整平台稳定性验收。

本次日志位于 `target/native-ci-fix/`：`before-native.log`、`diagnostic-unit-tests.log`、`diagnostic-clippy.log`、`final-native.log`、`final-lint.log`；`before/` 保留编辑前文件。最终原生报告为 `target/native-smoke/2026-10-21T02-27-17-454Z/report.json`。

普通 `tauri build -- --locked` 随后成功生成 NSIS 安装包；已核对应用及 session-core 的 features 均为空，普通主程序不含 `TERMINDER_TEST_CDP_PORT`、`TERMINDER_TEST_DATA_ROOT`、`--remote-debugging-port=` 或 `pty-fixture`。测试进程已退出且 9427 监听已释放；未安装或发布产物。普通构建日志、产物 SHA-256 和汇总保存在 `target/native-ci-fix/ordinary-package.log`、`verification.json`，安装包目录的 `SHA256SUMS.txt` 已更新。
