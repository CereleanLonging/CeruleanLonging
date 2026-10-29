# OMP 持久记录与工作流桥接

21:24 更新：OMP Windows受控进程已实现，创建时原子加入Job、固定二进制摘要并锁定文件、隔离profile与显式密钥环境。真实OMP握手/自然退出回归零模型prompt、剩余进程0；三个显式离线进程/宿主终止测试及233项工作区测试通过，严格clippy通过。见 [OMP-OWNED-PROCESS-V1.md](OMP-OWNED-PROCESS-V1.md)。尚待RuntimeHost/UI、产品凭证选择、审核工具代理和真实编程；完整P0–P6未通过。以下为历史批次。


2026-10-09。本批承接 [OMP 执行协议](OMP-WORKER-CONTRACT-V1.md)。实现文件为 `crates/core/src/omp_journal.rs`、`omp_journal/completion.rs`、`omp_execution.rs` 及相应测试。桥接接收可信宿主预先建立的独占连接，尚未暴露给 IPC 或 RuntimeHost 的产品任务入口。

## 已实现的事务边界

每个 attempt 使用单独目录与生命周期内独占的 OS 文件锁。派发意图、prompt ACK、取消意图、取消 ACK、关联终态按严格次序保存，最多五条事件。事件绑定批准修订、任务、尝试、围栏、会话、连接、控制器请求、供应商、模型和提示摘要；记录为有大小限制的严格 JSON，前向链校验，不保存消息正文或思考内容。

每次写入先独占创建 pending 文件、写入并同步，再以不覆盖的硬链接发布，最后移除 pending。任何写入错误都会使当前写入者不可继续使用。重开遇到 pending、缺项、多余项、错误哈希或跨范围记录即拒绝；已有意图绝不授权重发。这证明的是当前文件系统上的进程终止恢复，不等于整机断电、磁盘控制器或跨平台文件系统已验收。

工作流先保存 DispatchIntent，再保存 OMP 意图，两者完成后才能发送 prompt。ACK 保存后才进入 Running。取消可先于 ACK：先保留未知状态、保存取消请求，再发送一次 abort；后到的 ACK 不撤销取消。只有关联 terminal 和新的静止状态同时满足，才进入结果映射。中途断连、错误结果或产物缺失保持 Unknown；不以旧 ACK 证明进程当前仍在运行。

完成时先保存 OMP terminal，再保存包含原 ExecutionReceipt 和 ArtifactManifest 的 completion.json，最后向工作流 Submit。恢复只使用已保存的原回执和产物哈希，并重新核对文件未改变；禁止在缺失原产物记录时重新捕获当前文件并当作原产物。取消结果可从对应持久终态对账为 Cancelled。错误结果不能提交，Submitted 仍须独立检查和 chief 验收。

发送意图写完但实际发送前取消，会保存明确 NotStarted 观察。可信宿主可显式调用 resolve_unsent_cancellation，将该取消关闭为 Cancelled，并保留原始未发送记录。它不代表远端曾运行或返回取消回执。只有意图或 ACK 的任务不能用此入口关闭；不退请求预算、不复用原 attempt、不自动恢复任务。

## 批准与权限

桥接验证完整计划批准、原 attempt/controller-request 绑定、指定 worker/provider/model、当前分配状态及请求上限。货币预算尚无可核算的执行计量时明确拒绝派发。模型超时沿用批准的执行期限。当前 WorkerTransport 仍要求工具目录为空，不能据此宣称具备真实编程工具能力。

连接对应的批准二进制、进程生命周期、凭证、网络和 OS 权限必须由下一步产品宿主建立。本地哈希链用于完整性和关联检查，不构成对同一操作系统账户恶意改写全部文件的密码学认证。未接真实凭证，未进行真实模型调用；能力目录继续未验证。

## 验证与剩余工作

定向测试使用真实临时文件账本和离线双向 RPC 对端：检查两份意图先于网络请求、完成提交、ACK 前取消、丢 ACK、供应商错误、缺少产物，以及重开后提供全新连接仍不发送任何字节。恢复测试覆盖原产物、后改产物、缺失原记录、残留 pending 和被改记录；只保留意图、已有 ACK 与明确未发送三种情况分别处理。

派发 journal 的 synced/published/committed 三个边界有实际受控子进程终止测试，确认锁释放、原记录字节不变以及不重发。completion 文件和工作流提交之间目前通过文件状态夹具与正常关闭重开模拟，尚无该边界的真实进程终止矩阵；不与前三个实际终止点混为一谈。

后续按以下顺序推进：将 worker 接入固定摘要的受控进程与独立 profile；接本机凭证和审核工具代理；接 RuntimeHost 的异步任务、用户取消及恢复入口；完成真实 OMP 编程、独立验收和故障矩阵。首批其他 API/CLI、Qoder/WorkBuddy 已有会话、并行集成、效率和 P6 安装升级范围保持。完整 P0–P6 未验收。

## 本批结果（21:05 +08:00）

工作区日志 reports/p3-omp-journal-workspace-20261009.log：233 项常规 Rust 通过、36 项真实调用或专用辅助进程入口显式忽略；其中新 journal 6 项、桥接 4 项。随后扩展完成路径，实际启动批准的独立校验子进程，证明校验前不能 Accept、校验后文件被修改仍拒绝 Accept、原文件恢复后才可 Accept 并重开保持；该扩展后 reports/p3-omp-bridge-acceptance-recheck-20261009.log 的 4 项桥接测试全部通过。新增 artifact_validator 辅助入口为 ignored 并由父测试明确调用；当前全套忽略项因此为37，未将后一次定向测试数量重复加进233。最终 fmt 和 all-targets/custom-protocol 严格 clippy 通过。

保留失败日志：初版桥接夹具遗漏检查所需 RunCommands 权限；新增独立校验夹具遗漏 SnapshotPolicy。均在计划验证阶段被拒绝，修正夹具后通过，没有放宽产品约束。早期 journal 实际终止测试误读仍加锁的 writer.lock，改为只比对证据文件；重跑通过。

完整工作区中既有 verification_details_bind_real_stages_and_reject_corrupt_evidence 通过，但原间歇 ledger_recorded=false 的根因仍未证实。真实模型调用0；本批未重跑真实OMP只读探测、前端/Node、原生界面、发布或安装。证据索引 reports/p3-omp-journal-review-20261009.json；其中源码/文档副本仅覆盖所列文件，不是完整构建归档。测试临时目录已自动清理，实际终止矩阵依据日志和测试源码，未保留独立现场目录。
