# 阶段验收评估

评估：2026-10-08 23:46 +08:00，依据本地源码、常驻 Codex 编程与恢复、独立检查、真实管道、OpenAI 客户端及原生加密凭证证据。**P0/P1 部分完成，P2 多项组件通过，P0–P6 整体均未放行**。新增阶段间变更保护、指定边界真实宿主终止、显式验收补记及暂停后续调度的有限验收；新增Submitted整批处置、新批准和新尝试执行路径的有限验收；完整崩溃矩阵与监督继续。真实 OpenAI 主副指挥认证待用户本机配置 Key；其他工程可继续，持续目标 active。后续执行与证据门槛详见 [下一阶段验收顺序](NEXT-STAGE-GATES.md)。

| 阶段 | 已有实证 | 剩余放行条件 | 结论 |
| --- | --- | --- | --- |
| P0 契约 | 工具链/生成类型、固定角色、完整方案批准、尝试/回执/快照/报告关联、实际验收程序/固定文件/环境绑定与负面测试 | 间接依赖与运行时权限、真实模型身份、需求逐项测试映射、云 CI | 部分完成 |
| P1 核心 | 账本/恢复/日报、常驻服务/ACL、完整批准、真实独立验收、全输入副本、异步查询取消、显式放弃、持久化工具登记与原生操作 | 完整恢复/磁盘故障矩阵、任务取消/退出、集成租约、索引/游标/时区、备份/大账本/磁盘故障、后台工具登记、退出交互、沙箱 | 子模块通过，整体未通过 |
| P2 编程闭环 | RuntimeHost 真实后台 Codex 编程、账本回执、10 项独立检查；270 次客户端重连查询；模型/程序摘要进入批准；原生只读恢复同轮次，新增发送 0 次 | 真实 OpenAI chief/deputy 身份与权限、实际用户全流程、diff/活动取消/宿主崩溃/最终 chief 收口 | 常驻执行及原生对账组件通过，整体未通过；预批准夹具不替代真实业务流程 |
| P3 首批接入 | 保留 Codex/OMP/GPT/Claude/DeepSeek/Qwen/Kimi/xAI Grok 登记；旧原型有限证据 | 产品认证、工具流、CLI、取消/恢复、限流/预算、凭证生命周期逐项实测 | 未通过，登记不等于接入 |
| P4 桌面接管 | Qoder/WorkBuddy 旧原型证据保留 | 现有会话产品绑定、发送/读取/取消/恢复、用户竞争、多窗口、锁屏/升级矩阵 | 未通过，不能改成 CLI 替代 |
| P5 多任务效率 | 资源冲突、同目录串行与当前异步限额 | worktree/集成快照、端口/DB 隔离、异常纠偏、费用/人工时间、同标准并行对照 | 未通过，不宣称提效 |
| P6 发布 | 本机 release 构建、真实启动及关闭后核心读取 | 安装/卸载、升级/回滚/迁移、签名/渠道、干净机器及全部首批支持范围 | 未通过，本机 exe 不等于发布验收 |

## 本批证据
23:46进度：已提交未收口任务可由用户明确结束本版并修订，新事件RetireSubmittedPlan区别于普通修订；原Submitted/失败证据保持，新批准和新启动必需，历史调用不清零。180项Rust、4项Node与离线生产DAG返工、原生确认/重新批准/历史失败详情验证通过，见 [SUBMITTED-PLAN-RETIREMENT-V1.md](SUBMITTED-PLAN-RETIREMENT-V1.md)。直接下一步为异常观察和副指挥受控纠偏，完整崩溃矩阵、真实管理链及P0–P6门槛保持。

23:28进度：阶段间变更/锁定保护、指定边界的真实宿主终止，以及完整报告未入账后的用户显式补记已取得证据，见 [VERIFICATION-RECOVERY-V1.md](VERIFICATION-RECOVERY-V1.md)。补记重新核对原证据与链头、先暂停运行、只追加原Verify；失败/超时不冒充通过，部分报告拒绝，旧审阅和写入故障有测试。两任务生产路径证明补记后无自动Accept/后继派发，明确恢复后继续；原生确认和完整重启通过。完整崩溃矩阵、失败任务处置、监督纠偏及P0–P6仍未完成。

