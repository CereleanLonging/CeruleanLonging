# Codex 活动执行取消 V1

本文记录 2026-10-08 当前实现的活动取消协议、界面和验证入口。常规检查与三路原生协议场景已通过；原首次真实 Codex 早期取消仍为 inconclusive；21:01新增独立证明后，原尝试只读恢复已通过，见 [后续修复](CODEX-EARLY-CANCEL-V1.md)。下文原失败证据保留。此组件结果不代表 P6 或全部接入对象已验收。

## 可请求停止的范围

当前入口面向核心仍持有的 Codex 执行：工作线程控制通道可用，Job 为 `executing`，工作流任务为 `Running`，任务与原 attempt 一致，而且没有提交过停止请求。核心返回 `Snapshot.codexCancellations` 候选，界面据此提供“请求停止当前任务…”。候选摘要绑定活动工作流快照、执行模型选择和 attempt；提交时重新核对，拒绝旧摘要与重复请求。

`Starting`、尚未取得 Started 回执的派发阶段、已经断开的线程，以及 `Unknown` 状态，都不提供主动 interrupt。`Unknown` 只有在现存 Job 与工作流原 attempt 匹配时才提供“核对原会话”；该入口读取原会话，不重新发送任务，也不补发取消。启动关闭、未提交应用关闭和放弃未派发尝试仍使用各自已有的证据与入口，不能与活动取消互换。

该能力针对工作台自己管理的 Codex 执行通道，不代表能停止任意已打开桌面应用中的会话；也不表示 OMP、Qoder、WorkBuddy 或其他模型已支持同等取消协议。

## 从用户请求到持久化取消意图

1. 用户在确认框核对项目、任务目标、目录和记录标识，填写停止原因。原因须非空、不超过 8000 个 UTF-8 字节且不含 NUL。
2. `cancelCodex` 命令先校验身份、候选和原因。此命令不会先驱动一次调度轮询，避免在暂停前开始下一项工作。
3. Host 将该项目所有尚未完成的运行记录保存为 Paused。暂停失败时，不投递取消控制消息。
4. Host 保存 Job 的 `cancel-requested` 投影，再通过一次性控制通道把原因交给原工作线程。保存失败时不投递；通道已失效时保留暂停，将结果标记为 Unknown，并提示核对原任务。
5. 持有项目工作流账本与 Codex transport 的唯一工作线程收到消息后，先保存绑定原 task、attempt、thread、turn、目录及取消来源的 `CancelIntent`。保存成功后，才允许调用 `turn/interrupt`。

`cancel-requested` 表示本地请求已排队，不等于工作流 `CancelIntent` 已持久化，更不等于远端执行已经停止。Host 不代替工作线程写入工作流取消事件。队列处于内存中；如果在排队后、工作线程落下 CancelIntent 前进程中断，重启会保留待对账状态并保持后续调度暂停，不会根据 Job 投影自动补发取消。

取消控制通道只能消耗一次。正常路径每次取消最多发送一次 interrupt；若已有匹配终态缓存，可以直接使用终态而不发送。发送、回执或终态不确定时不会重试 interrupt，不释放原 attempt 身份，也不会退回请求预算或删除已发生的文件修改。

## 停止结果的确认

interrupt 的 RPC 成功回执只证明请求获得回复，不能独立证明任务停止。执行器还须提供与原 thread、turn 匹配的终态，随后通过 `thread/read` 读取完整原始回合，核对 workspace、唯一 client message ID、原任务 prompt 摘要及 turn 身份，并要求读回状态与终态通知一致。

| 观察结果 | 工作流与 Job 的处理 |
| --- | --- |
| 终态和完整读回均为 `interrupted` | 保存绑定原 attempt 的取消回执，任务及 Job 成为 Cancelled；不生成完成产物清单或验收通过 |
| 任务先自然完成，终态和读回均为 `completed` | 保留 Submitted 结果并独立捕获产物，等待验收；不误写为 Cancelled |
| 已保存 CancelIntent 后才观察到自然完成 | 经 Unknown 对账路径保存 Submitted；保留取消请求历史与实际完成结果 |
| interrupt ACK 丢失、观察超时、通道中断、身份不符或读回不完整 | 保持 Unknown，要求核对原会话；不宣称停止成功 |
| 读回仍为 `inProgress` 或 `failed` | 当前恢复入口保持 Unknown，不把它们解释成取消成功，也不重新派发 |

