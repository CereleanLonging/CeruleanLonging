# Codex 启动记录恢复：原始设计与剩余矩阵

日期：2026-10-08。状态：**设计存档；新协议启动关闭子机制已实现并通过有限验证，完整矩阵尚未验收**。15:48 实际结果见 [STARTUP-RECOVERY-V1.md](STARTUP-RECOVERY-V1.md)。以下保留实现前的行为分析、提案和原始完整矩阵，不能将其中“当前/建议/待实现”当作最新实现状态，也不能把部分通过扩大为全矩阵通过。

本文件讨论 Codex job 已持久化，但尚无管理 `.2 ApplyIntent`，也尚无工作流 Assign 的窗口。本轮既有测试数、Applying 恢复和原生验证结果，均不能视为本设计通过。本文只读分析当前源码，不修改实现。

## 1. 当前实际行为与真实缺口

以下事实来自当前 `RuntimeHost`、`async_codex` 和管理桥接代码。

| 当前情况 | 已有行为 | 剩余缺口 |
| --- | --- | --- |
| worker 正常返回错误，账本恢复成功且没有该 attempt 的 Assign | `poll_codex_workers` 将 job 标为 Failed；`observed_state` 在无 Attempt 绑定时保留 Failed。 | 这是现有启动失败分类，不等于已有用户签署的不可变关闭记录。 |
| 无 `management_operation` 的 Failed，无 Assign | `require_available` 不因 Failed 阻塞项目；新的合法 attempt 可以通过现有可信启动/后续编排入口继续，旧 ID 仍受 job map 防复用。 | 不应描述为完全无法恢复。若增加显式关闭，应以审计和稳定保留身份为目的，不能强迫正常 Failed 先走新的收费或远程流程。 |
| 有 `management_operation` 的 Failed，无 `.2`/Assign | 管理响应仍可能 Ready；`start_managed_codex` 拒绝同 operation 已有关联 job；普通管理放弃也拒绝有关联 job。 | 旧请求无法消费或放弃，新的项目编排仍会遇到这条未决请求，需要专门的跨账本关闭。 |
| 任一路由的 Unknown，无 Assign | `require_available` 阻止重派；`reconcile_codex` 又要求任务状态 Unknown 且当前 attempt 匹配。无 Assign 时任务通常仍 Pending，缺少可读回绑定。 | 属于真正的启动前未知窗口，现有远程对账按钮不能解决。 |
| job 保存成功但后台线程创建失败 | `launch_codex_job` 保守写 Unknown 并恢复项目账本。 | 即使错误发生在调用本地进程前，目前也没有独立的持久化“未启动”证明和用户处置闭环。 |
| job 保存后，进程/连接/模型检查或 `thread/start` 失败，未到 Prepare | 正常返回时可能成为 Failed；核心崩溃后 Starting 等投影通常成为 Unknown。 | 投影状态不能区分所有具体中断点，尤其 `thread/start` 响应丢失时会话可能已创建。 |
| 已有 `.2`、无 Assign | 进入现有 application recovery 模块。 | 不是本次新增路径；不得重复实现或降低原检查。 |
| 已有 Assign、无 DispatchIntent | 进入现有 unsent assignment 模块。 | 不能按无 Assign 启动记录关闭。 |
| 已有 DispatchIntent 或执行回执 | 保持原执行对账。 | 不得因为 job 消失、状态 Failed 或读回暂不可用而视为未派发。 |

当前 job 是可更新 JSON，`CodexJob` 没有启动协议版本、受控来源或不可变启动意图字段。`observed_state` 对无 Assign 的 Failed 保留分类，是当前可信 host 运行路径的行为；仅改写磁盘上的状态为 Failed 不能作为新增“已证明未发送”能力的凭据。

## 2. 当前协议可证明什么

当前 managed 路径的顺序是：

1. `start_codex_inner` 核对批准、配额、attempt 预留，构造 Starting job。
2. `launch_codex_job` 先保存 job，再把项目账本移交后台线程。
3. worker 启动 Codex、连接、核对账户与模型，调用 `thread/start` 取得新会话绑定。
4. Prepare 经 host 保存 `.2` 后才 ACK；worker 收到 ACK 后写 Assign。
5. Commit 经 host 核实磁盘 Assign、保存 `.3` 后才 ACK。
6. `codex_execution::dispatch` 先保存 DispatchIntent，再调用 `turn/start` 提交任务输入。