22:59批次：A→B两检查、主指挥收口和正常重开生产路径4场景通过；A未Accept时B派发0，失败恢复不重跑。主副指挥及Codex响应为离线替身，实际worker/validator子进程；不能冒充真实模型或进程崩溃。Pending整批修订已通过2项新增Rust测试及原生R9s8st，新方案移除未执行后继、重新批准/重启、旧事件保持通过。常规172Rust、4Node、另显式4DAG场景，fmt/clippy及构建通过；发布启动新增计划0，进程核查0。详见 [TWO-TASK-DAG-V1.md](TWO-TASK-DAG-V1.md)，索引reports/p2-dag-pending-review-20261008.json。M5/M6、失败显式处置与监督为直接下一步，完整P0–P6仍未放行。以下为历史批次。

22:38批次：终态修订与旧运行结束/恢复已实现，见 [PROJECT-REVISION-V1.md](PROJECT-REVISION-V1.md)。170项Rust、4项Node、fmt/clippy及发布/调试构建通过。原生project-revision-native-tmWmcs通过用户修订、配置解锁、主指挥合成决定应用为第二版、重新批准和完整核心重启，旧事件文件不变、worker派发0、真实模型0；四次核心正常退出。发布启动回归通过，新增计划0。原Verified详情跨版本仍可读；旧attempt不可复用、新fence递增；两类发布窗口故障和伪造结束拒绝通过。剩余A取消/B待执行的整批处置、多阶段故障/两任务DAG及监督继续，完整P0–P6未放行。索引reports/p2-project-revision-review-20261008.json。以下为历史批次。

21:42批次：按需只读验收阶段详情已实现并通过四种原生场景，见 [详情实现](VERIFICATION-DETAIL-V1.md)。166项Rust、4项Node、fmt/clippy及发布/调试构建通过。查询绑定原revision/manifest/attempt/operation，核对报告、阶段和日志摘要，不推进调度；无Verify时即使全部退出0仍为Unknown。原生成功yoZ2yw、失败Zj9Mcj、超时vdXMqN、待对账jO2LIk均通过完整核心重启，真实模型0；原取消UjDbgD及发布启动回归通过。下一步补多阶段故障/两任务DAG验收与终态方案修订，再继续监督纠偏。新确认缺口：Propose要求所有任务Pending，Accepted/Cancelled也无法修订，不能宣称终态可直接新批执行。完整P0–P6未放行，真实管理Key仍待本机配置。证据索引reports/p2-verification-detail-review-20261008.json。以下为历史批次。

21:20批次：有序多检查配置及批准已接入产品，见 [本批说明](MULTICHECK-CONFIG-V1.md)。支持1–8项已登记工具、独立名称/参数/时限及排序，完整列表进入方案和运行范围门禁，旧单项字节及摘要保持。163项Rust、4项Node、fmt/clippy及最终发布/调试构建通过；真实两检查成功/首项失败/次项失败/首项超时与禁止重跑通过。最终原生multicheck-native-YJZNwg完成增删排序、批准展示、返回、完整核心重启和零派发，真实模型0。发布启动通过。直接下一步是只读阶段结果详情、多阶段故障补测及A→B依赖验收，再接副指挥监督纠偏；M1–M7/U1整体尚未放行，完整P0–P6未通过。证据索引reports/p2-multicheck-review-20261008.json。以下为历史批次。

