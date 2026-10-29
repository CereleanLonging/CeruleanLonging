# 未提交应用事务的显式关闭 V1

2026-10-08 15:02 +08:00：本组件的核心、原生界面和完整核心重启验证通过。常规 Rust 115 项、Node 4 项，严格检查及发布/调试构建通过；完整 P0–P6 仍未放行。以下结果只适用于列明的本机测试范围。

该功能处理指挥决定已经生成 `ApplyIntent`，但对应工作流事件尚未提交的中断。用户可以审阅原目标与会话证据，关闭这一次本地应用事务，使它不再阻塞后续显式操作。

关闭保留原始请求、回复与应用意图，不派发任务、不执行原指令、不自动恢复项目调度，不退回已记录的管理请求额度，也不证明供应商已取消处理或退费。原方案及用户批准保持原样。

## 入口与审阅

“主副指挥请求记录”继续采用分页摘要。只有通过 `readManagementDetail` 读取全文，且核心返回非空 `ManagementView.applicationRecovery`，详情才提供“关闭未提交应用…”按钮。

`ApplicationRecoveryReview` 包含：

| 字段 | 含义 |
| --- | --- |
| `evidenceDigest` | 绑定本次关闭所依赖的管理、工作流、关联 Job 和运行状态证据 |
| `targetKind` | `assign`、`propose`、`accept` 或 `advise` |
| `taskId` / `attemptId` / `sessionId` | 原目标关联的任务、尝试与会话；没有关联时为 `null` |
| `workflowSequence` | 审阅时工作流历史的长度 |

点击按钮后，界面再次读取详情，校验摘要与关闭候选摘要，再显示确认对话框。用户填写关闭原因后发送：

```text
abandonManagementApplication {
  operationId,
  evidenceDigest: applicationRecovery.evidenceDigest,
  reason
}
```

此处必须使用候选的 `evidenceDigest`，不能使用管理记录本身的链头摘要代替。核心会重新计算候选并比较；旧页面、旧工作流位置或变化的关联 Job 不能凭旧证据完成关闭。原因须非空，最多 8000 个 UTF-8 字节，且不能含 NUL。

界面在详情或版本变化时阻止旧确认；核心校验仍是最终依据。返回对话框不会写记录。成功后刷新当前筛选首页与详情，管理状态显示“应用事务已关闭”，完整原因显示为“关闭原因”。

## 支持范围与相邻恢复路径

| 已有证据 | 本次行为 |
| --- | --- |
| `Assign` 的 `.2` 已保存，没有对应工作流事件，有匹配的 Codex Job | 校验原会话、批准、尝试与 Job；关闭后将该 Job 投影为 `abandoned`，工作流保持不变 |
| `Assign` 的 `.2` 已保存，没有对应工作流事件，也没有 Job | 使用持久化 `.2` 中的会话及尝试证据校验，允许关闭；不会补造 Job |
| `Propose` / `Accept` / `Advise` 的 `.2` 已保存，没有对应工作流事件 | 允许关闭已经过账本目标校验的本地应用意图；即使当前命令已经过期或不适用，也不会为关闭而执行它 |
| `Wait` 的 `.2` 已保存，没有工作流事件 | 没有工作流事件是正常语义；走正常 `.3` 回执恢复及用户答复路径，不进入应用关闭 |
| 对应 `mgmt-{operationId}` 工作流事件已经存在 | 核对目标及事件，并恢复或验证 `.3` 回执；不能按未提交应用关闭 |
| 工作流已经 `Assigned`，但从未 `DispatchIntent` | 使用既有“放弃未派发尝试”路径；它写入工作流的 `AbandonUnsent` 事件，区别于本功能不新增工作流事件 |
| 只有 Job，没有 `.2`，也没有可验证的分配事件 | 本功能不覆盖，仍需后续专门恢复设计 |

当前 `Assign` 恢复只支持已建立持久化派发协议的 Codex 适配器，并要求 `provider-id` 会话身份。不能把此能力外推到其他 CLI 或桌面应用接管。

## 核心判定条件

核心要求管理账本、工作流账本及运行控制处于可验证状态。项目必须没有 Active 调度、仍持有项目的执行或验收线程、正在运行的指挥请求、另一条未解决的管理请求，以及其他待对账编程或验收操作。原应用状态必须为 `Applying`，没有成功回执，也没有先前关闭记录。

