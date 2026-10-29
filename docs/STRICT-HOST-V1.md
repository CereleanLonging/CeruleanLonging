# 完整执行方案与常驻宿主 v1

更新：2026-10-07 23:24 +08:00。P1 子模块通过，P1 整体未放行。没有真实模型调用。

## 已实现

- RuntimeHost 持有基础 Store 和各项目 DurableWorkflow；服务启动时验证项目配置并重放账本。项目目录必须存在，拒绝链接路径；同一规范目录不能重复登记。
- 用户 IPC 新增 registerProject、approveRevision，始终使用宿主绑定的 user 身份。没有暴露任意 WorkflowCommand、principal、模型方案注入或自报验收通过入口。
- 内部 chief 回调另传宿主选定的 projectId，模型方案不能自行改投其他项目。此方法尚未连接真实 OpenAI/Codex 通道。
- 完整 PlanRevision、Approval、Attempt、Receipt/Manifest、Verification 随对应事件持久化。批准摘要覆盖目录、任务、策略、适配器、产物和验收命令。基础 Plan 批准不能继承为执行批准。
- 严格模式按固定角色校验命令；分配绑定版本、尝试、fence、会话与工作区。缺少结构化回执不得对账；已知启动的任务不能回退成未启动。unknown 不自动重发。
- verify/accept 的实际路径已调用产物核验门槛。接受前重新读取当前文件；同大小改动也拒绝。历史重放只检查历史证据关联，不要求历史产物永远留在原目录。
- 尚无 worktree 隔离时，同项目任务共用同一资源键，避免伪造 workspaceKey 并行写同目录。
- 未完成任务存在时 shutdown 返回 unresolvedTasks，核心继续运行。Pending/Accepted/Cancelled 不阻止停止；其他状态阻止。窗口关闭仍不停止服务。
- 桌面项目设置可关联目录，方案页可审阅完整执行授权。弹窗冻结当时方案，服务器出现新版本时旧摘要批准被拒绝。

## 证据

| 检查 | 结果与位置 |
| --- | --- |
| 全量基础检查 | reports/p1-host-checks-20261007.log：Rust 32 项、Node 4 项通过，fmt/类型生成/前端构建通过 |
| 回调项目范围修正后复核 | reports/p1-host-final-tests-20261007.log：严格工作流 5 项通过；含错误项目拒绝、无事件副作用 |
| 所有 target 严格 lint | reports/p1-host-clippy-20261007.log：通过 |
| 发布及原生调试构建 | reports/p1-host-build-20261007.log、p1-host-debug-build-20261007.log：通过 |
| 原生桌面完整批准 | reports/desktop-smoke-P3KGTq/result.json：基础 9 项，以及完整方案审阅、摘要绑定批准、项目登记、重开恢复通过 |
| 实際发布启动 | reports/p1-host-release-20261007/release-launch.json：启动/核心连接/关闭/关闭后读取通过，日常数据 0 计划 |

Rust 两个 ignored 是显式子进程辅助入口；崩溃 helper 由集成测试调用，native fixture 由 `node tests/desktop-smoke.mjs --workflow` 显式调用。后者只允许在本工程 reports 下的新隔离目录生成测试方案，不存在于产品 CLI。

原生截图 workflow-review.png 已目视检查：目录、权限、预算、任务、命令和确认按钮可读；较长内容在弹窗内滚动。方案为合成 chief 夹具，点击与持久化是真实 WebView2/Rust 服务操作；不能计作 P2 真实模型验收。

## 尚未达到的边界

23:42 更新：独立 runner 已实现，run_verification 实际运行批准命令后将真实报告写入 Verify；旧协议测试仍保留合成报告夹具。真实业务模型与异步服务调度尚未连接。详见 [运行器](VERIFICATION-RUNNER-V1.md)。回执摘要不自动证明供应商真实性。预算/工具权限参与批准，但运行时扣账及 OS 沙箱尚未落实。同用户进程可连接用户管道，不能将 SID 校验解释为同账户 Agent 隔离。

历史 accepted/verified 是历史记录；不会监控并撤销后来发生的外部文件修改。同 operationId 的缓存返回只是原操作结果，不是再次认证。当前重新 accept 会核验文件，但检查后仍存在并发写入窗口，需不可变受测快照与独占租约解决。

政策版本当前固定 1、时区固定 Asia/Shanghai；配置变更迁移、事件游标、SQLite 可重建索引、大账本、备份及故障矩阵仍待完成。禁止停止未完成任务只是退出门槛，不是完整取消/对账/托盘退出流程。

## 下一步与验收条件

1. 可信独立 runner：从已批准计划取得精确命令，固定程序/脚本来源，限定目录、环境、超时和进程树；真实记录退出码与有界输出。覆盖成功、失败、超时、子进程残留和产物变更。
2. 将运行前后不可变产物、命令日志及报告连到同一尝试。模型不能提交 passed=true 替代执行；报告丢失或结果未知不得收口。
3. 真实 chief/deputy/worker 受控通道与调度回调；在一个真实项目上完成用户方案确认、Codex 修改、diff、独立检查、恢复和 chief 收口，再评 P2。
4. 完成 P1 剩余存储、退出与规模检查；P3–P6 按阶段矩阵保留全部首批对象，不因本轮 GUI 通过提前放行。