21:01批次：早期取消的独立停止证明已实现并通过原真实尝试的只读恢复，详见 [实现与证据](CODEX-EARLY-CANCEL-V1.md)。最终159项Rust、4项Node、fmt/clippy、发布/调试构建通过。新原生空输入取消EvRE8g及空输入丢ACK后核对gF3jLJ通过，旧三种取消、启动恢复和应用恢复回归通过。真实恢复只读四种RPC各1，新任务/interrupt/管理API均0；旧数据和历史前缀不变，仅追加一条双锚点证明，重开为Cancelled/Paused。原首次在线测试仍inconclusive，旧-32600的方法仍不可追溯；恢复成功不等于首次全程成功。完整P0–P6仍未放行；下一步按CODEX-SUPERVISION-NEXT.md接有序多检查配置与详情，再接副指挥受控纠偏。真实OpenAI管理Key待本机配置。证据索引reports/p2-early-cancel-review-20261008.json。以下为历史进度。

19:48批次：活动取消已实现先暂停、队列投递、原worker先保存CancelIntent再最多一次interrupt、终态与完整读回确认、自然完成竞争保留Submitted和只读恢复；详见CODEX-CANCELLATION-V1.md。149项基础回归加4项诊断测试，最终153项Rust、4项Node通过，严格静态检查通过。三路原生3TmtlE/FULc35/i3vgQR分别证明Cancelled、Submitted及Unknown重启后核对为Cancelled；各start/interrupt/read为1，取消后新增派发0，真实模型0。首次脚本文案匹配错误及格式差异日志保留。

真实单次reports/p2-codex-cancel-live-20261008-1939/test/result.json为inconclusive，不能放行真实活动取消。Started至CancelIntent约41ms，首次RPC -32600（旧日志方法未知），对账缺用户输入；额外一次只读thread/read证实原唯一turn已interrupted/full，但items/userMessage为0。严格证明保持Unknown/Paused，无新派发。进程由私有Job Object归属与清理，真实cleanup剩余0，正常/超时/异常三种无模型清理自测均通过。原始结果不改写，独立早期取消证明须下一批实现，不能伪造clientId/prompt。新增诊断只输出白名单方法与有限结构计数，不改变接受规则。

旧启动恢复startup-managed-native-hIFUp0及发布p2-codex-cancel-release-20261008回归通过，所有模拟场景清理完整；Submitted/Unknown场景采用仅限已拥有测试核心的强制中断，不宣称全部正常退出。下一步先修CODEX-EARLY-CANCEL-NEXT.md，再按CODEX-SUPERVISION-NEXT.md补多检查配置与监督纠偏。真实管理Key缺项及原P1/P3–P6均保留。以下为历史批次。

15:48批次：不可变启动意图与显式关闭完成，见STARTUP-RECOVERY-V1.md。新意图先于Job/线程写入，managed/unmanaged/missing-job三路支持只读详情与候选摘要、用户确认、原证据保留和attempt永久预留。管理.4与启动.1双向关联，工作流/fence/请求计数不变。所有启动、应用、配置、运行和编程关联纯校验先于管理回执恢复；跨账本冲突不会提前给另一记录补.3/.4。

Rust135、Node4、fmt/clippy及发布/调试构建通过，新增20项Rust测试，包括生产入口Job写失败、跨账本冲突、管理关闭引用、旧应用关闭后新attempt重开。reports/startup-managed-native-x2kxPk、startup-unmanaged-native-JR3oOi、startup-missing-job-native-MPVs6Z三场景result.json均通过真实界面和完整核心重启，各新增启动关闭1条、工作流/派发0，managed另有管理.4一条。三张截图已检查；三场景及旧application-recovery-native-BF9d3l回归cleanup均0。发布p2-startup-recovery-release-20261008/release-launch.json通过，日常0新计划。证据摘要p2-startup-recovery-review-20261008.json。

本批真实模型调用0；合成证据不代表真实连接/建会话故障矩阵通过。legacy缺来源记录仍只读，不自动升级；已知旧协议接纳/迁移、容量及多窗口竞争待完成。原完整设计矩阵保留；继续活动取消/纠偏、多阶段验收、真实主副指挥API和原P1/P3–P6。以下为历史批次。

