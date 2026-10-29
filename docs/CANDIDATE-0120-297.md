# 297：0.1.20 / reader16 内部候选

2026-10-23。新候选位于 `reports/p6-windows-package-0120-20261023`，清单 SHA256 为 `155bceeb91e0640a07069bb0e17864d9ff599eae4543c6c652df6bfbc172d82a`。这次将 294 的资源页修正及 296 的 reader15/16 更新门禁修复纳入新构建；293 的 0.1.19 冻结包及先前真实模型任务均未重写。版本提升到 0.1.20，数据 reader 仍为 16，备份写入者白名单显式加入 0.1.20。

构建前冻结 1081 份构建相关源码，构建和后续检查均未出现源码漂移。`cargo fmt --all -- --check`、严格 `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`、Rust release 核心、TypeScript/Vite 生产前端、Tauri release 桌面与 NSIS 安装包通过。290 原资源矩阵及 294 真实本机 TCP 资源检查分别以 `--ignored --exact` 真正执行 1/1；读取本轮 0.1.20 包清单的 reader15/16 受控门禁为 11/11。第一次资源检查因验证器漏传真实 Git 路径而失败，最初的打包尝试因沙箱拒绝 Node 启动 rustc 而失败；失败回执保留，修正后用新标签完成检查和宿主单作业构建。

新包三项产物的摘要、实际 `--build-info`、清单、NSIS 解压记录一致，候选审查通过。实际打包的 `CeruleanLonging.exe` 各包含且只包含一份完整维护脚本字节；其中 `manage-windows-update.ps1` 与 `maintenance-window.ps1` 的 SHA256 分别为 `187d811f7eaf8e55099c27f40284a912ff75eb1c7b8ab629427813d6f06d0637` 和 `6985cedde43478693697192e9dc7326182988a9229f9aa0e2865c1d2aac79760`，与 296 修正副本相同、与 293 包的旧脚本不同。清单的 `buildInputs` 没有单列这些脚本；本轮另以冻结源码和实际 GUI 二进制的完整字节匹配核对，GUI 产物摘要已在清单中固定。

0.1.20 包装核心分别短暂启动并正常停止，对原 218 Accepted、284 Unknown、286 Accepted、295 Accepted 的项目、管理决定和详情进行回读。四份原目录及编程产物在前后逐文件保持一致，未重新规划、分配、执行或验收。另在新的隔离数据目录检查 0.1.16 旧备份、冻结的 0.1.18 备份、正规 reader16 提升与旧 reader15 拒绝、0.1.20 新备份原路径冷恢复字节一致，以及旧写入者拒绝新版备份；全部本轮核心正常停止。

本轮没有模型调用、凭证使用、安装器启动、签名、证书信任变更或密钥创建。三项新包产物实际均为 `NotSigned`，`releaseAccepted=false`。本机隔离恢复和 NSIS 载荷核对不能代替独立 Windows 的真实安装、升级、回退、卸载或 WebView2 首启。正式发布者证书与签名仍缺，其他供应商/CLI、严格桌面监督 ACK、逐笔金额与真实效率样本等也未由本轮覆盖。

权威入口：[最终结果](../reports/candidate-0120-297/candidate-ready.json)、[原包构建检查](../reports/candidate-0120-297/result.json)、[新包清单](../reports/p6-windows-package-0120-20261023/manifest.json)、[四份原数据回读](../reports/candidate-0120-297/readback-result.json)、[隔离恢复](../reports/candidate-0120-297/maintenance/result.json)、[NSIS 载荷](../reports/candidate-0120-297/payload.json)、[reader 门禁](../reports/candidate-0120-297/reader-gate-data/result.json)。
