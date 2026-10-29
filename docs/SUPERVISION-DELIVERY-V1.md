# 当前API开发增量 / 188阶段05 / 2026-10-17

API现可在原批准的工具边界请求并应用副指挥建议，建议在下一轮请求计数前进入消息。默认仍不投送；须明确勾选、重新规划批准。采用原API请求关联，不使用Codex turn/steer。状态区分本地应用、进行中、原回复与未知，重开不重发；正式范围与证据见[最新交接](HANDOFF-LATEST.md)。185冻结0.1.11不含本增量。下文为原批次历史契约，其中“仅支持Codex”的当前适用范围由本段更新；OMP/桌面主动监督仍待实现。

---

# 副指挥建议应用与原轮次送达

日期：2026-10-09。此文记录本批实现和已取得的组件、宿主与原生证据；最终发布结果在交接入口记录。完整 P0–P6 尚未验收。

## 产品行为与批准

新增监督策略 deliveryEnabled，默认 false 且 false 不写入 JSON，保留旧配置/方案摘要。只有建议额度非零且用户明确勾选“允许向当前执行轮次发送建议”、重新规划并批准后，调度器才自动应用和送达。原来的请求/保存能力保留；旧批准不会自动获得发送权限。

已批准的 Active 运行中，副指挥无工具建议响应匹配原任务、尝试、会话、轮次及持久观察后，宿主唤醒原 worker。唯一 worker 继续持有工作流和协议读取器，没有第二个 stdout 消费线程，也没有让副指挥直接操作业务文件。

应用链为：管理响应 → 宿主保存应用意图 → worker 追加副指挥 Advise 事件 → 宿主核对原文件与授权 → worker 保存送达意图 → 单次 turn/steer → 保存 ACK 或 Unknown → 宿主保存应用回执。后继派发还要经过独立检查和主指挥收口，送达 ACK 只证明接收。