15:02批次：未提交应用事务的用户关闭完成，见APPLICATION-RECOVERY-V1.md。Rust115、Node4、严格fmt/clippy、发布/调试构建通过；新增10项常规测试含Prepare落.2后丢ACK、无job未决工作流拒绝、未关闭.2跨账本冲突启动拒绝、投影写失败后修复、旧关闭历史前缀及四种本地目标。只关闭管理事务，不修改原批准、workflow或fence，不退请求预算，也不自动恢复。

reports/application-recovery-native-a1eX7T/result.json和application-no-job-native-c9B5XM/result.json均通过真实原生确认、返回、旧摘要拒绝、唯一.4与完整核心重启，新增工作流/派发0、真实模型0。三张截图已检查；两场景及unsent-assignment-native-RLwcBv回归cleanup均退出0。p2-application-closure-release-20261008/release-launch.json发布启动/正常关窗/常驻读取通过，日常0新计划。证据摘要p2-application-closure-review-20261008.json。完整矩阵仍有生产故障组合与真实模型缺项；下一步job已写但无.2的启动窗口，再继续取消/纠偏、多阶段及原P1/P3–P6。以下为历史批次。

14:31批次：未派发分配的用户放弃及持久恢复完成，见UNSENT-ASSIGNMENT-V1.md。常规Rust105、Node4、最终严格fmt/clippy、发布/调试构建通过。reports/unsent-assignment-native-1oCRRc/result.json和unsent-no-job-native-hzNaG5/result.json分别证明有job和无job的真实原生确认、空白/过期证据拒绝、返回不落盘、重复处置拒绝及完整核心重启；两场景均仅新增1条User处置事件，新增派发0，原管理链/批准/Assign保持。三个关键截图已检查，两个测试核心cleanup退出0。

补充故障测试证明历史DispatchIntent后NotStarted仍不能关闭、活动/断线线程仍占有时拒绝、Assign已写而管理.3丢失时重开先补回执、处置事件已写而job投影丢失时恢复，以及单改job为Abandoned不能伪造处置。旧管理operation/旧attempt不可复用，新attempt预检及递增fence有效。本批真实模型调用0；p2-unsent-release-20261008/release-launch.json发布启动/关窗/常驻读取通过，日常0新计划。完整P0–P6均未放行，下一步Applying无事件处置，然后取消/纠偏/多阶段检查和原P1/P3–P6范围。以下为历史批次。

14:06批次：管理历史摘要分页、按需全文及精确全局统计完成，见MANAGEMENT-HISTORY-V1.md。常规Rust97、Node4、最终fmt/clippy、发布与调试构建通过；管理记录原生规模reports/management-history-native-h0E43U/result.json证明1000条最长合法文本、两项目各500、全量分页无重复/缺失、完整详情哈希、过期版本/单条摘要拒绝及核心重启。实际完整ServiceReply最大215316字节，低于4MiB；历史正文不随每次Snapshot发送。

management-review-native-Ghoaut/result.json证明处置/Wait答复按需全文及重启回归；已有规划/批准/合成凭证回归通过desktop-regression日志；p1-management-history-release-20261008/release-launch.json发布启动/正常关闭/常驻可读，日常0新计划。分页与答复截图已目视检查，测试核心清理退出0。本批真实API/worker调用0。其他全量Snapshot字段、超过1000请求的归档与长期性能仍需验证；P1/P6不能据此整体放行。下一步专门处置Assigned未发送尝试和Applying未提交事务，再继续取消/纠偏及原首批接入/发布范围。以下为历史批次。
13:47 批次：用户放弃未应用指挥请求及答复 Wait 已落盘并接原生界面，见 MANAGEMENT-RESOLUTION-V1.md。95 项常规 Rust、4 项 Node、额外 3 项桌面缓存测试与最终严格检查通过；发布/调试构建通过。management-review-native-t4XFQE/result.json 证明明确确认、取消不落盘、Wait 原文及答复、完整核心重启恢复、无新请求/批准/执行，截图已检查。本批真实 API/worker 模型调用 0。累计请求预算包含已放弃请求；保存答复不自动恢复。

