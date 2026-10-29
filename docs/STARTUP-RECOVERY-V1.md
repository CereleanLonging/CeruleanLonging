# Codex 启动准备中断恢复 V1

本文记录 2026-10-08 已实现的启动账本、显式关闭和界面机制。15:48 批次的组件、三种原生场景与完整核心重启验证通过，具体证据见下文。P0–P6 尚未全部验收；既有整体实施方案、全部接入对象和原验收范围继续保留。

该功能覆盖受控 Codex 启动意图已经持久化，但尚未进入管理应用或工作流分配的中断。用户审阅证据后，可以关闭这次本地启动准备。关闭保留原始记录，不发送任务、不增加工作流事件、不推进任务进度、不自动恢复调度，也不退回已记录的管理请求额度。

启动准备期间可能已经建立远端会话。没有本地会话绑定、没有 Job 或没有 Assign，都不能单独证明远端从未建立会话；本地关闭不代表远程取消或退费。

## 正常启动顺序

可信核心的 `start_codex_inner` 先检查项目可用性、原批准、任务依赖、模型选择和 attempt ID，之后按以下顺序推进：

1. 写入不可变 `StartupIntent`，协议为 `codex-durable-before-send-v1`。如由副指挥分配，意图同时绑定原管理请求及其回复链头。
2. 保存 `CodexJob`，再把项目账本交给后台执行线程。启动意图因此先于 Job 写入和线程启动。
3. 后台可建立 Codex 会话，取得后续分配所需的会话身份。
4. 关联副指挥的路径先经核心确认持久化管理 `.2 ApplyIntent`，工作线程随后保存工作流 Assign，再经核心确认管理 `.3` 回执。没有关联管理请求的可信启动路径直接按其工作流协议完成分配。
5. 后续执行仍须保存 DispatchIntent，才能走实际任务发送。启动意图自身不是执行授权，不会在重启后自动触发任务重发。

本功能处理第 1 步之后、第 4 步持久化应用或分配之前的状态。已经出现 `.2` 或该 attempt 的工作流历史时，启动摘要成为 `advanced`，使用对应的既有恢复入口。

## 三种 prepared 路径

原生回归脚本用三种独立 fixture 表达以下路径；这些模式名是测试参数，不是用户需要选择的恢复策略。

| 模式 | 已有证据 | 用户关闭后的投影 |
| --- | --- | --- |
| `managed` | 启动 `.0`、关联副指挥原请求和回复 `.0/.1`、匹配 Job；无管理 `.2`，无工作流 Assign | 启动新增 `.1`；管理新增 `.4`，终态 `startupClosed`；现有 Job 成为 `abandoned` |
| `unmanaged` | 启动 `.0`、匹配 Job；启动意图未关联管理请求，无工作流 Assign | 启动新增 `.1`；现有 Job 成为 `abandoned`；不创建管理请求或管理处置记录 |
| `missing-job` | 启动 `.0`、关联副指挥原请求和回复 `.0/.1`；没有 Job，无管理 `.2`，无工作流 Assign | 启动新增 `.1`；管理新增 `.4`；继续没有 Job，不补造执行记录 |

`unmanaged` 指没有关联管理 operation 的可信核心启动路径，不意味着界面可跳过批准或自由提供执行命令。`missing-job` 覆盖启动意图已保存、Job 写入前中断等情形；其关闭依据是不可变启动身份及跨账本证据。

三种 fixture 初始工作流均为已批准的 Pending、sequence 为 2、fence 为 0。关闭后这些值和原工作流文件不变。这只是 fixture 的具体取值；生产关闭使用实际历史锚点，不要求所有项目都处于 sequence 2。

## 账本与原始证据

启动账本位于数据目录内：

```text
codex-startups/<projectId>/<attemptId>.0.json   启动意图
codex-startups/<projectId>/<attemptId>.1.json   用户关闭
```

每个文件包含 `{ body, hash }`。`body` 保存版本、项目、attempt、序号、前一记录哈希、时间和 evidence。`.0` 的 `previous` 为 `genesis`；`.1` 必须引用原 `.0` 的哈希。关闭不会覆盖 `.0`。

启动意图保存项目、任务、attempt、批准方案摘要、完整执行模型选择、开始时间、协议、工作流历史位置及哈希。关联管理请求时，`managementOperation` 和原回复 `managementHash` 必须成对存在。关闭保存原因以及关闭位置的 `workflowSequence` / `workflowHash`。

发布过程使用独占创建的 `.pending` 文件、完整写入及同步，再通过不可覆盖的硬链接发布 `.json`，最后移除 `.pending`。写入开始后发生不确定错误会使当前账本拒绝继续写入。打开时发现残留 `.pending`、非普通文件、非法路径、链不一致、孤立关闭或重复身份会拒绝继续；不会删除证据并自动重试。这里描述的是已实现的文件发布协议，不承诺所有文件系统或断电情形已经实测。

