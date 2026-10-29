# 328：0.1.23 保留未解回归的内部候选

当前状态（2026-10-24 06:25）：**构建、冻结、包一致性与独立终审完成，正式验收仍未通过**。原 session 99780 已正常终态；使用新版 verify-resumed-2 / publish-handoff-resumed-2 完成收口，见[016交接](../handoffs/2026-10-24/handoff_2026-10-24_016_062457+0800.md)。实际 manifest SHA256 为 `0be8d61c0515452762a1e439412da63e46539506fc2e3c9e10857084bb98a355`，新包三个 EXE 本次实读均为 NotSigned，签名/版本/清单冲突为零；未安装。

原 workspace 为1105 passed / 1 failed / 274 ignored，第一次精确诊断再次失败，第二次增强诊断单项通过但未触发原生句柄 helper。仅补齐原来未执行的20个普通与3个文档目标，新增204 passed / 20 ignored；严格 Clippy、fmt、前端类型检查和构建通过。跨次去重1310个用例有通过证据、294个仅忽略，**不代表单次全仓全绿，也不证明共享冲突根因已修复**。三代源码归档、两次失败、测试EXE和29文件/29目录现场均保留。后续只读[330排查](../reports/candidate-0123-328/omp-followup-static-330/followup.md)排除了第一次精确诊断中的实际还原写句柄路径，仍未定位占用者，不外推至原workspace失败。

以下为原候选计划及其范围；完成事实以本次闭合回执和独立终审为准，旧verify/publish成功模板不得执行。329查询功能仍为独立待合入提案，不属于此冻结包。

2026-10-24。整合 316 安装资源验收器、317 交接缓存、319 分发构建输入身份门禁、322 索引准备优化、324 A6API 响应头旁路留证、326 新基线根正式准备，以及 327 旧包完整 Host 回读测试增量。正式验收仍为 `false`，本文件不代替最终回执。

已先核对 [327 交接](../handoffs/2026-10-24/handoff_2026-10-24_012_040927+0800.md)和[独立源码审阅](../reports/candidate-0123-328/source-review/review.md)。保留 315 / 0.1.22 包、全部历史失败和其他模型成果。六个版本文件原件已另存，准确升至 0.1.23，reader generation 保持 16；备份 writer 列表只追加 0.1.23。冻结 1,128 个文件，相对 315 有 36 个变更、18 个新增，无删除。

本轮执行 `reports/candidate-0123-328/run.py`：格式、SQLite 许可、319 构建输入门禁、全 workspace 普通 Rust 回归（单测试线程）、严格 workspace/all-targets Clippy，随后通过现有正式 package 链生成 NSIS。每阶段原进程退出、捕获日志关闭后才写回执，任一步失败即停，先查原现场；不得因观察超时重启原进程。

新包单独冻结到 `reports/p6-windows-package-0123-20261024`。按实际产物验证版本、build-info、五资源、七项 NSIS 载荷、源码 ZIP 全成员、GUI 三份内嵌维护脚本、原生导入、315→328 reader16 关联以及当前证书公开元数据与签名/清单一致性。离线签名审阅不等于吊销新鲜度或正式发布者验收，不新建密钥或改变信任。

不重做 324/327 的成功导出、旧 Host 或恢复；310 原 11 项 reader 门禁只在函数摘要不变时继承。316 的 20 项与 22 项、319 的 16 项与 3 项已测逻辑证据按摘要继承，本次仍检查新构建输入和实际新包载荷。忽略用例不计通过，也不使用全局 `--ignored` 重跑固定目录的一次性用例。

324 原 `terminal.accepted` 偶发失败仍未定位。本次通过不会被描述为原问题已经修复，若复现保留新诊断与原回执。327 曾把 Submitted 任务误认为可直接停机的夹具错误已有独立解释，与324原问题分开保留。

本轮不调用真实模型、不读取真实凭证、不安装或改信任。325 网页回读已经完成，无需用户继续保持浏览器前台；旧正文 ID 与网关 ID 的提供方映射、金额合同仍缺，`actualMoney=null` 和 284 Unknown 保持。其余正式门槛见[318 完整清单](../reports/acceptance-gates-318/review.md)。无法可靠读取实时账户用量，95%收尾规则继续有效。

完成状态以 [candidate-ready-resumed-2.json](../reports/candidate-0123-328/candidate-ready-resumed-2.json) 及 [独立终审](../reports/candidate-0123-328/final-independent-review-resumed-2.md) 为准；状态为 `internal-candidate-with-unresolved-regression`，`releaseAccepted=false`。原315包与证据并列保留。