即使 Job 文件不存在，当前工作流中的 `Dispatching`、`Running`、`Cancelling`、`Unknown` 仍阻止关闭。这是提交时的实时条件，不用于判断历史关闭是否有效；后续合法执行不会因此破坏旧关闭的重放。

对 `Assign`，还要证明：

- 原应用对应的工作流事件不存在，预留尝试没有任何工作流事件或尝试证据；已经派发或已经分配的尝试不能从这条路径关闭。
- 关闭位置对应的历史中，任务已经批准且仍为 `Pending`，没有关联尝试。
- `.2` 中的项目、任务、执行者、方案摘要、工作目录、适配器、会话身份与原批准一致；预留 fence 是原任务 fence 加一。
- 若存在关联 Job，最多一个，且项目、任务、尝试、方案和执行模型均一致；关闭候选要求其处于 `Unknown` 或 `Failed`。不存在 Job 时仍须具有完整、可验证的 `.2` 证据。

`Propose`、`Accept`、`Advise` 不允许意外关联编程 Job。它们的确切目标权限已经在管理账本读取及应用意图验证时检查；关闭本身不会补做提案、收口或建议事件。

## 持久化顺序与失败边界

管理链保留既有编号约定：`.0` 请求意图、`.1` 回复或未知结果、`.2` 应用意图、`.3` 应用回执、`.4` 用户处置。

一次有效关闭按以下顺序执行：

1. 核心从当前状态重新建立候选，校验用户提交的关闭摘要和原因。
2. 在原链头之后追加 `.4`，内容为 `UserResolution::AbandonApplication`，保存原因及关闭时的 `workflowSequence` / `workflowHash`。原 `.0`、`.1`、`.2` 不改写；不伪造 `.3` 成功回执。
3. 管理请求成为终态 `abandoned`；它从待解决请求中移除，但仍保留在累计请求计数中。
4. 如果有匹配 Job，再保存 `abandoned` 状态及说明。该状态是 `.4` 的投影，保留原时间戳；没有 Job 时不创建文件。

工作流历史在本路径中没有新增事件，因此不会发生分配、派发、重新批准或收口。关闭只释放原管理应用的阻塞；后续新请求或恢复调度仍需要独立的显式操作，并继续受原批准范围、请求上限和运行状态检查约束。

管理文件发布沿用临时文件、同步写入和不可覆盖的链式发布。写入不确定时拒绝继续追加，不通过删除证据或重发模型请求进行“修复”。如果 `.4` 已发布但 Job 投影写入失败，当前调用可能返回错误；原关闭依然具有持久化依据。应重新读取或重开核心，由恢复过程补齐 Job 投影，不能把错误当成允许重新应用原指令的依据。

## 重启、历史位置与尝试 ID

关闭保存的是工作流历史位置及该位置的哈希。重启后先检查关闭证据，再核对其他管理应用，验证配置、运行记录及 Codex 关联，最后修复已关闭应用的 Job 投影。

验证旧关闭时，核心重放的是保存位置以前的历史，而不是要求当前任务始终停留在关闭时的状态。因此用户后续合法使用新请求、新尝试推进项目，不会让旧关闭凭证失效。同时，位置不能超出实际历史，哈希必须匹配该位置；全量历史中也不能后来出现原 `mgmt-{operationId}` 事件，或被关闭的预留尝试的工作流记录。

`Assign` 的 `.2` 一旦存在，就在该项目内永久保留其 attempt ID。关闭不会释放这个 ID。读取管理链会拒绝重复预留；准备新的应用、启动 Codex 以及准备分配确认都会检查预留归属。应用关闭后，即使携带原 operation ID，也不能重新占用原 attempt ID；合法后续执行必须使用新 ID。

迟到的分配确认、原指令应用和成功回执均不能把已关闭记录改为成功。普通启动恢复不会为了消除 `Applying` 而补执行缺失的工作流命令。

启动先纯核验所有未关闭 `.2 Assign` 的全量历史归属，再修复任何 `.3`。若预留 attempt 已有 Assign 或 Attempt 绑定，必须属于原 `mgmt-{operationId}` 且 principal、command、context 完全匹配；无 Job 或任务后来已回 Pending 也不豁免冲突。