WebView2 冲突已按每进程缓存隔离修复，见 NATIVE-INSTANCES-V1.md；native-instances-YF3yL4/result.json 证明 debug/release 三窗口并发、共享核心计划批准、重开、活跃/外来/junction 缓存边界。原 desktop-smoke-WNq5RT/failure.json 保留。空闲窗口尚不会自动同步其他窗口修改，测试明确刷新；不能推断全天稳定性或全 WebView2 版本支持。已有批准/规划/凭证原生回归通过，p2-management-resolution-desktop-regression-20261008.log。

复查发现大规模 Snapshot 管理问答全文可能超过 IPC 4 MiB，须补历史分页/按需详情与规模测试。Assigned 未发送尝试、Applying 处置、活动取消/监督、多阶段验收及全部 P1/P3–P6 继续。模型输入已限制问答24KiB及整体64KiB剩余容量，历史完整保存，省略数明确。真实 OpenAI Key 仍待本机配置。以下为历史批次。
08:51 批次：持续执行组件已串联，见 [持续执行](PROJECT-RUNNER-V1.md)。用户运行授权/暂停持久化，副指挥→Codex→独立检查→chief 收口→下一任务；未知请求和失败检查不自动重发。Rust 90 项、Node 4 项、严格检查和发布/调试构建通过。真实 reports/p2-project-run-live-20261008-084441/result.json：1 次 Codex worker、1 条发送意图、10 项独立断言、Accepted/Completed，重开新增请求 0。主副指挥 3 条回复及用户批准为夹具，真实管理 API 调用 0，完整 P2 未放行。

原生 reports/project-runs-native-Zheyj0/result.json 及已检查截图证明暂停显示、无 Key 禁用恢复、启动确认、取消不执行和重开保留。已有入口 desktop-smoke-9t6LQg/result.json、发布 p2-project-run-release-20261008/release-launch.json 通过，日常新计划 0。首次并行原生启动 desktop-smoke-WNq5RT/failure.json 出现 WebView2 0x8007139F，串行复测通过，但多实例策略仍为 P1/P6 缺项。受限构建失败及普通宿主重建成功记录均保留。

下一步补未知决定/Assigned 未发送尝试的显式处置、Wait 补充信息、多阶段验收和活动取消/监督。真实官方 Key 待用户本机配置，其他工程继续。以下为历史批次。

08:37 批次：原生项目需求/执行模型/验收工具/产物/请求预算配置、持久化版本、主指挥窄请求入口及未批准方案自动应用已接入，见 [项目规划](PROJECT-PLANNING-V1.md)。最终 Rust **87 项**、Node **4 项**通过；报告 p2-project-planning-final-checks-20261008.log、final-clippy、build、debug-build。配置不授予执行批准，任务数超过编程调用容量被拒绝，主副指挥请求预算修改配置不清零。

原生 reports/desktop-smoke-qtkbNr/result.json：配置填写/保存/重开、程序与验收绑定、参数保真、保存不生成/批准、缺 Key 禁用请求通过，同时回归原生批准和合成 Key 保存/删除。project-planning.png 已目视检查。发布 p2-project-planning-release-20261008/release-launch.json 启动/关窗/常驻读取通过，日常 0 新计划。本批真实 API/模型调用 0，主指挥回复为合成测试，尚无真实主指挥全流程证据。

配置当前一套 worker/验收/产物作用于整份计划，多阶段不同检查范围未实现；批准后持续自动调度、运行授权/暂停/恢复为直接下一步。Wait 补充信息的 UI 文案尚未重建，未知决定/未派发 Attempt 处置仍需完善。P0–P6 整体结论保持。以下为历史批次。

04:02 批次：副指挥决定到真实 Codex 会话准备/分配/发送的两次确认桥接完成，详见 [桥接机制](CODEX-MANAGEMENT-BRIDGE-V1.md)。常规 Rust **83 项**、Node **4 项**通过，reports/p2-managed-codex-full-checks-20261008.log；严格检查 p2-managed-codex-clippy-20261008.log；发布构建和实际启动通过，p2-managed-codex-release-20261008/release-launch.json 正常关窗后核心可读、0 新计划。

