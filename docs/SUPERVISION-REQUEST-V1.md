# 活动执行中的副指挥建议请求与审阅

日期：2026-10-09 01:03 +08:00。属于 P2/P5 的监督组件，完整 P0–P6 尚未通过。本批新增自动请求和保存，尚未实施本地 Advise 应用、turn/steer 或自动纠偏送达。

## 实际行为

用户批准的监督策略每次尝试建议额度大于 0 时，Active 项目的 Codex 执行者发生达到阈值的连续同类失败，可由宿主发起一次无工具副指挥请求。默认额度 0 继续只记录异常。管理模型取原执行配置中的 deputy_model，使用产品本机配置的 OpenAI 凭证；这批验证均使用明确的离线管理替身。

请求绑定原运行授权摘要、方案、任务、attempt、thread、turn、incident 与已可靠保存的观察摘要。仅接受仍在执行且可控制的原 worker、未取消/修订、未超时、无观察遗漏/拒绝/存储错误的状态。同项目未决管理请求继续互斥；没有放宽通用 require_available。确定性的 advice 操作 ID 去重，请求意图先于单次管理调用保存。

项目管理总预算和每 attempt 的建议上限共同生效。Unknown、显式放弃和重启均不返还已请求次数。新请求没有业务工具或新的执行权限，副指挥回复仅允许对应 task 的 Advise 或 Wait；错误任务返回 Unknown。

建议正文与理由按需从指挥记录读取，普通 Snapshot 不携带正文。原生详情区分已保存正文、结果未知、当前或过期观察，并明确标示尚未送达执行器。Ready 建议仍未消费，worker 结束后调度进入 Attention；Unknown 阻止后续调度。重复轮询和核心重启都不重发，也不派发后继任务。Wait 在此专用路径中也尚未自动应用，不能视为已实现完整监督交互。

启动时交叉检查管理意图、观察摘要/异常、原方案策略、Started 事件、原配置及运行授权历史。RunLedger 的授权摘要缓存从已验证的历史事件重建，不改变现有文件格式。

## 验证与证据

- 189 项常规 Rust 测试、4 项 Node 测试通过；fmt、严格 clippy、TypeScript/前端、调试与发布构建通过。日志：reports/p2-advice-request-checks-20261009.log、p2-advice-request-clippy-20261009.log、p2-advice-request-debug-20261009.log、p2-advice-request-release-build-20261009.log。
- 16 类请求门禁和放弃后的持久额度测试：前者使用明确的合成活动状态；后者证明管理账本重开后的计数，不冒充完整宿主的实况恢复。
- 原生保存路径：reports/advice-request-native-3YmKpx；响应未知：4Ye3kU；错误任务：hrfBFZ（均为相同目录前缀）。三者由实际自有 worker 进程触发异常及建议请求，完整核心重启后正文/未知状态保持。每次副指挥请求 1 次，总管理请求 3 次（计划、分配、建议），A 派发 1 次、B 派发 0 次、Advise 工作流事件 0、独立验收 0；这些不是完成编程项目的证据。
- 仅记录异常回归：reports/supervision-incidents-native-rCzeMO；监督配置/批准：reports/supervision-policy-native-qsSFuV。新增请求没有改变额度 0 的两任务观察路径。
- 发布启动：reports/p2-advice-request-release-20261009/release-launch.json；新计划 0、正常关窗后核心仍可读、页面错误 0。
- 所有协议模型响应与管理决定均为离线替身，真实模型调用 0。原生截图已检查。未决 Submitted 样例拒绝正常停止时，测试仅结束自己创建的核心进程；相关 PID/结果记录在各 native-result.json。

错误任务场景现可用 node tests/advice-request-native.mjs --wrong-task 复现。先前直接 cargo 调用未创建 CERULEAN_DAG_DIR，启动前即失败，保留 p2-advice-wrong-task-final-20261009.log，不计通过；随后用创建独立目录的原生入口通过。更早 saved 夹具分支落在计划循环的失败报告同样保留，修正后再验证。

本批证据索引为 reports/p2-advice-request-review-20261009.json。旧索引仅代表当时版本，不以新源码覆盖其历史含义。

## 直接下一步

专用监督回复应由持有工作流的唯一 worker 应用，宿主只核准发送范围及运行授权。需要分开记录管理响应、本地 Advise、送达意图和远端 ACK；没有回执时保留未知，不能重发。预计扩展 async_codex 的窄控制消息和进度回执、codex_execution 的受控等待、CodexClient 的原 turn steer。取消优先、暂停撤销未发送建议、自然完成先于建议时保存真实完成，不创建新 turn 或 attempt。

进入实现前沿用本地已导出的官方协议核验 expectedTurnId/input/turnId；ACK 只能证明接收，不能证明执行或成功。边界测试必须覆盖半帧、通知洪流、取消/暂停竞争、回复过期、应用或 ACK 丢失及宿主中断。真实管理建议与真实 Codex steer 仍须独立实测。完整其他供应商、桌面接管、并行隔离和安装升级门槛不变。