## 验证状态与待办

以下为本轮实测记录：

| 检查层 | 已验证范围 |
| --- | --- |
| Rust 管理链及核心用例 | 有 / 无 Job 的 Assign 关闭、投影写入失败后恢复、新尝试推进后历史验证、Propose / Accept / Advise 关闭、Wait 排除、关联事件和忙碌状态拒绝、锚点篡改、attempt ID 保留与迟到回执拒绝 |
| 原生 UI，默认模式 | 按需详情、目标 / 任务 / 尝试 / 会话可审阅、空白原因拒绝、返回不写入、显式关闭、旧摘要与候选摘要拒绝、唯一 `.4`、零新增工作流事件、保留请求计数、完整核心重启及重复关闭拒绝 |
| 原生 UI，无 Job 模式 | 同一应用关闭流程，确保不补造 Job 且原工作流不变 |

- `reports/p2-application-closure-checks-20261008.log`：Rust 115、Node 4；同前缀 clippy 日志为严格检查，build/debug-build 日志为发布/调试构建。初次 focused 的合成 response ID 格式错误已修正，失败日志保留；最终 focused 18 项通过，包含既有相关测试，不能另加到 115。
- `durable_prepare_with_lost_ack_can_close_after_worker_releases_ownership` 在真实 Prepare 方法成功保存 `.2` 后丢弃 reply；活动期间拒绝关闭，归还持有权后关闭并重开，after_ack=0、Assign=0、Dispatch=0。使用合成 worker 通道，无真实模型。
- `reports/application-recovery-native-a1eX7T/result.json` 和 `application-no-job-native-c9B5XM/result.json`：两种原生场景均通过；每种仅新增一份 `.4`，新增工作流事件/派发 0，原 `.0–.2` 及批准不变，完整核心重启保持关闭。
- `reports/unsent-assignment-native-RLwcBv/result.json`：已有 Assigned 未派发处置回归通过；此路径新增一条工作流用户事件，与本功能的零工作流事件分开统计。
- `reports/p2-application-closure-release-20261008/release-launch.json`：发布程序正常启动/关窗，关窗后常驻核心仍可读，日常新增计划 0。三个原生场景 cleanup 均退出 0；关闭确认、关闭结果及无 Job 重开三张截图已目视检查。
- 本轮新增 10 项普通 Rust 测试，涵盖两条审查修复、投影写失败后重开、历史锚点、新尝试推进、四类本地目标及 Wait 排除。完整设计矩阵还包括更多生产入口故障组合、真实中断、容量和外部环境；不把本批子集视为全矩阵放行。

原生测试入口为 `tests/application-recovery-native.mjs`，无 Job 模式追加 `--without-job`。测试在独立的 `reports` 子目录内生成合成证据，持有自己启动的核心进程；确认核心握手成功后才启动桌面。失败清理只针对自身 ChildProcess，不按进程名清理用户程序。报告、快照、截图和管理链文件供根代理验收。

这些测试不会配置 API Key，也不会调用真实模型。真实 OpenAI 主副指挥 API 与真实 Codex 中断产生的 `.2` 恢复链尚未由本轮合成场景证明；真实生产故障验收仍待补充。`Job` 已写但 `.2` 尚未保存的前置中断、其他适配器及更完整的失败重试流程也属于后续工作。

本文件不替代整体实施方案或阶段验收表。源文件与证据摘要见 `reports/p2-application-closure-review-20261008.json`；该摘要固定本轮文件版本，后续修改应新增证据。

## 源码索引

- [应用关闭候选、提交、历史验证和 Job 修复](D:/test/CeruleanLonging/crates/core/src/application_recovery.rs)
- [管理链、终态、不可变处置及尝试预留](D:/test/CeruleanLonging/crates/core/src/management_execution.rs)
- [应用回执恢复与已关闭应用拒绝](D:/test/CeruleanLonging/crates/core/src/management_application.rs)
- [Codex 分配确认桥接](D:/test/CeruleanLonging/crates/core/src/codex_management_bridge.rs)
- [原生详情和确认界面](D:/test/CeruleanLonging/apps/desktop/src/ManagementReview.tsx)
- [原生验收脚本](D:/test/CeruleanLonging/tests/application-recovery-native.mjs)
