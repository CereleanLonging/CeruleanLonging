# 296：reader15/16 安装更新门禁差异已修复，实装待客体

2026-10-23。只读核对 223 安装方案与现有包后发现：`manage-windows-update.ps1` 的控制器/事务 reader 白名单停在 14，`maintenance-window.ps1` 的 schema2 意图门禁停在 12。若按旧 223 参数直接做 0.1.16→0.1.19 更新，将在准备或维护 UI 读回时拒绝当前 reader15/16；这不是候选包或数据损坏。293 冻结 0.1.19 包内嵌的仍是旧脚本，不能把本次修复算作 293 包能力。

当前源码已将更新控制器的显式 reader 白名单扩到 16，识别 reader15 团队导入与 reader16 中转守卫，按最高现存守卫核对核心报告的 `minimumReaderGeneration` 和摘要；仍拒绝未知未来守卫、字段类型错误、旧 reader 读新数据、未完成迁移。维护窗口只放行已审阅的 15/16 事务身份，实际动作仍由控制器复核。原两份源码在 `reports/installation-preflight-296/before`，修正副本在 `after`，旧 223 报告和 293 包未改写。

冻结源包 0.1.16/reader15 与候选 0.1.19/reader16 的清单、三个归档文件逐字节摘要通过 `isolated_nsis_install.py` 的包输入校验。PowerShell 5 定向门禁 **11/11** 通过，包括 reader15 数据前向更新、reader16 数据对旧 reader 的拒绝、完整 14→15→16 守卫链、错误最小 reader/摘要与未知 reader17 拒绝；两份 PowerShell 文件语法和 `git diff --check` 通过。审查代理未发现确定的门禁逻辑错误。测试中的 Core 为受控响应，**实际安装器启动 0 次**，没有客体、升级、回滚、卸载或 WebView2 首启验收。

下一步须把这两份脚本纳入**新的内部候选**，校验内嵌字节与源码一致；不能直接重跑 293 的 create-new 冻结脚本或修改旧包。独立 Windows 客体启动后，在客体先核空白登记与标准数据目录，再按 [223 协议](INSTALLATION-ACCEPTANCE-223.md)使用 0.1.16/reader15 作为源包、新候选作为目标包，先回滚未提升数据的事务，再用全新事务提交，并核对 reader16 提升后旧 reader 拒绝与冷备份恢复。当前宿主仍有 0.1.7 登记，不能在宿主冒充干净环境；Windows Sandbox 上次在客体前报 `0x800705aa`。正式签名/证书仍缺，`releaseAccepted=false`。

证据：[包输入钉住](../reports/installation-preflight-296/preflight.json)、[reader15/16 门禁结果](../reports/installation-preflight-296/update-gate/result.json)、[当前修正副本](../reports/installation-preflight-296/after)、[293 包](CANDIDATE-0119-293.md)。