单个启动记录限制为 64 KiB，每项目最多 1000 个启动意图。关闭仍计入历史容量，不会释放存储名额或 ID。文件哈希用于本地链和引用一致性校验，不是抵御任意本机文件篡改的外部签名证明。

## 用户审阅与显式关闭

独立的“编程启动记录”列表读取 `Snapshot.codexStartups` 摘要，因此即使没有 CodexJob 也能找到记录。摘要只包含项目、任务、attempt、关联管理请求、状态和 `intentHash`。状态含义如下：

| 状态 | 界面文字及含义 |
| --- | --- |
| `prepared` | “启动准备已记录”：尚未进入后续应用或分配；也可能仍在正常启动中，不自动判定为故障 |
| `advanced` | “启动已进入后续流程”：已有相应应用或工作流历史；不能从此入口关闭 |
| `closed` | “启动记录已关闭”：仍可查看原证据及关闭原因 |
| `legacy` | “旧记录缺少启动证据”：只有现存 Job，没有受支持的不可变启动意图 |

“查看启动详情”调用 `readCodexStartup { projectId, attemptId }`，通过 `Snapshot.codexStartupDetail` 返回目录、模型、时间、协议、批准摘要、当前工作流序号、关闭原因及 `blockedReason`。详情查询不授予执行权限。

只有核心返回非空候选 `evidenceDigest` 时，界面才提供“关闭启动记录…”按钮。点击后再次读取详情，比较身份、摘要和候选；再由用户填写原因并显式确认：

```text
abandonCodexStartup {
  projectId,
  attemptId,
  evidenceDigest: detail.evidenceDigest,
  reason
}
```

必须使用详情的候选摘要，不能把列表的 `intentHash` 当作关闭授权。候选绑定启动意图和当前链头、项目工作流位置、同项目管理链头、相关 Job 及运行记录。提交时核心重新验证这些证据，拒绝旧摘要和迟到重复命令。

原因经界面去除首尾空白后提交，须非空、不超过 8000 个 UTF-8 字节且不含 NUL；核心同样检查空白、字节上限和 NUL。返回确认框不写记录。忙碌时禁用操作；项目历史或摘要变化会使旧确认失效，用户须重新审阅。

确认框保留项目、任务、attempt、关联请求、工作目录和关闭影响；完整协议及摘要保留在详情中。成功后刷新启动详情，关联管理记录显示“启动记录已关闭”。现有 Job 使用通用“本次尝试已关闭”文案，不能据此声称所有关闭路径都曾完成分配或都发生了任务回退。

## 候选检查与 legacy 边界

核心先验证启动协议及跨账本身份。启动历史锚点必须真实存在，重放该前缀后任务应已获批准、处于 Pending、没有关联尝试，且依赖已验收。任务适配器和模型、方案摘要、项目及工作目录须与该历史批准一致。

若存在 Job，其项目、任务、attempt、方案、模型、关联管理请求和启动时间必须匹配。若关联管理请求，须绑定同项目原 Assign 回复、原任务及 `worker-codex`，且不能被另一管理请求或工作流操作占用。检查覆盖全量历史，不能只因当前任务后来回到 Pending 就认为旧 attempt 没有执行过。

可关闭候选还要求没有以下状态：

- 已关闭记录、已存在的 `.2` 应用或该 attempt 的工作流历史。
- 当前项目的活动调度、仍持有项目的执行或验收线程、运行中的指挥请求、另一条未解决管理请求。
- 项目中执行中或待对账的任务、待对账验收，或另一条活动或 Unknown 编程记录。
- 不健康的账本、运行控制或待恢复错误。

存在匹配 Job 时，其候选状态只允许 Starting、Unknown 或 Failed；但 Starting 不会绕过活动线程限制。最终是否可关闭完全由核心详情返回的候选决定。

`legacy` 缺少不可变启动来源及发送顺序证明。详情只展示现有证据，`evidenceDigest` 为 `null`，不提供关闭，不自动补造启动意图，也不根据当前代码版本、文件时间或 Job 缺字段推断过去未派发。旧记录如果具备其他完整证据，仍应走相应既有恢复路径。

无管理关联、正常返回的 Failed 原有新 attempt 继续能力保留，显式关闭没有成为其额外前置条件。来源不明的旧 Unknown 不能凭可变状态升级为新协议；具体历史协议的接纳、迁移与降级矩阵仍待完成。

## 关闭顺序及管理 .4 绑定

一次关闭按以下顺序保存：

