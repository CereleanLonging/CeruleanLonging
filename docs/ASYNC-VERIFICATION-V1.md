# 异步验收宿主 v1

2026-10-08。生产实现：`host.rs`、`async_verification.rs`、`ipc.rs`。真实模型仍未接入，P1 整体未通过。

宿主先同步保存 VerificationJob，再把该项目 DurableWorkflow 的所有权交给专用线程。该线程持有项目账本锁，负责受测副本、进程树、日志与 Verify 提交；主宿主保留只读项目快照及取消信号。查询不会等待检查进程，其他项目可继续操作。同一项目不能并发验收，重复操作 ID 拒绝；最多同时 4 个项目，每项目最多 1000 条验收记录，达到上限拒绝而不丢历史。

命名管道仍只接受固定用户命令。新增 CancelVerification 仅包含 projectId/operationId，不接受程序、参数、角色或 passed。真实开始方法 start_verification 只供可信宿主调用，工具绑定仍需后续产品登记；用户 IPC 尚无任意运行命令入口。

服务每 100ms 非阻塞接收线程结果，更新状态并收回项目账本；前端有进行中任务时每秒查询，取消或完成后更新状态。请求取消先持久化 Cancelling，再发原子信号；实际进程树清理后记录 Failed，不把取消当成功。UI 可以继续操作，停止按钮已在原生窗口实测。

状态为 Running/Cancelling/Succeeded/Failed/Unknown。状态文件位于 verification-jobs/<project>/<operation>.json；它是视图，不替代 Verify 账本证据。重启时，即使保存的状态尚为 Running，只要存在匹配的 Verify 就恢复真实结果；缺少可确认结果则 Unknown，禁止换 ID 自动重跑。失败状态若对应的原始结果仍残缺，也回到 Unknown。文件读取有 64KiB 状态和 1MiB 结果限制，拒绝链接路径。

线程异常、通道断开、账本提交不确定或状态保存失败均不报成功；需要重开的账本在线程内重开，失败时保留只读项目和 Unknown。服务不允许在进行中/待对账任务存在时停止。00:50 更新：显式放弃 UI 已实现，绑定当前证据、保留原记录且不令任务通过，详见 VERIFICATION-RECONCILIATION-V1.md；不能通过删除记录恢复执行。

本轮同时按规范路径组件、忽略 Windows 大小写检查目录包含关系。登记重叠项目目录会拒绝，包括另一个项目正在验收时，避免绕过同目录串行限制。worktree 的合法隔离与共享资源声明仍待 P5 实现。

## 实证

- `reports/p1-async-checks-20261008.log`：Rust 46 项、Node 4 项通过；fmt/生成 TS/前端构建通过。
- 最终严格工作流 9 项：`p1-async-recovery-final-tests-20261008.log`；最终 clippy：`p1-async-recovery-final-clippy-20261008.log`。
- 新增实际慢检查证明 10 次宿主查询在 1 秒内完成；真实命名管道查询也在 1 秒内返回，并能取消。检查进程仍在运行时拒绝重复/其他 ID 启动、错误 ID 取消、重叠目录登记及核心停止。
- 测试覆盖取消持久化、完成入账而视图尚未更新的重启恢复、没有入账结果的 Unknown、失败标签但结果 JSON 截断的 Unknown；未知结果不会自动重跑。
- `tests/native-verification.mjs` 启动隔离的真实 Rust 服务与原生 Tauri/WebView2，点击停止验收，自动等待终态，读取磁盘状态并从 CLI 确认 task.verified=false。没有向模型发送消息。

## 下一步和边界

完善显式对账/放弃流程、可信工具/脚本/依赖环境登记及其批准绑定，再把真实主副指挥与 worker 回调接入。用户不能自行填 passed=true。

线程隔离解决长验收阻塞管道全局锁的问题；初始化、日常账本落盘和状态文件更新仍有同步磁盘操作，不能宣称慢/坏磁盘也有 1 秒延迟保证。受测文件复制目前容量有界，阻塞文件 I/O 的强制取消未实现。OS 文件/网络沙箱、动态依赖及绝对参数访问范围、集成租约仍须后续完成。

本批合成任务使用真实文件和进程；不算真实 P2 模型闭环。P3 API/CLI/OMP、P4 Qoder/WorkBuddy 桌面、P5 多任务效率和 P6 发布全部原范围保留。

