# Codex 早期取消的独立停止证明

2026-10-08。已实现独立证明和原真实尝试的只读恢复；不代表首次在线取消无错误，也不代表完整 P2/P6 验收。原设计与负例矩阵见 [实施边界](CODEX-EARLY-CANCEL-NEXT.md)。

## 已实现行为

适配器将一次 thread/read 分为 RecordedInput 与 EmptyInterrupted 两种结果。原输入证明的字段、JSON 与检查保持；新结果不含 clientMessageId 或 promptSha256。空输入结果只允许明确启用该路径时解析，必须同 thread/canonical workspace、唯一原 turn、显式 full/interrupted/items=[]。completed、failed、inProgress、重复 turn、缺视图和矛盾输入均不能借此通过。

核心启用前从可信账本找出唯一真实 Started 和随后 CancelIntent，核对 DispatchIntent→Started→CancelIntent 顺序、当前 attempt/revision/fence/provider 绑定、启动 Running receipt、同 thread/turn/workspace 和本地用户取消来源。Reconcile(Running) 不能代替 Started，缺任一锚点继续使用原严格路径。空输入结果不能提交产物或产生验收通过。

新增 `ExecutionEvidence::StartedTurnCancellation`：保存原两事件序号/哈希、规范化读回和观察时间；回执摘要绑定整个证明。调度器在写入及重放时重新检查锚点、结构、时间、回执和命令证据，仅允许 Reconcile(Cancelled)。不改旧事件、旧回执 JSON 或旧链摘要；旧版遇到新证据类型会拒绝读取，不支持删除证明字段来降级。

用户停止后仍保持 Paused，不自动恢复或重发。常驻核心的 recovery 使用专门只读 transport：只允许 initialize/account-read/model-list/thread-read 四种 RPC（实际方法名保留斜杠）；其他方法在写入任何通信字节前拒绝。固定方法计数不记录参数、消息正文、凭证或思考内容。

## 当前证据

- `reports/p2-early-cancel-final-checks-20261008.log`：**159 项 Rust、4 项 Node**及 fmt/前端构建通过。较上轮新增 6 项常规测试（2 adapter、4 core）；表格驱动覆盖多个负例，不将每个分支另算一项。
- `reports/p2-early-cancel-clippy-20261008.log`：全 workspace/all-targets/custom-protocol 严格 clippy 通过。
- 原读取缺口复现保留在 `reports/p2-codex-cancel-live-20261008-1939/`，其中原 result 仍为 inconclusive，没有改写。
- 新恢复报告 `reports/p2-early-cancel-live-20261008-2050/test/result.json` **passed**：可信副本恢复同一真实原尝试，旧数据哈希及历史前缀保持，只新增一条证明；原 Started 序号5、CancelIntent序号6，读回原唯一 turn interrupted/full/empty。重开历史不变、Job/任务 Cancelled、运行保持 Paused。
- 本次真实恢复请求 initialize、account/read、model/list、thread/read 各1；新 turn/start、interrupt、真实管理 API 均0。原主副指挥回复与批准仍是合成条件，不能视为真实三级闭环。客户端正常关闭，外层私有 Windows Job 清理 RemainingProcesses=0，无超时。

回归包含：没有 Started、仅 Running 对账、没有 CancelIntent、错误来源或 workspace 不可进入空输入路径；篡改锚点、turn、视图、数量、时间、摘要及借证明提交完成被拒；即使重新计算被篡改末事件的所有摘要，重开仍因锚点不符拒绝。已有输入取消、自然完成竞争和原始丢 ACK 恢复保持。

初次编译遇到两处内部类型适配和旧忽略测试字段访问错误，已修复，未放宽门禁；初始失败日志保留。最终 fmt 与严格 clippy 见 p2-early-cancel-final-fmt/final-clippy-20261008.log；发布与调试构建通过。

原生空输入取消 reports/codex-cancel-native-EvRE8g/result.json，以及空输入丢ACK、完整核心重启后用户核对 reports/codex-cancel-native-gF3jLJ/result.json 均 passed；均使用生产异步worker及Tauri、确定性协议子进程，真实模型调用0。旧取消 interrupted/completed/ack-lost 分别为5B18Hs/rEyoVJ/4IqGCx，旧启动恢复WniWtg及应用恢复z17Pim均通过。所有cleanup完成；部分故障场景仅强制结束自建测试核心，不能记成全部正常退出。

发布 reports/p2-early-cancel-release-20261008/release-launch.json 通过正常启动、关窗及常驻读取，新增计划0；关键原生及发布截图已检查。p2-early-cancel-process-cleanup-20261008.json 记录产品与helper剩余0。汇总及源码/构建/报告摘要见 reports/p2-early-cancel-review-20261008.json。

## 边界与后续

本次证明“原已启动回合在早取消后可只读恢复确认”。首次 -32600 的具体请求方法在旧报告中缺失，仍不能追溯；新方法诊断可供后续独立实测，不把恢复成功写成首次在线取消全程无误。进一步的新真实任务必须单独统计。

继续有序多检查配置和阶段结果展示，再接事件触发的副指挥建议及受控纠偏，见 [后续实施边界](CODEX-SUPERVISION-NEXT.md)。真实 OpenAI 管理 Key、完整用户闭环、其他首批接入和 P1/P3–P6 原范围保持。