无管理 operation 的路径跳过管理双 ACK，但同样先写 Assign，再进入持久化 DispatchIntent 后才能发送的 dispatcher。

因此，“完整受控账本不存在原 attempt 的 Assign/Dispatch，且原 worker 已不可能继续写入”可以与已知的发送协议合起来证明：**工作台没有通过这条受控任务通道派发该任务。** `.2` 不存在只是 managed 路径的额外检查，不能单独作为结论。

这个证明不包括：没有启动 Codex 进程、没有创建远程会话、没有网络连接、没有计费、用户未从其他入口操作过、外部桌面会话未执行任务。`thread/start` 响应丢失时尤其不能声称“没有会话”。不应为寻找一个可能存在的空会话再次调用 `thread/start`，也不应发送任务来试探是否已经执行。

对于导入/接管会话、不明来源 job、未知历史启动器或不保证持久化先于发送的旧协议，缺少本地事件不构成未发送证明。此类记录继续保留 Unknown，并采用适配器自己的只读执行证据核对；用户的“接受风险”勾选不能替代证据并自动开放重派。

## 3. 推荐的证据结构

建议把新启动流程的权威记录从可变 job 投影中分离。具体名称和 schema 需要实现前定稿，下述均为提案。

### 3.1 新启动协议

在启动线程、进程或调用远程会话 API 前，持久化独立 `StartupIntent`，至少包含：

- schema 与发送顺序协议版本、受控来源标识；不能只存当前应用显示版本。
- project、task、attempt，以及可选的 management operation。
- 完整批准 revision 标识和 worker 选择（provider/model/executable pin）。
- 启动授权时的工作流前缀锚点 `{ sequence, hash }`，以及必要的管理链 head。
- 创建时间和唯一启动身份；每个项目的 attempt 必须在此阶段即被永久预留。

写入成功才允许创建可变 job 和启动 worker。若不可变意图已经保存但 job 写入失败，仍保留预留记录；不能自动删除意图后使用同一 ID 再启动。job 仅作为可修复的 UI/运行投影。

关闭动作追加不可变 `StartupClosed`，记录原因、原启动意图 hash、关闭时经审阅的历史锚点及关闭依据类型。其含义限定为关闭受控启动记录，不是远程取消、执行成功或费用返还。

如需保存 `WorkerStoppedBeforeAssignment` 等内部诊断，可以补充，但只靠一个错误字符串不足以证明未派发；仍要交叉验证原启动身份、完整工作流、发送协议和活动持有权。不能让 worker 自报“未执行”绕过核心账本。

### 3.2 有管理 operation 的原请求

有 job、无 `.2` 时，原管理 `.0/.1` 和 job 关联必须保留。普通 `Discard` 当前禁止关联 job，这个保护不能直接去掉。

建议追加一种专门的管理用户 resolution（例如 `AbandonStartup`），引用同一启动关闭证据和 attempt，仍使用合法的 `.4` 槽位，不制造 `.2` 或 `.3`。其准入条件必须是：原响应确实为匹配的 Assign、没有应用意图/工作流事件、关联启动身份唯一且已经核验。

`validate_managed_codex_links` 当前在没有 `.2` 时使用 `management.ready(...)` 验证关联；当新 resolution 令原请求终结后，该调用会拒绝。因此必须新增“有合法启动关闭证据”的专用校验分支，不能简单把所有已放弃请求都视为合法，也不能删掉 job 的 `management_operation` 来通过校验。

### 3.3 跨账本提交顺序

推荐先提交权威 StartupClosed，再提交关联管理 `.4`，最后修 job 投影。启动和用户提交均在已有单写者范围内执行。

