# Applying 应用事务恢复：下一阶段设计

日期：2026-10-08。状态：**原始设计归档；15:02 已完成首版实现及列明的组件验收，完整故障矩阵仍有待办**。实际机制和证据见 [APPLICATION-RECOVERY-V1.md](APPLICATION-RECOVERY-V1.md)。

本文件保留实现前的依据、边界和完整目标矩阵，正文的“建议/应”保持历史语境；不能把设计要求自动算作通过。当前结果以 V1 文档和新报告为准。`Assigned` 未派发尝试关闭见 [UNSENT-ASSIGNMENT-V1.md](UNSENT-ASSIGNMENT-V1.md)，其证据与本组件分开统计。

## 1. 要解决的中断

管理请求已有 `.0` 请求、`.1` 响应和 `.2 ApplyIntent`，但目标工作流事件 `mgmt-{operation}` 尚不存在。当前状态为 `Applying`。可能已经有 Codex job 和会话绑定，但还没有 `Assign`；也可能没有 job。

当前普通放弃要求没有应用事务，`AbandonUnsent` 又要求工作流已经 `Assigned`，因此这一窗口缺少用户可完成的处置路径。目标是追加用户关闭证据，让用户在审阅后恢复后续工作，同时保留原请求、响应、应用意图和预留身份。

**关闭不是成功应用、远程取消、费用撤销或原请求重试。** 不得删除 `.2`，不得伪造 `Assign`、`DispatchIntent`、`AbandonUnsent` 或远程回执来推进状态。

## 2. 完整支持范围与实施顺序

最终覆盖所有合法 `ApplicationTarget` 的恢复路径。可以先实现 Codex Assign 的两条路径再补其他本地应用，但先后顺序不缩减最终范围，不能把后续类型默认为不支持。

| 情况 | 应有处理 |
| --- | --- |
| `.2 Workflow::Assign`，有 Codex job，无 Assign/Dispatch 历史 | 审阅完整绑定和持有权后，用户显式关闭应用；保留 job 并修正其投影。 |
| `.2 Workflow::Assign`，无 job，无 Assign/Dispatch 历史 | 同样允许有证据的本地关闭；不为了关闭而制造 job，必须保留 `.2` 预留的 attempt 身份。 |
| `.2 Workflow::Propose`，无对应工作流事件、无关联执行 | 允许关闭尚未应用的本地方案事务，保留原方案响应；后续由用户重新请求或配置。 |
| `.2 Workflow::Accept`，无对应工作流事件、无关联活动执行 | 允许关闭本地收口事务；不改写已有提交与验收，后续使用新的主指挥操作收口。 |
| `.2 Workflow::Advise`，无对应工作流事件 | 允许关闭尚未应用的本地建议；不制造已经给出建议的事件。 |
| `.2 Wait`，无工作流事件 | 这是正常表示方式，应按现有无工具 Wait 应用回执恢复，再进入答复流程，不能仅凭“无事件”当作异常关闭。 |
| 原操作事件已存在、`.3` 缺失 | 先核对事件与 `.2` 一致并修复应用回执；若为 Assigned 且从未 DispatchIntent，使用已实现的未派发尝试关闭。 |
| 原 attempt 已有任何 DispatchIntent，包括后来读回 NotStarted | 必须处理原执行证据，不允许通过本功能重置为未开始。 |
| job 存在但尚无 `.2` | 属于另一个启动中断窗口，需要独立设计；不能借此功能直接清除未知执行。它仍是最终恢复覆盖清单中的待办。 |
| 残留 `.pending`、链损坏、绑定冲突或项目账本无法完整恢复 | 保持拒绝继续；不能将缺失文件或读取失败解释为“没有执行”。 |

未来接入其他 CLI、API 或桌面会话时，需要各适配器提供等价的持久化先后约束。Codex 当前协议的证明不能直接移植到 Qoder、WorkBuddy 等接管会话。

## 3. 能够声明的事实

当前 Codex 生产链路顺序为：

1. 创建线程并取得会话绑定；此时尚未提交任务输入。
2. worker 请求 Prepare，host 持久化 `.2` 后确认。
3. worker 写入工作流 `Assign`。
4. worker 请求 Commit，host 核对实际磁盘事件并持久化 `.3` 后确认。
5. 持久化 `DispatchIntent` 后，才允许调用 `turn/start`。

可核对代码：`async_codex.rs::managed_assign`、`async_codex.rs::run`、`codex_execution.rs::dispatch`、`adapters/src/codex.rs::create_worker/start_turn`。

