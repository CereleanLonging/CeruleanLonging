# 327：完整 API Host 的旧包业务回读

2026-10-24。327 限定兼容验收已通过，结果见 [final-result.json](../reports/api-host-compatibility-327/final-result.json)。继承 324/326 两类旧包冷恢复与当前 reader 关联结果，本轮补齐 011 交接明确留下的旧 315 API Host 业务语义缺口；正式验收仍未完成。

本轮不改原 324 基线绑定，不重做真实模型任务、不重新读取网页。325 已从用户打开的 A6API 账单详情取得一条网关 ID；缺少提供方对历史正文 ID 的映射，`actualMoney=null` 与 284 Unknown 保持。

新测试夹具通过正式空根冷备份、审阅及 reader16 晋级，复用完整 `RuntimeHost` 的配置、合成主指挥计划、用户批准、合成副指挥分配。它实际经过两次持久化分配确认、两轮合成 API 回复和受保护文件工具，将 `result.txt` 提交。新增回调只存在于测试构建，返回合成回复后不进入 HTTP；原协议与磁盘格式不变。

本轮状态限定为 Submitted。它没有新做独立验证或主指挥 Accept，不能替代原 324/326 已通过的 Accept，也不能代证真实 API 的版本兼容。为正常退出，夹具先断言普通 Submitted 会阻止停机，再沿用 280 的正式用户 `RetireSubmittedPlan` 流程结束本版，保留原交付未验收状态与历史前缀；不会删除任务或把它改成 Accepted。夹具为正常 ApiClient 构造保存、加载并删除自己的无效测试凭证；用户的真实凭证不参与。

首次检查为 5 passed / 1 failed / 12 ignored。新夹具误把普通 Submitted 当成可停机状态，`can_shutdown` 正确拒绝；失败现场和日志保持。按既有 280 机制修正夹具后，`run-checks-fixed.py` 只续接新增夹具及修订相关检查和未执行的 Clippy/fmt/固定程序构建，不重复原五项通过检查。`run-fixture.py` 分别负责一次性的 prepare/export/old/current，原进程和失败记录保留，不自动重启或重发。

修正批次为 7 passed / 0 failed / 4 ignored，严格 Clippy、fmt 与固定测试程序构建均通过。连同两次明确选定的导出/回读用例，去重共有 14 个 Rust 用例取得通过证据，12 个 ignored 没有本轮通过记录；不能说历史全绿。旧 280 用例本轮未重跑。驱动另有 15 项离线检查通过；前版将工作流摘要与完整审批摘要混用的问题已在实际旧 Host 启动前修正，原 13 项记录和前版脚本保持。

旧 315 固定为 0.1.22 / reader16；清单 SHA256 `f1724261e037ae23f7b3e64ba77432d0afc2fba16c1e781ba897bc7bd0dcd8b7`，核心 SHA256 `ecec0eca9fe81eeb094f79e5570b6653e44da2e2db70934ac008da5b401fd70b`。它只在新隔离 case 中读取作业列表、API 作业详情、完整批准方案、任务列表及详情，随后正常停止；不恢复旧 case、不改绑定。当前 reader 再读原记录，验证两轮网关旁路关联保持。

四个实际阶段已全部通过：prepare、export、old、current。五类旧 Host 业务回复与导出全等，44 个数据文件、26 个目录、4 份旁路证据以及 1 个项目文件在当前 reader 再打开后仍完整一致，连允许重建的投影也没有变化。case 内共 10 份进程回执：连接前 status 的 exit4 为预期，其他 9 份 exit0；另有原 PID 17016 的正常 stop 与原句柄退出证明，不应误算为第 11 份进程回执。原进程都已终态，勿重跑导出、恢复或旧核心。

本轮只新增测试构建的运输回调和验收夹具/脚本，生产路径、磁盘格式及 reader16 保持。4 份源码及独立快照见 `reports/api-host-compatibility-327/source-fixed.json`；原 324 的 29 份源码快照保持。原 324 偶发基线失败的根因仍未确定，本轮不同夹具的成功不解释该历史失败。

完整正式门槛仍以 `reports/acceptance-gates-318/review.md` 为准。317/319/322/324/326 尚未纳入 315 包；签名、独立 Windows、剩余供应商和桌面差异、金额合同与真实效率样本等不能由本项关闭。账户用量无法可靠实时读取，95% 收尾规则继续有效。