- 第一步前失败：维持未决，不解锁。
- StartupClosed 已提交、管理 `.4` 未提交：重开核验关闭记录和原 `.0/.1`，只补同一关闭 resolution；不重发模型请求，不生成 `.2`。
- 管理 `.4` 已提交、job 未更新：重开按权威证据修复投影。
- 关联文件冲突、未完成发布或身份不一致：保持拒绝继续，不自动删证据或重建为成功。

如果最终选择其他写入顺序，也必须明确唯一权威记录、每个中断点的幂等恢复规则，并在管理/worker 调度开放前完成交叉核验。不能仅更新两份可变 JSON 后假定已经原子完成。

## 4. 旧记录兼容与协议边界

新增字段不得以 `serde(default)` 把旧 job 自动升级为“拥有新协议未发送证明”。当前 job 使用 `deny_unknown_fields`，也需要考虑旧二进制无法读取新增字段或新证据；升级前保存可恢复备份，并明确支持的读取版本。不能承诺旧二进制可任意降级读取未来格式。

建议分类处理：

| 来源 | 可用处理 |
| --- | --- |
| 新协议，StartupIntent 完整 | 按版本对应的持久化先后约束及日志核验，提供显式关闭。 |
| 已知历史工作台协议，能够证明它也始终先落 Assign/Dispatch 再发送 | 通过经过审查的兼容规则识别；保存原 job 原文 hash、稳定身份和采用的历史协议依据，再进行关闭。兼容规则必须有对应旧 fixture 回归，不能只判断文件时间早于升级。 |
| 当前进程内已知受控启动，尚未具备新磁盘意图 | 若准备提供过渡能力，必须在当前可信生命周期中保存足够证明；重开后缺失的内存事实不能猜补。 |
| 来源/发送顺序未知，导入或接管的旧 job | 保持待核对；使用外部只读证据或受支持的迁移审计，不能仅凭缺 Assign 就宣告未派发。 |

当前 `CodexJob` 本身没有记录创建它的 host 发送协议。worker executable 的 hash 是 Codex 程序身份，不能替代工作台 host 的协议来源；当前 `PROTOCOL_VERSION` 也不是旧 job 的创建证明。

若引入 `LegacyAdoption`，应明确它是本次审计对旧证据的接纳，保留原内容和依据，不伪装成当年启动前已经存在的 StartupIntent。未知旧记录可以保留并显示限制，不能以“兼容”为由放松对未知远端执行的约束。另一方面，已被证据覆盖的历史协议应提供兼容路线，不应无条件把全部旧 Failed/Unknown 永久封死。

## 5. 用户提交的资格检查

新增“审阅启动记录”和“关闭启动记录”的用户接口；名称暂不固定。详情读取只返回证据，不驱动 worker、管理请求或调度。提交命令必须跳过 host 的 pre-poll，然后在同一写锁下重新检查。

必须核对：

1. 管理、启动和项目账本健康，完整历史可读；没有隐藏的未完成发布。
2. 项目无 Active run、活动管理请求、执行/验收线程；断连但仍占有项目的对象仍阻止关闭。
3. 工作流不存在 Dispatching/Running/Cancelling/Unknown，包括没有 job 文件的其他任务。不要只检查当前被选中任务或 job map。
4. 其他不确定 job/verifier 不能因关闭目标记录被一并解锁；对待关闭目标的 Unknown 只作严格绑定后的定向例外。Assigned 的处置遵循既有路径，不随意增加会形成互相等待的全局限制。
5. 原 attempt 在完整项目历史中没有任何命令或绑定证据；同时核对没有对应 `mgmt-{operation}` 或直接启动的 Assign/Dispatch。不能只看最后一个事件或当前 TaskRun。
6. 没有 `.2`，没有 ApplicationClosure/AbandonUnsent 等另一条既有处置路径；若核查发现已进入后续阶段，转到该阶段并要求重新审阅。
7. project/task/attempt/revision/worker 与历史批准和受控启动身份一致。managed 还需原 Assign 响应、唯一 operation/job 关联一致；unmanaged 必须明确没有管理归属，不能通过清空字段伪装成直接启动。
8. 审阅摘要绑定权威启动 head、工作流 head、管理 head、job 审阅内容和 run 状态。提交时摘要过期必须拒绝。