在完整日志已恢复、项目独占持有权已回收、上述协议得到遵守的前提下，没有该 attempt 的 Assign/Dispatch 历史可以证明工作台未通过这条受控链路派发任务。不能据此声称远程会话不存在、外部程序从未运行、没有计费或用户没有在其他入口操作过。界面应表述为“关闭尚未写入工作流的本地应用事务；原证据保留”。

仅凭 job 的 `Unknown`、`Failed`、`Abandoned`，或者 job 文件不存在，都不足以作出上述判断。

## 4. 提交前的实时核验

新增用户命令建议名为 `AbandonManagementApplication`，接收 operation、审阅摘要和原因；不得提供任意 principal、目标命令或修改证据的入口。

必须在同一 host 写锁和项目账本持有权下完成检查与提交：

- 管理日志和工作流均健康；存在合法 `.2`，未 Applied，未有用户 resolution；目标为支持的 Workflow 类型。
- 项目没有 Active run；用户先暂停或确认已进入 Attention。
- 不存在该项目的活动 Codex、verifier 或管理请求；断连但仍保留持有权的对象也算活动，不能通过从 UI 隐藏它来解锁。
- 不存在其他待对账执行、验收或未决管理操作。不能直接复用会拒绝所有 Unknown job 的 `require_available`；应仅为经过精确绑定核验的目标 job 开例外。
- 完整历史中不存在 `mgmt-{operation}`。Assign 还要检查同一项目内该 attempt 的全部历史，不能只看当前 TaskRun，也不能仅检查预期 operation 名。
- Assign 的实时候选要求对应任务 Pending、没有当前 attempt，并核对 `.2` 的完整 Attempt 与批准方案、worker、revision、workspace、adapter、会话身份及预期 fence。
- 有 job 时，要求该 operation 只关联一个匹配 job；项目、任务、attempt、revision、worker 均一致，拒绝错绑、重复绑定及矛盾的终态。允许被完整证据解释的 Unknown/Failed，而非信任状态字符串。
- 无 job 时仍执行全部日志和绑定检查，不将 job 缺失视作充分条件。
- 原因非空、无 NUL，并沿用现有 8000 字节上限。

审阅摘要应包含管理链 head、项目工作流 head、应用目标、目标 job 的当前审阅数据和项目运行记录；活动持有权另外实时检查。已有管理详情的 `evidenceDigest` 仅代表管理链，不能单独覆盖工作流变化。可新增独立的 `applicationReviewDigest`，避免破坏普通放弃/答复的摘要语义。

新 IPC 命令应加入 `RuntimeHost::handle` 的 pre-poll 排除项。读取候选和提交关闭都不能先触发一次调度，再假定用户审阅的状态仍有效。候选计算不得调用 `snapshot()` 或 `management_context()`，避免从 Snapshot 构建反向递归。

## 5. 不可变证据与崩溃恢复

建议扩展 `UserResolution::AbandonApplication`，写入现有 `.4 UserResolution` 槽位：

- `.4.previous` 继续链接原 `.2` 的链 hash；`.0–.2` 原样保留。
- 保留 `application`，`applied` 保持 false，不生成 `.3 Applied`。
- 原因必须保存。建议同时保存关闭时的工作流锚点 `{ sequence, hash }`，空日志使用显式 genesis 表示，具体字段在实现时定稿。
- 状态可以新增明确终态，或复用 `Discarded` 并增加 resolution 类型；详情必须区分“未应用请求放弃”和“应用事务关闭”。`resolved()`、列表计数及后续操作资格须一致。
- 先持久化 `.4`，再修正目标 job 为 Abandoned，并给出“应用准备已关闭、未重新派发”的说明。没有 job 时不创建投影。

若 `.4` 提交后、job 更新前崩溃，启动应根据合法 `.4` 和跨账本绑定修复投影。若 `.4` 尚未提交或只有未完成发布文件，不能提前把 job 当作已关闭；沿用当前失败封闭的日志恢复规则。

`record_applied` 当前没有关闭 resolution 的检查，扩展后必须显式禁止关闭后补 `.3`。`reconcile_management_application` 也必须先识别关闭证据：合法且无关联事件时仅核验，不继续应用；若关闭证据与既存事件冲突，应拒绝恢复，不能自动补回执抹平矛盾。所有迟到 ACK 和可信 continuation 都必须经过这道检查。

### 5.1 重放不能绑定“当前状态”

提交时检查与永久证据校验必须分开：