取消与自然完成竞争时，已经缓存或立即可读取的匹配终态优先。若自然完成发生得更晚，仍以完整读回为准。任何停止请求都不会自动恢复项目调度；自然完成后的独立验收和收口也须在用户明确恢复调度后继续。

确认 Cancelled 后，原任务保持取消终态。恢复原运行授权会被拒绝，需要用户修订并批准后续方案。若同一方案还含其他未解决任务，仍须满足既有方案修订门禁，不能仅因一个任务已取消就绕过其余任务的处置。

## 重启、历史记录和兼容性

工作流账本是取消结果的权威来源。重启后，已保存的取消回执恢复 Cancelled；已保存的提交证据恢复 Submitted；只有活动投影而没有可信终态的记录恢复 Unknown。持久化的 CancelIntent 保留原因与取消来源，恢复时不会将它解释为重新发送 interrupt 的授权。

原会话对账使用原 attempt、prompt、workspace 和已知 turn。发生过 Started、CancelIntent 或 Running 对账的记录会沿用相应 turn 身份，不选择另一个回合代替。读取到可信的完整取消或完成终态后，才能追加相应对账结果。旧记录没有新取消字段时，不自动补造用户取消请求；外部中断如果有匹配原 attempt 的完整终态证据，也可按实际取消结果对账。

原始 Assign、DispatchIntent、Started、管理分配记录及批准保留。取消不会把任务重置为 Pending，不回滚文件，不创建新的工作任务，不把“关闭记录”当作“停止远端”。旧启动恢复及应用恢复的兼容性仍由其专门检查维护。

## 界面状态与用户操作

| 新 Job 状态 | 界面含义 |
| --- | --- |
| `cancel-requested` | 已请求停止，等待执行器处理；后续调度已暂停 |
| `cancelling` | 工作线程已登记停止意图，正在等待最终结果；尚不能确认已停止 |
| `cancelled` | 执行器终态及读回已确认停止；已有修改与记录保留 |

Unknown 且工作流保留取消请求时，界面显示“停止结果待核对”。若原请求先完成，显示 Submitted 并保留实际执行结果。确认框说明暂停、文件保留、完成竞争和不会自动重发的影响；返回确认框不投递请求。提交期间防止重复点击，候选变化后旧确认失效。

## 半帧和并发读取

等待终态与等待取消由同一 transport 所有者协调。读取中的部分 JSON 帧保存在 transport 的 `partial_frame`，收到取消时即使挂起的读取 future 被丢弃，已消费的半帧仍保留。后续 interrupt RPC 读回复时继续完成该帧，避免将剩余字节当作新消息。

审批请求的拒绝回复在当前消息处理完成后才让出控制，不会为响应用户取消而截断正在发送的拒绝帧。连续通知之间检查取消通道，避免通知持续到达使停止请求长期得不到处理。帧大小、等待时限、消息身份和终态队列仍有原协议限制；协议不确定时使通道失效并要求对账。

## 验证入口与结果

取消逻辑新增 5 项 adapter、7 项 dispatcher 和 2 项 host；真实诊断后另增 4 项 adapter 测试。最终 `reports/p2-codex-cancel-final-checks-20261008.log` 记录 **153 项 Rust、4 项 Node** 通过，fmt 和前端构建通过；`p2-codex-cancel-diagnostics-clippy-20261008.log` 为最终严格检查。此前 149 项记录 `p2-codex-cancel-complete-checks-20261008.log` 保留，定向测试不重复累计。最终生产诊断只增可观测信息，没有放宽证明规则。

