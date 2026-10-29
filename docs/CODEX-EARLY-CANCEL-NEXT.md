# Codex 过早取消的读回恢复：下一批实施方案

状态：2026-10-08 21:01，独立证明及原真实尝试的只读恢复已实现并有限验收，见 [实现与证据](CODEX-EARLY-CANCEL-V1.md)。以下保留原实施方案和负例矩阵；原首次在线取消仍 inconclusive，不代表完整 P2/P6 已通过。下一步转 `CODEX-SUPERVISION-NEXT.md`，原范围保持。

## 1. 已确认的问题与证据边界

本次真实验证位于 `reports/p2-codex-cancel-live-20261008-1939/`：

- `test/result.json` 为 `inconclusive`：一次 host start、一次 DispatchIntent、一次 Started、一次 CancelIntent；一次取消命令被本地接受，一次原会话对账，最终 job/task 为 Unknown，run 保持 Paused。重开没有新增发送或管理请求。
- Started 到 CancelIntent 约 41 毫秒（两条账本 receivedAt 间隔 40.6656 ms）。首次执行错误为 RPC code `-32600`，旧错误记录没有 method，不能据此断言是 `turn/interrupt` 还是其他 RPC 失败。
- `readback-structure.json` 是随后独立只读诊断：同一 thread、同一 workspace，唯一匹配原已知 turn 的记录为 `interrupted`、`itemsView=full`、`items=[]`，没有 userMessage/clientId。此次诊断仅 `initialize`、`thread/read`，没有模型任务或再次 interrupt。
- `cleanup.json` 证明本次私有 Job 已清理，RemainingProcesses=0。清理成功不等于业务取消证据链通过。
- 管理回复和用户批准是明确标注的合成测试条件；不能将这次验证写成真实主指挥—副指挥—工作模型闭环。

目前严格读回需要远端已记录的 userMessage 同时匹配 clientId、原 prompt 和已知 turn。过早取消留下了真实的 Started ACK 及 CancelIntent，却没有远端 userMessage。因此当前 Unknown 是正确的保守结果；缺少的是可独立表达、验证和回放的取消证据类型。

## 2. 证据模型：保留旧证明，增加另一种取消证明

两种证明各自表述实际观察到的事实，不按宽松程度相互替代：

| 证明类型 | 能证明什么 | 允许的用途 |
| --- | --- | --- |
| `RecordedInput`（现有严格路径） | 远端记录了唯一匹配的 clientId 和原 prompt，thread/workspace/turn 绑定一致 | 维持现有完成、取消及读回逻辑，完成仍需独立制品捕获与验收 |
| `StartedTurnInterruptedV1`（新增） | 本地原先收到并持久化了目标 turn 的 Started ACK；随后持久化用户取消意图；远端现将同一唯一 turn 报为 interrupted，但没有记录输入 | 仅确认该已知 turn 已停止，写 Cancelled；不能证明 prompt 被记录、任务完成或制品正确 |

不要给新证明填入调用者传来的 prompt、clientId，再伪装成远端观察结果。新证明的数据结构根本不包含 `clientMessageId` 和 `promptSha256`。若需要显示原批准任务，读取本地既有批准记录，并清楚标注其来源。

## 3. 新证明必须同时满足的条件

### 3.1 本地可信锚点

核心从已通过完整性校验的原工作流历史中提取锚点，不接受 UI、测试参数或适配器传入的任意“已知 turn”作为授权依据：

1. 原 Revision、Approval、Assign/Attempt 仍构成同一 task、attempt、revision、fence、worker、thread、workspace 的有效绑定。
2. 原 DispatchIntent 在 Started 之前；Started 必须是实际 `WorkflowCommand::Started`，有合法 Running receipt 和非空 turnId，且 receipt/context/Attempt 绑定一致。
3. 随后的原 CancelIntent 针对同一 attempt，threadId、turnId、workspace 与该 Started 相同，含可信本地用户取消来源。必须满足 Started < CancelIntent 的原历史顺序。
4. 新证明记录这两个原事件的序号和哈希。验证时定位并核对原事件，不通过“最近一条 turn”推断，不以 CancelIntent 自己、Reconcile(Running)、投影文件或仅见到远端 turn 代替 Started ACK。
5. 当前是同一尝试的 Cancelling/Unknown，未被新尝试接替；run 仍暂停。恢复路径不得改变批准、预算、fence 或另行派发。