另显式运行一次真实测试：reports/p2-managed-codex-live-20261008-035625/result.json。真实 worker 1 次模型任务生成 sum.mjs，10 项独立断言通过；1 条管理分配、1 条发送意图，应用回执关联同一分配事件。重启重复请求被拒绝，新增发送 0 次；1388 次为宿主轮询，不是客户端重连/性能结论。主副指挥模型调用 0，方案、批准和副指挥回复为夹具，没有 chief Accept，完整 P2 尚未通过。

新增两项常规故障测试覆盖丢失任一确认、上下文变化及假冒已提交事件。Assigned 但尚未形成发送意图的显式处置仍待实现；原生需求/实际接入与验收配置、主副指挥模型/预算、真实批准和指挥全流程为直接下一步。P0–P6 整体结论保持未放行。以下为历史批次。

03:41 批次：指挥应用意图、角色/任务/工具绑定、工作流事件关联与应用回执已接入，提交后中断仅补回执，提交前中断不自动执行；真实用户批准和独立验收不可被模型回复替代。新增 6 项测试，最终 Rust **81 项**、Node **4 项**通过，见 reports/p2-management-application-final-checks-20261008.log；final-clippy 和 build 日志通过。发布 p2-management-application-release-20261008/release-launch.json 正常，关闭后服务可读，0 新计划。详见 [指挥应用与工作流对账](MANAGEMENT-APPLICATION-V1.md)。

首次完整回归虽测试通过，但并发启动留下较晚接管的测试服务，已核实来源并正常停止；修复客户端启动句柄归属，增加关闭后端点/锁观察，再完整回归通过。initial 编译日志保留一次可见性错误及修复，旧 full-checks 保留初次回归，最终以 final-checks 为准。本批 API/模型调用 0。

应用事务通过不代表全链路通过：现有 Codex 启动器自行创建 Assign，与本批已 Assigned 路线尚需共用真实会话/Attempt；原生需求与模型/预算配置、实际用户批准和真实供应商验收未完成。Unknown/未完成应用显式处置及残留发布文件恢复仍待补，P0–P6 整体结论不变。以下为历史批次。

03:23 批次：请求意图与响应证据、RuntimeHost 指挥后台单次调用、崩溃后不重发、当前上下文校验已实现，见 [指挥请求持久化](MANAGEMENT-JOURNAL-V1.md)。Rust **75 项**、Node **4 项**通过，报告 p2-management-journal-full-checks-20261008.log；严格检查 p2-management-journal-clippy-20261008.log 与发布构建 p2-management-journal-build-20261008.log 通过。发布启动 p2-management-journal-release-20261008/release-launch.json 正常，日常 0 新计划，关闭后服务可读。

新增 6 项故障/宿主测试使用确定性传输替身，真实 OpenAI API 与模型调用 0。Ready 仅代表已保存有效回复；当前尚无角色应用事务、决定消费/显式处置、用户发起入口及项目统一指挥预算。已有决定继续占用同项目，不能宣称完整连续调度通过。下一步补应用意图及工作流事件关联，再接实际用户全流程。P0–P6 整体结论不变。以下为历史批次。

03:10 批次：reports/p2-openai-full-checks-20261008.log 中 Rust **69 项**、Node **4 项**通过；p2-openai-clippy-20261008.log 严格检查通过，desktop-build/debug-build 日志证明最新构建通过。客户端固定官方 HTTPS 端点，无工具、严格响应结构、不跟随重定向、不重试；回环 HTTP 测试不等于真实供应商验收。新增 Windows DPAPI 凭证存取及原生入口。