| 入口 | 重点覆盖 |
| --- | --- |
| `crates/adapters/src/codex.rs` 新增 5 项测试 | 半帧保留、终态优先、控制通道断开后继续观察、审批拒绝完整发送、取消回执或终态缺失后不重复发送 |
| `crates/core/tests/codex_cancellation.rs` 7 项测试 | 意图先于 interrupt、绑定终态与重开、ACK 丢失后只读恢复、自然完成竞争、错误证据、意图发布失败、运行中或失败读回、外部中断与原 turn 关联 |
| `crates/core/src/codex_cancellation.rs` 2 项 host 测试 | 校验失败不暂停、暂停先于控制投递、重复拒绝、暂停或投影失败不投递、接收者消失后保持暂停 |

定向入口可从项目根目录执行：

```powershell
cargo test -p cerulean-adapters --lib codex::tests --locked
cargo test -p cerulean-core --test codex_cancellation --locked
cargo test -p cerulean-core --lib host::codex_cancellation::tests --locked
```

第一条包含该 adapter 的既有测试，并非只选择新增 5 项。常规质量门禁继续使用 `scripts/check.ps1`、严格 clippy 和桌面构建。

原生入口为 `tests/codex-cancel-native.mjs`，模式为 `--mode=interrupted`、`--mode=completed`、`--mode=ack-lost`。它连接生产 Host、worker、stdio 子进程和 Tauri 界面，使用独立数据目录及确定性的协议模拟进程；后台夹具入口为忽略测试 `host::codex_cancel_native::native_codex_cancel_service_fixture`。运行前需要相应调试二进制及 `codex_protocol_fixture` example。

| 报告目录（均在 reports 下） | 观察与重启结果 |
| --- | --- |
| `codex-cancel-native-3TmtlE` | Running → Cancelled，完整核心重启后仍 Cancelled |
| `codex-cancel-native-FULc35` | 取消后任务先完成，保留 Submitted 和产物；重启仍 Submitted |
| `codex-cancel-native-i3vgQR` | interrupt 回执丢失保持 Unknown；完整核心重启后明确核对原会话，成为 Cancelled |

三场景均通过：各 1 次 `turn/start`、1 次 `turn/interrupt`、1 次 `thread/read`、1 条 CancelIntent，取消后新增派发 0；请求前先暂停，错误/空白/NUL/超长原因及旧摘要/重复命令拒绝，返回确认框不写账本，原证据保留；调度保持 Paused，不自动验收或调用指挥模型。`checks` 中仅属于另一模式的项目为 false，表示该模式不适用；总体执行状态以顶层 `status` 为准。确认框及停止、自然完成、未知、核对后的截图已检查。

三份 cleanup 均确认测试自建核心退出。Cancelled 可正常停止；Submitted/Unknown 核心拒绝正常退出时，脚本在工作线程已归还控制后只终止所持有的测试 ChildProcess，以注入核心中断，随后完整重开。不能将这些强制退出写成全部正常退出，也不会按进程名称清理既有应用。

首次 7 项 dispatcher 测试因夹具未绑定 Chief/Deputy 的 OpenAI provider 被正确拒绝，补齐夹具后通过。首次 ack-lost 原生测试 `codex-cancel-native-NE3wIx` 等待了通用 Unknown 文案，而产品实际显示“停止结果待核对”；只修正文案匹配后通过，失败报告和清理记录保留。生产权限和状态判定未放宽。

旧入口回归 `reports/startup-managed-native-hIFUp0/result.json` 与清理通过；发布 `reports/p2-codex-cancel-release-20261008/release-launch.json` 证明实际启动、正常关窗、关窗后常驻核心仍可读，日常新计划 0，未修改用户项目。脚本只清理其自行启动的日常核心。发布产物保留旁路 `cerulean-runtime.exe`。

这些夹具不使用真实模型或凭证。它们能验证本地协议与恢复顺序，不能替代官方 Codex 当前版本的真实取消、远端执行停止和真实产物验证。持续调度、多模型接入、副指挥纠偏及全部 P0–P6 门槛仍按原实施方案推进。

### 一次真实调用与已定位缺口

`reports/p2-codex-cancel-live-20261008-1939/test/result.json` 为 **inconclusive**：真实 Codex 0.160.1 / gpt-6.1-sol，1 次 Host 启动入口、1 条 DispatchIntent、1 条 Started、1 条 CancelIntent，最多一次取消和一次只读对账。测试断言通过只证明单次调用与不重发等约束，不能写成取消验收通过；主副指挥回复和用户批准仍为合成夹具，真实管理 API 调用 0。