没有持久化 Started、ACK 丢失、只有 DispatchIntent 的情况仍走现有严格 RecordedInput 读回；缺少严格证明就继续 Unknown。新路径不授予这些情况“未开始”“已取消”或重发权限。

### 3.2 远端只读观察

通过既有受控 Codex 连接，读取原 thread，要求：

1. 返回的 threadId 与原 Attempt 完全相同；workspace 按现有 canonicalize 规则与原 workspace 相同。
2. 在整个返回 turns 中，目标 Started.turnId **恰好出现一次**。允许其他无关 turn，但不能用其状态支持目标 turn。
3. 目标 turn 的 status 必须严格为 `interrupted`，`itemsView` 必须显式为 `full`，`items` 必须存在且为数组。
4. 本批只支持实际遇到的 `items=[]`。如果存在 userMessage、其他条目、冲突输入或重复 clientId，不能借新路径绕过原严格输入验证；按原严格路径处理或维持 Unknown。
5. 不接受 summary、缺少 itemsView、未知字段值代表的其他视图、缺少目标 turn、重复目标 turn、未知/进行中/failed/completed 状态。

采用结构化解析结果区分“无输入的明确 interrupted”与“输入冲突/结构错误”。不要通过匹配错误字符串触发宽松 fallback。优先单次 `thread/read` 后按类型判定；不因解析失败反复请求、再次 interrupt 或启动另一个任务。

## 4. 代码边界与 schema 回兼

建议分为四个明确改动点，保持现有行为可逐项回归：

1. **适配器**：保留现有 `TurnObservation` 的字段及序列化结果，作为 RecordedInput 证明。增加独立的 `InterruptedTurnObservation`，只包含远端观察的 threadId、turnId、workspace、interrupted/full、itemCount=0、匹配目标数=1等最小结构。将同一次读回解析成有标签的结果，或者提供独立受限方法；不要把原 `TurnObservation` 的必填字段改成可选值。
2. **核心执行器**：增加从历史读取 `StartedCancellationAnchor` 的函数。新增证明分支必须先通过本地锚点验证，再解析/验证远端观察。`WorkerChannel` 测试接口需显式支持新类型；不支持的 channel 默认拒绝。当前严格 `check_observation`、`submit` 保持严格。
3. **持久化**：新增有独立标签的证据变体，例如 `ExecutionEvidence::StartedTurnCancellation { receipt, proof }`。`proof` 包含版本、两个历史锚点、归一化远端观察和观察时间；不包含 receipt 本身，避免摘要循环。`receipt.evidence_sha256 = digest(proof)`，工作流 evidence 同样绑定该 digest。原始输入、key、完整 wire body、思考内容不落盘。
4. **调度器验证/重放**：新变体仅允许 `Reconcile { outcome: Cancelled }`，receipt.status=Cancelled、manifest=None。重新校验两个原事件及其先后关系、绑定、proof 摘要和远端结构。Submit、Started、Reconcile(Submitted/Running/NotStarted)、Verify、Accept 均不得接受新变体。job 的 Cancelled 投影继续由真实工作流终态派生。

新增变体不改旧 `ExecutionEvidence::Receipt` JSON，不改现有 receipt schema/version，不补写旧事件，不更改旧历史的哈希。旧无类型标签的严格回执保持原回放方式，不能根据缺少 prompt 字段被解释成新类型。

新版必须读取既有数据及本次真实失败记录；旧版读到新变体会 fail closed，需要在升级说明中明确“此记录新增后不支持旧版本读取”，不得以删除证明字段实现降级。前端若只消费既有 Snapshot/CodexJobState，可保留协议；若暴露证明详情，则更新 TS 契约并保持旧快照兼容。

## 5. 实施与验证顺序

### 第一步：先落结构与回归，不调用模型

实现独立类型、锚点提取、解析和 scheduler 新分支；加入本节列出的有效/无效样例。先跑适配器、核心、domain 的定向测试，随后 fmt、clippy 和现有完整检查。

至少覆盖这些必要负例：