Failed、Unknown、Starting 等仅影响说明，不替代上述证明。可以接受有完整依据的启动失败，也应拒绝伪造 Failed/Abandoned、串项目、多个任务共用 attempt、一个 operation 绑定多次启动等冲突。

关闭不修改工作流任务状态。特别是旧 Failed 之后用户已经用新 attempt 推进了同一任务时，不能把该任务重置 Pending、清掉新 attempt 或回滚验收。需要修改配置、重新批准或恢复运行时，走原有显式流程。

## 6. 永久身份与历史重放

原 project+attempt 必须永久保留，不能因为 job 终态、删除投影、归档或升级而重新变成可用。管理 operation 同样不可复用。

目前 `start_codex_inner` 的 job map 检查和管理 `.2` reservation 覆盖一部分路径；无 `.2` 的启动预留需要扩展到新的权威启动账本，且所有直接/managed 启动、Prepare、通用应用入口及启动交叉核验使用同一规则。投影文件缺失不能让 ID 重新可用。

本窗口没有实际 Assign，不增加 fence。后续新 attempt 的真正 Assign 应在对应任务原 fence 上加一；不能为了占住 ID 伪造一次分配。

历史处置记录的校验与实时关闭资格必须分开：

- 验证保存的工作流 head 是当前完整日志中的合法前缀，不要求它仍是最新 head。
- 验证当时的历史批准与绑定，不要求当前 revision 永远相同。
- 旧关闭记录重放时不要求任务仍 Pending；新 attempt 可以已运行或完成。
- job 的 mutable state/message/updatedAt 不能成为永久相等条件；稳定身份不得改变。
- 原 attempt/operation 的禁止复用贯穿整个后续历史。若旧关闭记录之后出现原身份的工作流事件，必须报告冲突，不能自动“恢复”为已执行再放行。

如需恢复锚点状态，应使用无副作用的前缀 replay，不能调用会生成新 RecoveryUnknown 的 restore 来审阅旧记录。

## 7. UI 与最小用户闭环

在 job 详情展示启动阶段和证明范围；managed 同时显示原管理请求链接。按钮建议使用“关闭启动记录…”，避免“已取消远程任务”“未发生计费”或“重新尝试原任务”等误导文字。

用户流程：暂停项目 → 只读审阅原启动与历史依据 → 输入原因并确认 → 追加关闭证据、修复投影 → 保持暂停 → 用户显式恢复或调整配置 → 使用新 operation/attempt 继续。

- 有管理 operation：关闭后原请求终结，但保留响应、关闭原因、job 链接和已消耗的管理请求计数。
- 无管理 operation 的 Unknown：成功关闭解除这条启动记录造成的阻塞，其他阻塞保持。
- 无管理 operation 的 Failed：已有新 attempt 能力应保持；正式关闭是可审计处置，不应新增无关的前置障碍。
- 来源未知：解释缺失哪项证明，提供现有只读对账路径；关闭本地显示或用户接受风险不能触发自动重派。
- 提交过程中证据变化：刷新详情，不能静默切换成另一路关闭或取消。

不自动发送管理模型或 worker 请求，不自动结束用户其他程序，不退款或重置请求计数，不隐去原错误。创建会话可能已发生，应诚实显示“会话建立情况可能需要单独核对”。

## 8. 代码落点