1. 重新计算候选，检查用户摘要和原因。
2. 启动账本追加 `.1`，保存关闭原因与当前工作流锚点。这是本次关闭的权威证据。
3. 若有关联管理请求，在原回复链头后追加 `.4`，内容为 `UserResolution::AbandonStartup { reason, attemptId, startupHash }`。`startupHash` 指向启动关闭 `.1` 的链头。
4. 若原 Job 存在，保存其 `abandoned` 投影及说明；没有 Job 时不创建。

管理 `.4` 不替代启动 `.1`。恢复时必须双向校验：管理请求、项目、attempt、关闭原因、引用的启动链头，以及启动意图保存的原管理回复头全部一致。管理终态为 `startupClosed`，其原 `.0/.1` 继续保留；不补造 `.2` 或 `.3`。累计请求数不减少。

如果启动 `.1` 已保存而管理 `.4` 或 Job 投影写入失败，调用可能返回错误，但不能据此重新启动原 attempt。重读或重开核心后，在证据校验通过的前提下补齐投影。已有正确 `.4` 不重复追加；孤立 `.4`、错误反向引用或出现冲突的执行证据会拒绝恢复。

## 重启时先校验，再修复授权账本

`RuntimeHost::open` 的跨账本阶段先完成全部纯校验，再开始会写入授权证据的恢复：

1. `validate_startups`：校验全部启动身份、历史位置和双向关闭引用。
2. `validate_application_closures`、`validate_management_applications`：校验既有应用关闭及全部管理应用归属。
3. `validate_project_setups`、`validate_runs`、Windows 下的 `validate_managed_codex_links`：校验配置、运行记录及编程关联。
4. 以上全部成功后，才执行 `reconcile_management_applications`、`recover_startups`、`repair_closed_application_jobs`，分别恢复已有应用回执、启动关闭投影和应用关闭 Job 投影。

这样可以避免先给某条记录补写 `.3` 或 `.4`，再发现另一个项目或账本存在冲突。这里的顺序针对授权账本恢复，不等于声称打开数据目录绝不创建目录或调整可重建的运行投影。`recover_startups` 自身还会先验证全部启动记录，再逐条修复；用户提交单条关闭后只修复该条投影。

## 历史前缀与 attempt 永久保留

启动及关闭证据保存历史位置和该位置哈希。核验旧记录时使用保存位置的历史前缀，不要求当前任务或当前方案永久停留在启动时状态。位置超出历史、锚点不匹配、启动前已有相同尝试，或被关闭尝试后来出现应用或执行历史，都会被拒绝。

每个项目内的 attempt ID 一经启动 `.0` 保存就永久保留。关闭不释放 ID，删除可变 Job 也不释放 ID。启动入口拒绝重复 ID；管理应用准备及分配确认同时检查启动预留与原管理归属。相同管理 operation 不能拥有多个启动意图；迟到的准备或提交确认不能重新开启已关闭请求。

后续合法推进应使用新 attempt，并由用户独立发起或恢复调度，继续受原批准和请求额度约束。关闭本身不创建新尝试、不改变批准范围、不重新调用指挥模型或 Codex。

## 与其他恢复入口的关系

| 持久化位置 | 对应恢复入口 |
| --- | --- |
| 受支持的启动 `.0`，没有 `.2` 或 Assign | 本文启动关闭 |
| 管理 `.2` 已有，但对应工作流事件缺失 | 未提交应用事务关闭；管理终态仍为 `abandoned` |
| Assign 已提交，历史从未 DispatchIntent | 放弃未派发尝试；新增工作流 AbandonUnsent 事件 |
| 已派发且任务为 Unknown，有匹配原 attempt | 原会话对账；不能使用前置启动关闭 |
| 只有旧 Job，缺少启动 `.0` | legacy 只读；需要独立证据或后续设计 |

这些入口的证据要求和工作流影响不同。界面不依赖当前管理历史分页来猜测是否存在操作；无效的“核对原会话”按钮仍应隐藏。

## 本批验证与限制

常规 Rust **135 项**、Node **4 项**通过，较上一批新增 20 项 Rust 测试；fmt 和全部 target 的严格 clippy、发布及调试构建通过。日志为 `reports/p2-startup-recovery-complete-checks-20261008.log`、`p2-startup-recovery-final-clippy-20261008.log`、`p2-startup-recovery-build-20261008.log`、`p2-startup-recovery-debug-build-20261008.log`。

新增测试覆盖如下；这不是原设计全部故障矩阵已经通过的声明。

| 入口 | 本批覆盖 |
| --- | --- |
| `startup_journal.rs` 单元测试 | 记录发布、不可覆盖链、文件边界、故障点及永久预留 |
| `startup_recovery.rs` 与 `startup_order_tests.rs` 测试 | 三种前置路径、跨账本引用、历史前缀、忙碌拒绝、legacy 边界、关闭投影恢复和全校验先于修复 |
| `management_application.rs`、`management_execution.rs` | 后续跨账本冲突不提前补前一条 Wait 回执；原回复头保持，关闭后迟到应用拒绝 |
| `tests/startup-recovery-native.mjs --mode=managed` | 真实桌面界面与生产核心、关联管理和 Job 的显式关闭 |
| 同脚本 `--mode=unmanaged` | 不关联管理请求的关闭，不新增管理记录 |
| 同脚本 `--mode=missing-job` | 无 Job 的启动入口、关闭后仍不补造 Job |