依据：[官方 App Server 文档](https://learn.chatgpt.com/docs/app-server) 和本机 0.160.1 导出的 TurnSteerParams/TurnSteerResponse（reports/codex-supervision-schema-20261008）。输入只含 threadId、expectedTurnId、clientUserMessageId 和文本；不覆盖模型、工作目录、沙箱或工具权限，不创建新 turn。客户端消息 ID 用于关联，不假定服务端提供幂等重试。

## 持久化与竞争

送达账本位于产品数据目录 advice-deliveries/<project>/<attempt>/，每项保存发送意图与结果两份不可覆盖记录。其关联包括原管理操作、方案、运行授权、异常观察、响应摘要、Advise 事件摘要和文本摘要。发送前 flush/sync，未完成 pending 文件要求恢复审核；意图已存在就不再产生新的发送授权。未知不能通过刷新、重启或换连接自动重发。

核心启动交叉核对管理账本、原批准策略、Codex job、工作流 Advise 及送达关联；不能只凭文件哈希相符判真。已完成运行检查其原方案下的送达证据，新版本未决记录不污染旧版本的完成证据。未决建议会阻止当前项目继续调度。

暂停/取消撤销共享的发送许可。无未决建议且用户明确恢复时，新的 Active 授权可重新开放监督；未决旧建议阻止恢复发送权限，原响应仍受此前运行摘要约束。worker 在完整意图落盘后以一次原子状态转换开始发送；若暂停已先撤销，便不发送。若发送先开始，暂停不撤回已在途字节，仍收集回执。取消在取出新建议前优先；完整可读终态先保留原执行结果。等待被执行总时限中断、错误轮次 ACK、协议错误或回执超时，会使该连接不再允许后续写入，禁止重发。

发送许可失效但本地 Advise 已写入时，记录保留，后续显示应用/送达待核对；不会把该建议伪造为已接收。当前仍缺专用的用户对账/关闭流程及完整进程强杀矩阵，这些是后续工作，不能把保守停住等同于恢复能力已完成。

## 已取得的证据

- 协议层六组新增测试覆盖半帧保留、终态/取消优先、通知洪流、审批拒绝写入完整性、错误/缺失 ACK、外部超时丢弃 future、重复操作与只读/错轮次拒绝。
- 送达账本四组测试覆盖意图后重开、未知不重发、回执独立、重算哈希后的错轮次、sync 后故障、pending/缺失意图、Windows 路径与摘要校验。
- reports/steer-journal-process-0166e46e：实际 CodexClient 与自有离线进程，正常 ACK、错误 ACK、本地 ACK 未保存三种状态；各仅一个 thread/start、turn/start 和 steer，重开账本不再次授权。该测试的授权摘要是夹具，不能替代宿主链验证。
- 宿主完整 ACK：reports/supervised-dag-ack-c8a5e780。A→B 实际 worker 与独立 validator，2 次 turn/start、2 次 steer、2 条 Advise、7 次合成管理请求、2 个各含两检查的独立验收任务，两个任务均 Accept；完整核心重启新增事件 0。
- 错误 ACK：supervised-dag-wrong_ack-dd4d820d；丢失 ACK：supervised-dag-lost_ack-3ef5b517。各仅一次发送，后继 B 派发和独立验收均 0，重开保持未决。
- 发送前暂停：supervised-dag-pause-51e5c317；应用后暂停：supervised-dag-pause_commit-70ab7a49；取消：supervised-dag-cancel-a486a8a0。steer 均 0；应用后暂停保留一条 Advise，其余两条为 0；取消只发一次 interrupt，原任务 Cancelled。应用后暂停使用测试构建中的明确时点注入。
- p2-supervised-tamper-20261009.log：成功 DAG 数据独立副本中七类破坏均被拒绝，含重算完整链后的正文/运行/应用/响应/观察摘要/轮次变更和已完成运行缺失 ACK。原始报告保持。

以上管理决定和 app-server 响应均为合成替身，真实模型调用 0。可证明生产宿主事务、实际进程与协议行为；不能证明真实副指挥建议质量、真实 Codex 接收兼容性、计费或所有崩溃恢复情形。

## 下一步

补齐建议应用/送达未知的专用只读对账及用户明确关闭入口，保留原尝试与证据；基于真实进程中断验证应用、意图、ACK 各边界。然后用产品本机配置的真实管理认证和真实 Codex 验证原任务纠偏、独立验收和 chief 收口。真实 Key 仍待本机配置；其他首批 API/CLI、OMP、既有 Qoder/WorkBuddy 会话、worktree 并行/集成、效率对照和 P6 安装升级全部保留。

## 原生及最终回归

199 项 Rust、4 项 Node、fmt、严格 clippy、前端及调试构建通过，最终日志为 reports/p2-supervised-resume-checks-20261009.log、p2-supervised-resume-clippy-20261009.log、p2-supervised-resume-debug-20261009.log。新增恢复许可检查包括 17 类请求门禁；实际暂停/取消后尝试恢复，旧未决建议不重新获权。

原生 ACK 为 advice-delivery-native-hFdCUu、丢失回执 epij5Z、应用后暂停 Jo7m4n、取消 og0nsu；恢复修正后的暂停 h2opGl、取消 cEW4Ya。均完整重启核心后保留正确状态，没有重复派发。监督发送批准为 supervision-policy-native-QzmTUK；旧模式 advice-request-native-lH2Ixc 仍只保存不发送。目录均位于 reports，各自 native-result.json 或 result.json 记录实际检查及自有进程清理。截图已检查。

错误或未决执行阻止正常退出时，测试只终止自己创建的核心；清理记录中的 forcedOwnedProcess 不代表业务未知已解决。正式发布构建/启动的最终记录为 reports/p2-supervised-release-final-build-20261009.log 与 reports/p2-supervised-release-20261009/release-launch.json。旧的 p2-supervised-release-build-20261009.log 对应恢复许可修正前版本。
