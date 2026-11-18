# 第三方依赖与许可材料

Rust 依赖版本以 Cargo.lock 为准。Microsoft ConPTY 独立于 Cargo 依赖图，版本与来源以 [组件清单](../../vendor/microsoft-conpty/manifest.json)为准。项目许可证与上游许可原文属于分发材料，不随说明文档重写。

## 生成

~~~powershell
./scripts/release-notices.ps1
~~~

脚本读取 Windows x64 的 Cargo metadata，生成 target/release-notices/THIRD-PARTY-NOTICES.txt 和 dependency-inventory.json。清单包含构建及测试依赖的保守超集，不表示每项均链接进正式程序。package.ps1 将两份文件放入便携包。

每个包优先使用发布包根目录中的 LICENSE、COPYING、COPYRIGHT、NOTICE 或 Cargo 清单声明的 license_file。缺少这些文件时，按包名和版本匹配 [notices/sources.json](notices/sources.json)中的补充记录，并校验其 SHA-256。缺项或哈希不符会使脚本失败。

## 补充来源

每条记录包含名称、版本、来源、文件和 SHA-256，可附 note 或 declaration_only。原文必须保持字节不变，.gitattributes 禁止该目录的换行转换。

| 条目 | 记录中的来源限制 |
| --- | --- |
| convert_case 0.4.0 | 补充来自后续上游提交，不是发布版本中的原始文件 |
| simd_helpers 0.1.0 | 补充来自后续上游提交 |
| mac 0.1.1 | README 中的许可声明，标记为 declaration_only |
| seahash 4.1.0 | 发布包 Cargo.toml.orig 中的许可声明，标记为 declaration_only |

生成成功证明所需材料存在且与记录哈希相符，不补足上述记录之外的许可证据。具体来源、提交与差异以 sources.json 为准。

## 依赖维护

依赖变更应检查包名、来源、锁定版本及许可表达式，重新生成清单。只为当前依赖图实际缺少原文的包添加补充，保存确切来源和哈希。删除不再使用的补充前，核对生成器和依赖图；不得借用其他版本或其他包的许可文本而不说明来源。

## ConPTY 分发材料

便携包包含 conpty.dll、OpenConsole.exe、CONPTY-LICENSE.txt 和 conpty-manifest.json。构建与打包都核对组件哈希。原始许可证和组件来源维护方式见 [ConPTY README](../../vendor/microsoft-conpty/README.md)，不应仅根据 Cargo 清单判断整个发布包的组成。