- 提交时可以要求任务当前 Pending；重放旧 `.4` 时不能继续要求当前任务 Pending。用户后续新 attempt 运行、提交或完成后，旧关闭记录仍须可读。
- 不能要求当前 revision 永远等于旧 `.2` 的 revision；应核验其历史批准和历史绑定。
- 若保存 workflow head，重放时核对它仍是完整日志中的合法前缀，**不能要求它等于最新 head**。必要的原状态验证可在该前缀重放，不能拿最新投影替代。
- job 的 `state`、`updatedAt`、message 会因投影修复变化，不能要求它们永久等于关闭时审阅值；项目、operation、task、attempt、revision、worker 等身份关联必须保持一致。
- 对原关闭 operation/attempt 的禁止复用是永久约束，应检查完整历史，包括锚点以后的事件。旧关闭记录后若又出现原 operation 的应用事件，应拒绝恢复。

## 6. 预留 attempt 的防复用

这是有无 job 两路都必须解决的独立问题：`.2` 已包含 Attempt，但没有 Assign 时，domain 当前基于历史 Assign 的唯一性检查看不到它；无 job 时，job map 也无法阻止复用。

因此，应将管理日志中的 Assign 目标作为项目范围内的持久化 attempt 预留记录。关闭后仍保留这项 tombstone，不能通过删除 job、换 operation 或重开核心复用旧 ID。

建议提供一个共享核验函数，查询同项目所有 `.2 Assign` 的 attempt 与所属 operation：原应用尚未关闭时，只允许它自己的合法 continuation 使用该预留；已关闭后任何入口都拒绝使用。覆盖 `start_codex_inner`、managed bridge 的 Prepare、通用 `apply_management_decision_inner`，并在启动交叉校验时发现冲突。不能只在 UI 或一个 start 方法中检查。

此类关闭没有实际 Assign，所以 fence 保持 F；新 attempt 真正写入 Assign 时应为 F+1。与“已经 Assign 后再 AbandonUnsent”的场景不同，不能为了封存预留 ID 而伪造一次分配来消耗 fence。

## 7. 用户恢复闭环

1. 用户暂停项目；详情显示原管理响应、应用目标、匹配 job/会话绑定、可证明的范围和关闭原因输入。
2. 核心提供当前审阅摘要；用户明确确认关闭应用事务。
3. 核心重新核验并只追加 `.4`，修复必要 job 投影；任务原有状态、提交、验收均不因本操作被伪造或清除。
4. 操作完成后 run 保持 Paused/Attention，不自动生成新管理请求、worker、远程取消或重试。
5. 用户显式恢复；新请求使用新 operation，Assign 使用新 attempt。关闭的旧管理请求仍计入历史和管理请求额度，不返还或重置额度。
6. 后续新尝试执行及再次重开仍正常；旧请求和 `.2/.4` 可通过历史详情审计。

Plan/Accept/Advise 复用相同的用户关闭框架，但资格检查和后续动作按目标类型定义。特别是 `poll_planning_results` 当前自动消费 `ui-plan-*` 的 Ready/Applying：必须明确保留哪些受核验的本地继续行为，或对恢复中的 Applying 改成显式继续。至少要保证先读后写竞争时旧摘要被拒绝，不能把已经应用的事务显示为成功关闭。暂停持续运行不等于停止这个独立的计划消费流程。

## 8. 精确代码落点

以下为当前函数位置，行号可能随实现变化，应以函数名检索：

| 文件与函数 | 下一阶段改动 |
| --- | --- |
| `crates/core/src/management_execution.rs`：`UserResolution`、`validate_resolution`、`statuses`、`review`、`resolve` | 新 resolution 及其格式校验；允许 `.4` 跟在 `.2` 后；保留应用目标；状态、详情、原因投影一致。提供关闭证据及预留 attempt 的只读查询。 |
| 同文件：`record_applied` / `validate_receipt` | 禁止关闭后写 `.3`，包括迟到提交及重复调用。原 Reply/Discard 的合法行为保持明确。 |
| `crates/core/src/management_resolution.rs` | 独立的应用关闭候选和实时授权核验；不能直接放宽普通 discardable。候选、详情、提交复用同一核心检查。 |
| `crates/core/src/management_application.rs`：`reconcile_management_application` | 重开时验证 `.4` 与历史、处理冲突；不能重新执行已关闭目标或补写成功回执。无 job 的记录也必须验证。 |
| 同文件：`apply_management_decision_inner` | 阻止关闭后 continuation；执行共享 attempt 预留检查。 |
| `crates/core/src/async_codex.rs`：`observed_state`、`load` | 工作流目前无法单独证明新的管理关闭；增加经交叉验证的管理证据来源，或由 host 做第二遍投影修复。不得把 mutable Abandoned 当依据。 |
| `crates/core/src/codex_management_bridge.rs`：`prepare_managed_codex_assignment`、`commit_managed_codex_assignment`、`validate_managed_codex_links` | 拒绝迟到 ACK；完整核验目标与 job；禁止旧预留 attempt 再绑定。 |
| `crates/core/src/host.rs`：`open`、`start_codex_inner`、`handle` | 按项目与管理日志完成交叉验证后才开放服务/调度；新 IPC 不预先 poll；所有 worker 启动检查预留身份。 |
| `crates/core/src/project_planning.rs`：`poll_planning_results` | 明确 Applying 的自动本地继续与显式处置之间的规则，验证读取后应用竞争。 |
| `crates/domain/src/lib.rs`、`crates/domain/src/project_setup.rs` 及导出契约 | 用户命令、候选/摘要/详情字段；不开放任意 workflow command 或执行角色。 |
| 桌面管理详情、历史和 job 展示 | 审阅/原因/确认流程、状态区分、过期提示、显式恢复入口。不能把关闭按钮当作重试按钮。 |