Started 至 CancelIntent 约 41 ms（按两条账本 receivedAt 为 40.6656 ms）。第一次错误是 `RPC -32600`，旧错误未包含请求方法，因此不能断言它来自 interrupt 或紧随其后的 thread/read。随后对账为 `attempt missing or ambiguous`，最终及重开均 Unknown/Paused，新派发和新管理请求均 0。原始历史、Started 回执和绑定已保存，原始远端证明正文没有被产品持久化，不能补造。

一次额外只读诊断 `readback-structure.json`（仅 initialize/thread/read，无新任务或 interrupt）证实：原 thread/workspace 匹配、唯一原 turn、`interrupted`、`itemsView=full`，但 items 和 userMessage 均 0。本次精确会话的本地日志仅有 session_meta/task_started 结构；只保存结构及源文件摘要，不复制账号、凭证或思考内容。见 `early-cancel-analysis.json`。这说明当前要求输入 clientId/prompt 的严格证明无法匹配；不能把“回合中断”直接冒充现有证明条件已经满足。

下一批先设计并验证独立的早期取消证明，绑定原持久化 Started/CancelIntent 与完整唯一 turn；不得伪造 prompt/clientId，不放宽 completed 或没有原 Started 的不确定派发。详见 [早期取消修复](CODEX-EARLY-CANCEL-NEXT.md)。仅增加延迟来避开窗口、重发任务或重复 interrupt 都不能作为修复。

真实测试必须通过 `scripts/test-codex-cancel-live.ps1`：挂起创建测试进程，归属私有 Windows Job Object 后恢复，210 秒上限及最多 10 秒清理，仅处理本次进程树。`cleanup.json` 显示未超时、退出 0、RemainingProcesses=0；无模型自测 `cancel-job-selftest-20261008-02/cleanup.json` 的正常/超时/异常三条路径均归零。继承原进程环境，不宣称环境白名单或所有输出普遍脱敏；不读取其他应用凭证。

已补最小诊断修复：RPC 错误仅含白名单方法名和数字 code；匹配失败附缺失/重复分类、已知回合存在性、固定状态/itemsView 枚举及安全计数，不输出正文、ID 或路径。新增 4 项测试，接受条件保持不变；这项可观测性修复不能把上述真实记录升级为已验收。

19:52 最终诊断版的 `p2-codex-cancel-diagnostics-build/debug-build-20261008.log` 构建通过，`codex-cancel-native-dNcQv2/result.json` 再次验证 ACK 丢失及原生只读恢复通过（模拟模型调用 0），cleanup 完整且恢复截图已检查；`p2-codex-cancel-final-release-20261008/release-launch.json` 正常发布启动/关窗/常驻读取通过，日常新计划 0。`p2-codex-cancel-process-cleanup-20261008.json` 核查产品及 helper 进程剩余 0。证据索引与 SHA-256 见 `p2-codex-cancel-review-20261008.json`。完整 P0–P6 仍未通过，早期取消规则待修复。

核对依据：[OpenAI App Server 官方说明](https://learn.chatgpt.com/docs/app-server) 将 interrupt 成功响应与最终 interrupted 状态区分，并说明 started 通知的 items 为空；该说明本身没有给出本次空输入回合的产品证明规则，后者仍需本项目实现和验证。

## 源码索引

- [Host 取消入口与候选](D:/test/CeruleanLonging/crates/core/src/codex_cancellation.rs)
- [执行线程和 Job 恢复](D:/test/CeruleanLonging/crates/core/src/async_codex.rs)
- [持久化执行协议](D:/test/CeruleanLonging/crates/core/src/codex_execution.rs)
- [Codex transport](D:/test/CeruleanLonging/crates/adapters/src/codex.rs)
- [运行暂停与恢复门禁](D:/test/CeruleanLonging/crates/core/src/project_runner.rs)
- [执行记录与停止确认界面](D:/test/CeruleanLonging/apps/desktop/src/CodexJobs.tsx)
- [原生取消验证脚本](D:/test/CeruleanLonging/tests/codex-cancel-native.mjs)
