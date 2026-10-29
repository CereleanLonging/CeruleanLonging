# 已应用建议的用户处置

2026-10-09，09:07 +08:00。本批实现“结束建议跟踪”，用于 Advise 已入账但没有可确认接收回执的情况。完整 P0–P6 尚未通过。

## 行为与边界

用户在指挥记录的完整详情中审阅原建议、任务和尝试，填写原因并确认。系统追加独立 `advice-closures` 记录，保留管理请求、应用事件和送达文件。`delivered` 仍为 false，`deliveryState` 仍为 unknown，详情另行显示用户已结束跟踪。

此操作不调用供应商、不重发、不改变工作流或执行结果、不退回请求预算，也不自动恢复项目。项目暂停且本地执行、验收和指挥请求全部结束后才能处置。已有正确 ACK 的建议不提供此入口；只有应用意图但没有 Advise 的情况仍使用原有未提交应用处置。

用户明确恢复后，已提交任务进入独立验收和主指挥收口；原 job Unknown 仍阻止调度，必须先完成执行结果对账。结束建议跟踪不能绕过这个门槛。

## 实现

- `advice_closure.rs` 保存有版本、user actor、时间、原因、审阅摘要与证据关联的不可覆盖记录。先 create_new 临时文件并同步，再发布；失败使写入器失效，残留临时文件阻止重开时静默忽略。
- `advice_recovery.rs` 核验原管理摘要、原 Advise 事件、任务/尝试、送达文件或其缺失，以及工作流历史锚点。审阅摘要另绑定运行账本链头、执行/验收记录和管理请求，防止暂停→恢复→暂停后使用旧确认。
- `CloseAdviceTracking` 只在可信用户 IPC 中处理，不进入模型指令集合。提交前重新生成候选；确认失效时不写入。
- 原始记录完整且处置合法时，只解除这条建议的调度阻塞。后续执行、取消和验收门槛保持生效。重启校验历史锚点，允许其后发生合法工作流进展。
- `AdviceTracking.tsx` 提供按需重新读取、项目/请求范围、原建议、原因、返回和确认；处置后保留项目筛选并刷新详情。

## 已取得的证据

常规检查日志 `reports/p2-advice-closure-checks-20261009.log`：201 Rust、4 Node、前端通过。严格 clippy：`p2-advice-closure-clippy-20261009.log`。发布构建及 `p2-advice-closure-release-20261009` 只读启动通过，新增计划 0、页面错误 0，正常关窗后核心仍可读。

| 证据目录（reports 下） | 结果 |
| --- | --- |
| advice-closure-ack-74d7406b | 正确接收的建议拒绝未知跟踪处置 |
| advice-closure-lost_ack-253deaa3 | 未知回执可处置；重启不自动调度；明确恢复仍因执行 Unknown 停止 |
| advice-closure-pause_commit-338741c6 | 无送达意图的已应用建议可处置；明确恢复完成 A→B、每任务两项验收与合成主指挥收口；旧建议未变成 ACK |
| 同上 tamper.log | 六类重写哈希的跨账本关联伪造被拒绝 |
| advice-delivery-native-rCXvKe | 原生无送达意图路径：返回不写、确认保存、完整核心重启后状态保持 |
| advice-delivery-native-T57fwy | 原生未知 ACK 路径：同上 |
| advice-delivery-native-bj4Vur | 最终界面：审阅后暂停改变运行链头，旧确认被拒绝；刷新后确认成功，完整重启保持证据 |

两组新增常规账本测试覆盖不可覆盖、写入中断、actor/文件名/内容异常。原生确认、拒绝和保存截图已人工查看，无页面错误。全部供应商/管理响应为明确离线替身，真实模型调用 0。未决测试只清理自有核心进程；其业务状态仍保持未决。

保留失败证据：`advice-closure-pause_commit-cce02a6f` 的恢复夹具未为第二任务启用等待 steer 标记，导致替身拒绝请求，修正后新目录完整通过；首次原生运行被核心/runtime 不一致检查拦住，第二次暴露测试辅助函数缺失，均修正后重跑。失败日志不作为通过依据。

## 尚未完成

尚未实现依据真实远端历史确认建议接收，也未完成应用/Advise/送达意图/ACK 四个断点的实际进程终止矩阵。现有写入故障注入和正常核心重启不能替代该矩阵。真实 OpenAI 管理配置、真实 Codex 建议效果、完整编程与 diff、首批 OMP/API/CLI、Qoder/WorkBuddy 既有会话、并行隔离、效率对照及 P6 安装升级继续保留。

## 回归中的待查问题

`p2-advice-closure-final-checks-20261009.log` 在既有 `verification_details_bind_real_stages_and_reject_corrupt_evidence` 测试出现一次 ledger_recorded=false。首次失败没有记录具体 mode/执行错误，不能确定根因。随后单项、完整 strict_workflow 组（28 项）及四个隔离进程并发复核均通过；这些通过不构成已修复证据。现已增强失败诊断并保留失败临时目录，待复现后核验原 result.json、job、阶段输出与工作流。完整 P2/P6 验收继续受限，禁止将这一失败删去或写成已解决。

13:50 收尾：最终全套复核 p2-advice-closure-recheck-20261009.log 通过（201 Rust、4 Node、前端和核心构建），最终 clippy 通过。上一节间歇失败仍保持未解决；后续通过不替代根因诊断。证据索引 p2-advice-closure-review-20261009.json。