| 类别 | 必须拒绝或保持原逻辑的样例 |
| --- | --- |
| 本地锚点缺失 | 无 Started；只有 DispatchIntent；仅 CancelIntent；仅 Reconcile(Running)；ACK 未持久化 |
| 本地绑定不符 | 不同 attempt/task/revision/fence/thread/turn/workspace；CancelIntent 早于 Started；伪造来源；历史锚点哈希不符；已切换到新尝试 |
| 远端目标歧义 | 同 thread 不同 turn；目标不存在；目标出现两次；另一 turn interrupted 而目标仍 running/completed |
| 远端视图不足 | itemsView=summary/缺失/其他；items 缺失/null/非数组；status 为 inProgress/failed/completed/未知 |
| 输入冲突 | 目标有不匹配 userMessage、重复 clientId、不同 prompt，或本批不支持的非空 items；不能进入空输入取消路径 |
| 结果越权 | 用新证明提交完成、捕获通过制品、通过独立验证、主指挥收口、重置预算或重发 |
| 持久化/重启 | 新证据摘要被改、锚点被改、记录中断；写入后再重开，不新增请求/取消/重复 Cancelled 事件 |
| 旧路径回归 | RecordedInput completed/interrupted 仍按原要求验证；无 ACK 但有严格 RecordedInput 的旧恢复能力不被删除；无 ACK 且无输入仍 Unknown |

有效样例必须验证：空 items 的唯一 interrupted + 合法锚点 => Cancelled，保留已有修改，run Paused，预算/fence/DispatchIntent 数不变。仅 transport 返回 interrupted、但缺任一锚点，不得通过。

### 第二步：恢复本次真实原记录，不启动新模型任务

保持 `p2-codex-cancel-live-20261008-1939/` 的原报告和原业务数据原样，先生成文件哈希清单。在新的 fresh reports 目录准备该原业务数据的可信副本，保留其原 project/workspace/Attempt 绑定，不重写路径、thread、turn 或 prompt。此副本仅用于原尝试读回恢复，不运行验证命令或工作模型。

用新版核心显式对账该原 Unknown attempt，所有权仍由外层私有 Job 约束。远端仅连接和 `thread/read`；禁止 `thread/start`、`turn/start`、`turn/interrupt`，也禁止主/副指挥 API。若既有初始化包含 account/model 查询，要将这些只读方法如实列入方法清单。

若新证据满足条件：在新的恢复数据中追加一次带新类型证明的 Cancelled 事件及派生状态，保存原 Started/CancelIntent、normalized proof、receipt、前后历史摘要、方法计数、重开快照及 cleanup。原失败报告继续保留 inconclusive，用新恢复报告说明“同一真实原尝试经补充证明恢复成功”。

若读回状态已变化、目标缺失或验证不符：保留 Unknown 和诊断，不能强改旧数据，也不能为了拿到成功结果再发模型任务。任何重开都要证明新增 DispatchIntent=0、管理请求=0、interrupt=0。

### 第三步：桌面状态与验收

沿用已有用户显式对账入口展示结果，确认 Cancelled 后 run 仍 Paused，已有修改可见，不能直接恢复旧取消任务；需要新方案/批准的规则保持不变。回归之前的 interrupted、completed race、ACK lost 三条 native 场景及 startup/application 恢复。

界面可以显示“原执行已确认停止；远端未记录输入”，避免显示“任务完成”或宣称模型执行过任务。只读观察不能代替独立制品验证。

## 6. 本批验收口径与后续

本批通过需要同时具有：类型隔离与负例回归、新版读取旧数据、原真实尝试的零新任务恢复证据、重开无新增发送、Job 清理为零。仅合成样例通过，或仅看到远端 interrupted，不足以验收。

修复真实原尝试可证明“早取消后恢复确认”能力，不自动证明首次在线取消过程无错误；首次 `-32600` 的 method 缺失仍是历史证据限制。新诊断已可附安全 method 标签，未来新一次真实取消验收应明确是否另需执行，不能在本批恢复中偷偷增加任务。

完成后更新实际交接和阶段矩阵，再推进 supervision。真实 OpenAI 管理调用、完整三级闭环、多接入对象、安装升级和 P6 总体验收继续按原计划推进，不能以这个组件通过替代。