| 当前文件 / 函数 | 下一阶段工作 |
| --- | --- |
| `crates/domain/src/lib.rs`：`CodexJob`、`CodexJobState`、用户 Command | 设计启动身份/详情/关闭接口及兼容格式，明确 mutable job 与权威启动证据的边界。 |
| 新启动证据模块（待定名） | 不可变启动、预留、关闭及旧协议接纳记录；链校验、前缀锚点、单写者和发布中断恢复。 |
| `host.rs::start_codex_inner` / `launch_codex_job` | 启动前持久化新意图；所有入口统一检查原 attempt 预留；线程创建失败的证据处理。 |
| `async_codex.rs::run` / `managed_assign` | 保证新旧受支持协议的落盘先后顺序；传播稳定启动身份，不以错误字符串推断远程结果。 |
| `host.rs::poll_codex_workers`、`async_codex.rs::observed_state/load` | Failed/Unknown/Closed 的投影以完整证据为准；兼容现有已知正常失败行为；重开能修复投影。 |
| `management_execution.rs` | 如采用 managed `.4` 新 resolution，定义合法前序、引用及终态；保留原 `.0/.1` 和 operation。 |
| `codex_management_bridge.rs::validate_managed_codex_links` | 无 `.2` 且原请求已由启动关闭终结时，验证专用关闭证据；不能再仅调用 ready，也不能跳过身份核验。 |
| `management_resolution.rs::management_discardable` | 保持普通放弃的关联 job 限制；新跨账本关闭不能通过粗略放宽此函数实现。 |
| `host.rs::open` | 在开放服务/调度前完成所有启动/工作流/管理交叉校验和必要投影修复。 |
| `host.rs::handle` 与 read API | 详情只读、候选无 snapshot 递归；关闭命令不 pre-poll。 |
| `project_runner.rs`、桌面 job 与管理详情 | 保留暂停和新身份恢复语义；不消费旧 Ready 请求再次发送。 |

## 9. 待执行验收矩阵

全部先用独立目录、合成响应/进程/传输通道，不需要真实模型或 Key。此表没有任何已通过标记。

| 场景 | 必须验证 |
| --- | --- |
| 新意图已写、job 未写或写失败 | 不启动线程；原身份预留保留；重开不会自动补启动。 |
| job 已写、线程尚未启动；线程 spawn 失败 | managed/unmanaged 两路均可按完整受控证据审阅；关闭不创建 `.2`/Assign/Dispatch。 |
| 程序启动、连接、登录或模型检查失败 | 正常返回与核心崩溃分别覆盖，不能把所有 Unknown 都当 Failed。 |
| `thread/start` 响应丢失 | 允许保留“可能已有会话”的事实；`turn/start` 次数仍为零；不为探测重复创建会话。 |
| managed Ready + Failed/Unknown，无 `.2` | 专用关闭终结原管理请求并保留链接；普通 discard 和同 operation 重试仍拒绝。 |
| unmanaged Failed，无 Assign | 既有新 attempt 能力保持；原 ID 不可复用；新增审计关闭不成为强制重试前置条件。 |
| unmanaged Unknown，无 Assign | 只有受支持协议及完整证据满足才解锁；无绑定的旧 reconcile 不被伪装成成功。 |
| 已进入 `.2`、Assign 或 Dispatch | 分别转既有处置；对旧摘要拒绝，绝不由本功能吞掉后续事件。 |
| 关闭各落盘点中断 | StartupClosed、管理 `.4`、job 三者的每个中断点均可幂等恢复，零自动模型调用、零重复文件链。 |
| 多任务/多 job/活动持有权 | 其他任务无 job 但 Running/Unknown 仍阻止关闭；active、断连持有者、其他 Unknown 不被一并释放。 |
| 错绑与伪造投影 | 改 state、改 management_operation、串项目/task/revision、双绑定、删除 job 均不能取得未派发证明或复用 ID。 |
| 旧协议兼容 | 每个支持的旧版本 fixture 正常读取与恢复；未知来源没有默认升级；旧证据 hash 保留；明确降级限制。 |
| 新 attempt 后再次重开 | 旧关闭记录依历史前缀通过，新任务状态和验收不回退；方案升版后亦不要求旧 revision 等于当前。 |
| 所有复用入口 | 新 operation 携旧 attempt、直接启动、managed Prepare、通用 apply、归档后重开均拒绝复用。 |
| UI/IPC 双窗口 | 只读详情不派发，错误摘要和重复提交拒绝，关闭需明确原因，刷新/重开仍保留证据，恢复使用新身份。 |
| 容量与计数 | 历史分页/详情保持边界；不复制全部问答；关闭不减少旧管理请求数，也不假装退回费用。 |

后续按上述原始矩阵补齐证据，实际已覆盖部分以 STARTUP-RECOVERY-V1.md 的报告为准。旧协议接纳、真实启动故障、双窗口竞争和容量等仍未完整验收；正常 Failed 的有限继续能力也不等于整个恢复矩阵通过。