reports/desktop-smoke-4Zo96T/result.json 证明原生合成 Key 保存、输入框清空、加密文件、重开及删除；截图已检查。p2-openai-release-20261008/release-launch.json 证明发布启动和关闭后核心读取。证据与源码摘要见 p2-openai-review-20261008.json。本批真实 OpenAI API 调用及模型调用均为 0。详见 [无工具指挥通道与凭证](OPENAI-MANAGEMENT-V1.md)。

该客户端尚未连接持久化决策意图和实际角色回调；不代表主副指挥链已经完成。P3 仅增加凭证组件，其他接入门槛不变。真实验证待用户在产品配置官方 Key，不能从其他软件读取或导入凭证。以下为历史批次。

02:43 批次：reports/p2-host-codex-final-checks-20261008.log 中 Rust **63 项**、Node **4 项**通过；p2-host-codex-final-lint-20261008.log 严格通过；final-build / final-debug 日志证明发布及调试构建通过。新增测试拒绝未批准/旧摘要/程序替换/重复 Attempt；重开不信任无账本的成功标记、拒绝更换 worker。

真实常驻 tests/strict_workflow 的 live_codex_resident_worker_and_recovery 另行显式运行：reports/p2-resident-codex-20261008-023403/result.json。本批 1 次模型编程发送，客户端每次读后断开并重连共 270 次，最大单次查询 2 ms；独立检查通过，账本副本重启对账新增发送 0 次。该时延仅为本机小夹具数据，不代表大账本性能。方案和主副指挥回调仍由夹具预设。

原生 reports/desktop-smoke-rGKf7u/result.json 证明批准前审阅模型/程序路径与摘要并保存/重开；native-codex-recovery-5JTvNo/result.json 证明实际按钮、核对中状态、终态轮询、无自动 Verify、正常关窗后核心仍保留结果。截图已目视核对，原生对账无新模型发送。p2-host-codex-release-20261008/release-launch.json 发布启动通过、日常 0 新计划。汇总见 p2-host-codex-review-20261008.json 与 [常驻执行](CODEX-RESIDENT-V1.md)。

中间 clippy 大枚举警告已用 Box 修复；Vite 在受限环境 spawn EPERM，正常宿主构建通过，没有降低产品 ACL。所有失败与通过记录分开保留。以下为历史批次。

02:14 批次：reports/p2-dispatch-full-checks-20261008.log 中 Rust **61 项**、Node **4 项**通过；p2-dispatch-clippy-20261008.log 严格检查通过。另显式运行两项真实集成测试：p2-live-dispatch-20261008-020440/result.json 证明一次 turn/start 实际生成 sum.mjs，外部固定检查器 10 项断言通过，重开为 Submitted/verified=true；recovery-result.json 证明读回同一原轮次，恢复新增发送 0 次。测试预设方案和角色，不证明真实 chief/deputy/用户批准，也未产生 Accept。详见 [派发与恢复](CODEX-DISPATCH-V1.md)。

p2-dispatch-desktop-build-20261008.log 发布构建通过；p2-dispatch-release-20261008/release-launch.json 实际启动、正常关窗、关窗后核心读取通过，无新计划、无页面错误。当前批次没有改 UI，因此沿用上一批原生按钮测试作为历史覆盖，不把它冒充本批重新执行。证据文件及 SHA-256 汇总于 p2-dispatch-review-20261008.json。

首次恢复夹具因缺少 bindings.json 在远程读回前失败，已修正并在新目录通过，原 recovery-test.log 和目录保留；恢复是截取真实账本前缀的故障注入，并非声称发生了真实掉线。以下为历史批次，早期“模型调用 0”仅适用于相应批次。

01:52 批次：Rust **57 项**、Node **4 项**通过，见 reports/p2-codex-full-checks-20261008.log；严格检查 p2-codex-final-clippy-20261008.log。正式 Rust app-server 连接及工作会话建立分别见 p2-codex-live-connection-20261008.json / p2-codex-live-binding-20261008.json，模型调用 0。原生 reports/desktop-smoke-48ZLPZ/result.json 及 codex-connection.json 确认按钮、异步刷新、真实现有登录和模型目录。发布启动 p2-codex-release-20261008/release-launch.json 通过，日常 0 计划。详见 [Codex 正式通道](CODEX-ADAPTER-V1.md)。以下为历史批次。