实际生产启动入口在 Job I/O 失败时已保存意图、未启动 worker；重开后原 ID 仍被占用。另有混合链测试验证 StartupIntent → `.2` → AbandonApplication → 新启动/Assign → 重开，旧启动保持 advanced、旧 Job 为 Abandoned，新 attempt 及 fence 不回退。另一非法关联存在时，打开失败但合法待修复启动的管理 `.4` 不提前生成。

三种原生报告均为 `passed`，各自正常停止核心再重新启动：

| 场景 | 报告目录 | 新增启动 / 管理记录 | 新增工作流 / 派发 |
| --- | --- | --- | --- |
| managed | `reports/startup-managed-native-x2kxPk` | 1 / 1 | 0 / 0 |
| unmanaged | `reports/startup-unmanaged-native-JR3oOi` | 1 / 0 | 0 / 0 |
| missing-job | `reports/startup-missing-job-native-MPVs6Z` | 1 / 1 | 0 / 0 |

每个目录保存 `result.json`、快照、详情和截图；三份 `cleanup.json` 均退出 0。已目视检查 managed 确认框、unmanaged 关闭详情和 missing-job 重开详情。原始批准、工作流、管理原文件和请求计数保持；无 Job 路径没有创建 Job。

旧应用恢复原生回归 `reports/application-recovery-native-BF9d3l/result.json` 通过，cleanup 为 0；发布启动 `reports/p2-startup-recovery-release-20261008/release-launch.json` 通过，正常关窗后核心可读、日常新计划 0，随后只停止测试自建核心。全部测试与构建已结束。

初次 clippy 的大枚举警告以 Box 修复。两次全量测试曾在新增冲突夹具的正常请求保护处失败，最终改为先构造合法历史，再只移除夹具自己的早期回执模拟缺失；没有放宽生产规则。原失败日志 `p2-startup-recovery-clippy`、`final-checks`、`verified-checks` 保留，最后通过以 `complete-checks` 和 `final-clippy` 为准。定向修复通过日志 `p2-startup-recovery-receipt-regression-20261008.log` 不另计常规测试数。

原生脚本调用 `seed_native_startup_recovery_fixture`，使用独立 `reports` 目录、`CERULEAN_STARTUP_NATIVE_DIR` 和 `CERULEAN_STARTUP_MODE`。它持有自己启动的核心进程，握手完成后才启动桌面；失败清理仅针对自身 ChildProcess，不按名称清理用户程序。

脚本检查详情读取、空白及过长原因拒绝、返回不写、旧摘要拒绝、原始文件保留、唯一启动 `.1`、关联管理唯一 `.4`、零新增工作流事件和派发，以及完整核心重启后的原因保留和重复关闭拒绝，并保存快照及截图。

这些 fixture 不配置 API Key，不调用真实模型。真实 OpenAI 指挥调用、真实 Codex 启动中断、远端会话残留，以及生产环境故障和容量矩阵，不能由这些合成场景替代。其他 agent、CLI、Key 和桌面接管目标继续属于原项目范围；本次 Codex 子机制不会将其删除，也不代表它们已验收。

源码、二进制和报告摘要汇总于 `reports/p2-startup-recovery-review-20261008.json`。本机 main 尚未提交或推送，无云 CI；reports 被忽略，移机须单独保留。完整启动故障矩阵、旧协议审计接纳、真实连接/创建会话中断及容量仍未全覆盖；继续活动取消、副指挥纠偏、多阶段验收及原 P1/P3–P6 门槛。

## 源码索引

- [启动意图与关闭账本](D:/test/CeruleanLonging/crates/core/src/startup_journal.rs)
- [跨账本核验、关闭候选和投影恢复](D:/test/CeruleanLonging/crates/core/src/startup_recovery.rs)
- [可信启动顺序与打开恢复顺序](D:/test/CeruleanLonging/crates/core/src/host.rs)
- [管理处置和 startupClosed 终态](D:/test/CeruleanLonging/crates/core/src/management_execution.rs)
- [分配准备及提交确认](D:/test/CeruleanLonging/crates/core/src/codex_management_bridge.rs)
- [启动记录界面](D:/test/CeruleanLonging/apps/desktop/src/CodexStartups.tsx)
- [原生三模式回归脚本](D:/test/CeruleanLonging/tests/startup-recovery-native.mjs)