`observed_state` 若保留现有签名，可以让 host 在所有账本加载后、开放调度前做第二遍经过验证的 job 修复；若改成接收管理日志，应统一复用同一纯核验函数。不能只在运行时改 job，而漏掉重开恢复；也不能在校验失败前将投影当作成功关闭对外返回。

## 9. 验收用例

所有新增 fixture 首先使用 synthetic channel、合成管理响应和独立 reports 目录，不需要 Key 或真实模型调用。可复用 `management_application.rs::simulated_worker` 与双 ACK 测试。

| 用例 | 必须证明 |
| --- | --- |
| Prepare 已写 `.2`，随后丢失 ACK | 先调用 `prepare_managed_codex_assignment` 成功，再丢弃 reply；`after_ack=0`、Assign=0、Dispatch=0。退出活动持有权后可关闭。仅在写 `.2` 前丢消息不覆盖本用例。 |
| 有 job 和无 job 两路 | 都有合法关闭路径；无 job 时不造 job；原 `.0–.2` hash 不变，只增加合法 `.4`。 |
| `.4` 提交后、job 保存前崩溃 | 重开修复 Abandoned；不产生原 Assign、Dispatch、模型调用或补 `.3`。 |
| 未提交/未完整提交 `.4` | 不提前解锁未知执行；保留不确定证据，不删除残留文件来“通过”。 |
| 旧 operation、新 operation 携旧 attempt | 所有生产启动、Prepare、通用 apply 入口均拒绝；无 job、删除 mutable job 后、重开后仍拒绝。 |
| 新 operation、新 attempt | 显式恢复后可继续；关闭不增加 fence，下一真实 Assign 为 F+1。 |
| 新 attempt 后再次重开 | 新尝试已 Assigned/Submitted/Accepted 或方案已更新时，旧 `.4` 仍能通过历史验证；不能依赖最新 Pending/最新 head。 |
| 活动和陈旧审阅 | Active run、active/disconnected worker、verifier、其他 Unknown、改变的 head/job/run 和错误摘要均拒绝，且拒绝过程不调度。 |
| 不完整或冲突证据 | 错 project/task/revision/worker/session、多个关联 job、错误 `.2` 链、原操作事件或同 attempt 历史出现时拒绝。 |
| 伪造 mutable Abandoned | 没有合法 `.4` 或工作流关闭事件时仍为 Unknown/拒绝恢复，不能由投影解锁。 |
| 关闭后的迟到消息 | `record_applied`、Commit ACK、启动对账和 trusted continuation 均拒绝，不写 `.3`。 |
| 原事件已存在、缺 `.3` | 走现有回执恢复；Assigned 无派发走现有 AbandonUnsent，不能走本次关闭。 |
| 曾有 Dispatch/NotStarted | 不允许按“从未开始”处置或重发。 |
| Propose/Accept/Advise | 分别验证关闭不制造或回滚业务事件，后续新操作可完成；覆盖 Plan 自动消费与旧摘要竞争。 |
| Wait | 无 workflow event 仍按正常 Wait 回执/答复恢复，不出现错误关闭入口。 |
| 原生 UI/IPC | 查看详情、错误摘要、关闭确认、刷新、窗口重开、核心重开、显式恢复至少一次；无 Key 时零真实调用。 |
| 容量和审计 | 管理分页/详情上限保持，候选不复制全部历史问答；旧请求计数与预算保持，关闭原因可查。 |

以上是原始完整目标清单。首版已完成的核心与原生子集在 V1 文档逐项列证；真实模型中断、更多入口和磁盘故障组合、后续 Submitted/Accepted/新方案的历史验证与容量等仍需补充。不能将本设计文档或上阶段测试数视为本阶段全部验收结果。