最新 01:29：工具绑定与登记完成。reports/p1-catalog-host-checks-20261008.log：Rust **53 项**、Node **4 项**通过；p1-catalog-clippy-20261008.log 严格检查通过。p1-catalog-host-build-20261008.log / p1-catalog-debug-build-20261008.log 构建通过。reports/desktop-smoke-2lSuP0/result.json 证明原生工具路径/摘要/环境审阅、登记落盘和重开恢复，模型调用 0；截图已检查。p1-catalog-release-20261008/release-launch.json：发布启动、正常关窗、关闭后读取通过，日常 0 计划。

受限执行环境下 p1-catalog-checks-20261008.log 曾在同用户管道处“拒绝访问”；已定位和关闭本轮临时服务，正常宿主权限回归通过，未放宽产品 ACL。p1-catalog-build-20261008.log 保留受限环境 Node 探测 Rust 失败，宿主构建通过；这不是 Rust 缺失或测试放行证据。全部原失败日志保留。

最新 00:50：Rust **48 项**、Node **4 项**通过；最后严格工作流 11 项复核、全 target clippy、发布构建通过。原生放弃 8 项见 native-verification-JFp9u5/result.json；发布启动见 p1-reconciliation-release-20261008/release-launch.json。放弃保持原证据/任务未通过，重启恢复且审计后篡改回 Unknown。详见 [显式放弃](VERIFICATION-RECONCILIATION-V1.md)。以下 00:30 为历史批次。

reports/p1-async-checks-20261008.log：Rust **46 项**、Node **4 项**通过。补不确定结果分类后，p1-async-recovery-final-tests-20261008.log 严格工作流 9 项通过，p1-async-recovery-final-clippy-20261008.log 全 workspace/全部 target 严格通过。

reports/native-verification-cX7q5U/result.json：原生显示进行中、点击停止、自动终态刷新、磁盘取消、CLI 确认任务未通过、正常关窗和无页面异常，共 7 项通过；截图已目视检查。p1-async-final-build-20261008.log 发布构建通过；p1-async-release-20261008/release-launch.json 启动/关闭/常驻读取通过，日常 0 计划。

测试先证明真实检查已经运行，再测宿主查询与取消，拒绝重复/错误 ID/重叠登记。重启以 Verify 账本恢复，缺失或截断结果归 Unknown，不换 ID 自动重跑。该历史批次的 7 个 ignored 都是由父测试显式启动的辅助入口；当前另有 2 个须显式选择的真实 Codex 集成测试，常规回归不会调用它们。

## 直接下一步

1. 显式放弃已绑定当前证据并保留记录；继续完善未入账报告的可信对账与任务取消/退出，不允许填 passed=true。
2. RuntimeHost worker 生命周期、批准模型/程序绑定、可信派发回调、异步状态与原生只读恢复已完成。直接推进真实 OpenAI 主副指挥的身份/工具隔离、结构化方案、实际用户批准和最终收口；同时补 diff、活动取消及崩溃/进程树验证。间接依赖和权限隔离仍需补。
3. 补 P1 存储/规模/退出/系统隔离，并继续 P3–P6 全部原范围。

最新凭证批次模型调用 0；02:43 批次发送一次真实模型任务，恢复新增发送为 0。未输出凭证或发送外部聊天。订阅金额预算不可计量，带金额预算的该路线拒绝派发；请求次数仅约束工作台发送次数。同步磁盘、复制阻塞 I/O、同账户权限、动态依赖/绝对路径和最终集成写入竞争仍有限制。详情：[异步宿主](ASYNC-VERIFICATION-V1.md)、[受测副本](SOURCE-SNAPSHOT-V1.md)。前日记录：[10 月 7 日矩阵](STAGE-ACCEPTANCE-2026-10-07.md)。

