# 建议事务的实际进程中断验证

2026-10-09，14:02 +08:00。生产 RuntimeHost 与唯一 worker 的五个边界已通过真实宿主进程终止、重开、只读对账、用户处置与明确恢复。管理决定和协议服务器为离线替身，真实模型调用0；不代表真实供应商故障矩阵已通过。

证据：`reports/advice-owner-crash-18692f5a/crash-result.json`，每个子目录保留原始账本、进程标记、协议调用日志和最终快照。最初四点批次 `advice-owner-crash-e23bcb3e` 也通过；最终五点批次为本页依据。

| 终止点 | 终止前 turn/start / steer 次数 | 重开状态与处置 | 明确恢复后累计 start / steer |
| --- | --- | --- | --- |
| prepared：应用准备已持久化，Advise未写 | 1 / 0 | Applying保留；原执行对账后，用户关闭未提交应用 | 2 / 1 |
| advised：Advise已写，宿主尚未接收提交 | 1 / 0 | 补原应用回执，送达未知；用户结束建议跟踪 | 2 / 1 |
| intent：发送意图已持久化，尚未发送 | 1 / 0 | 保留未知，不因本地意图推断已接收；用户结束跟踪 | 2 / 1 |
| received：远端ACK已到，尚未持久化 | 1 / 1 | 本地仍未知，不能从内存丢失的ACK恢复为已接收；用户结束跟踪 | 2 / 2 |
| acknowledged：ACK已持久化，应用回执尚未补齐 | 1 / 1 | 原ACK与应用事件交叉核验后恢复回执；不提供未知处置 | 2 / 2 |

后一次 start/steer 都属于后继任务B，旧建议未重发。五种情况均有独立进程和目录：父测试持有宿主 Child，等边界文件同步发布后调用实际进程终止并等待退出；持有当时协议进程句柄，确认其收到EOF后退出。16–19个已有不可变文件逐字节保持，重开未发送协议请求。

宿主重开后任务Unknown，持续运行进入Attention；用户暂停后才启动原会话只读对账。对账新增thread/read，不新增start或steer。用户处置后再次重开仍保持暂停；只有明确恢复后才完成两任务、每任务两项独立检查及合成主指挥收口。最终再重开，工作流和发送计数保持。

`supervised_worker.rs` 的屏障只在 cfg(test) 编译；发布配置不存在这些环境变量触发点。`advice_crash_tests.rs` 使用实际业务函数，不通过删文件或复制账本前缀伪装进程中断。标记本身也先同步临时文件再发布，避免测试读到半帧。`codex_dag_fixture.rs` 只为自有协议进程增加PID证据。

复现需先构建 `cargo build -p cerulean-core --example codex_dag_fixture --locked`，创建 reports 下全新目录并设置 CERULEAN_ADVICE_CRASH_DIR、CERULEAN_DAG_EXE，再运行 `cargo test -p cerulean-core --lib host::advice_crash_tests::five_advice_boundaries_survive_real_owner_termination_without_resend --locked -- --ignored --exact --nocapture`。只对测试自有进程执行终止。

本批核心88项常规测试、严格clippy和发布配置编译检查通过。UI未改动，原生用户处置与发布启动沿用前一批证据；不宣称本批重跑了安装或原生验收。先前验收详情的间歇入账断言失败仍未确定根因，继续保留。

剩余边界：文件发布中的磁盘/中断矩阵、真实远端历史能否唯一确认建议接收、真实主副指挥与用户编程流程。完整P0–P6仍未验收；接下来并行推进可独立完成的OMP产品接入工作，不将需要真实凭证的验收标成完成。
