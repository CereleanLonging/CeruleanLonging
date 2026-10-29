# 独立验收运行器 v1

2026-10-07 23:42 +08:00。实现位于 `crates/core/src/verifier.rs`、`verifier/process.rs`；调度入口为 `DurableWorkflow::run_verification`。Windows 原生实现，无模型调用。P1 子模块通过，P1/P2 整体未通过。

## 实际执行链

1. 宿主从严格账本取得已批准版本、当前 submitted 尝试、完成回执与产物清单；尚未提交、已验收、旧尝试或重复操作 ID 均不得启动。
2. 从可信工具登记取得 PinnedCheck：精确命令摘要、绝对 exe 路径及 SHA-256、可信验收脚本/配置的路径与哈希。验证源码必须在 worker 项目外，执行期间保持禁止写入/删除的文件句柄。脚本与程序的具体来源由可信宿主选定，不能接受 worker 自报来源。
3. 重读产物；在项目外的 `verification/<operationId>/` 独占创建操作目录并同步 intent.json。已有未完成意图或损坏结果时禁止换 ID 重跑，须后续对账入口处理。
4. 指定绝对 exe 创建原生进程；参数逐个转义，无隐式 shell、PATH 查找或批处理解释。环境只传 SystemRoot/WINDIR 与独立 TEMP/TMP，不继承 API key、PATH 或运行时注入变量；stdin 为 EOF。实际构建工具需要的额外环境尚须可信配置与批准。
5. CreateProcessW 的 JOB_LIST 属性在创建时关联 kill-on-close Job；HANDLE_LIST 仅允许 stdin/stdout/stderr。无先运行后加入 Job 的窗口。超时、取消和输出超限终止整棵树；核心异常退出时 Job 句柄关闭也会清理。
6. stdout/stderr 分开读取，各最多保留 1 MiB，超限拒绝报告。监控每项和整组时间；父进程正常退出后允许最多 100ms 的系统计数排空，再检查残留子进程。残留会被杀掉并拒绝报告；清理最多确认 5 秒，未确认不报通过。
7. 实际输出、退出码、超时、命令摘要写入文件；检查前后重新核对产物。有效报告由核心写入 Verify 事件，再由 chief 接受时重读文件。非零退出/超时写失败报告；取消、超限、产物变化等写诊断结果，不生成可通过报告。

Job 与创建属性依据微软 [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) 和 [UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)。这些机制管理生命周期，不构成文件/网络权限沙箱。

## 实证

- 原有 32 项 Rust 基线 + 7 项运行器集成 + 1 项工作流真实报告集成，共 **40 项通过**。Node 两种 IPC 共 4 项通过。
- `verification_runner.rs` 使用本机编译的测试辅助进程，实际观察输出 SHA、退出 7、1 秒超时、启动后取消、日志洪泛、同大小产物改动、外部验收源改动拒绝、遗留子进程清理。
- 强杀 runner 宿主时，父测试持有已经启动的子进程句柄，观察其进入退出状态；账本留下 intent 而无 result，换不同操作 ID 也被拒绝，未产生第二次文件副作用。
- `strict_workflow.rs` 实际执行独立检查、写入 Verify、重开账本、拒绝重复运行、chief 接受；辅助进程读取真实产物断言内容，并写一次计数。
- 全量日志 `reports/p1-runner-final-checks-20261007.log`；补不同 ID 恢复保护后定向 `p1-runner-recovery-final-tests-20261007.log`（严格 6 + runner 7），最终 clippy `p1-runner-recovery-clippy-20261007.log`。
- 发布构建 `reports/p1-runner-build-20261007.log` 与实际启动 `reports/p1-runner-release-20261007/release-launch.json` 通过；日常数据 0 计划，测试创建的常驻进程已清理。
- 共 6 个 ignored 辅助入口由对应父测试显式启动；不是跳过待实现业务测试。

首次测试发现：父进程句柄已 signalled 时 Job 活跃计数可能短暂未归零，误判成功进程为残留。加入有界排空后通过，故障日志 `p1-runner-tests-20261007.log` 保留。最初 lint 的测试 slice 复制已修复；故意制造孤儿的辅助函数局部注明 lint 例外。崩溃测试常量最初导入错误已修复；所有旧日志保留。

## 当前限制与下一步

这是宿主内部方法，尚未暴露任意命令到用户 IPC，也未接真实主副指挥。当前运行方法同步执行，正式接服务需专用工作线程、进度/取消通道和状态事件，不能阻塞管道服务全局锁。

通过固定 exe 与传入的可信文件能防止这些文件在执行期间被替换；不能自动保证其依赖库、项目测试脚本或其他动态载入文件可信。下一步须明确可信验收脚本来源，把工具绑定及依赖环境纳入可审阅策略。

23:59 更新：runner 已改到完整受测副本运行，已有输入文件持写保护句柄，报告绑定全树并在 live verify/accept 复核。详见 [受测副本](SOURCE-SNAPSHOT-V1.md)。独占集成租约、可信工具环境和 OS 权限隔离仍待完成，不能宣称消除了所有外部写入竞争。

普通同账户进程仍可访问磁盘和网络；清理进程树、移除继承凭证不等于 OS 沙箱。日志文件未宣称可自动识别并脱敏所有项目秘密。没有测恶意代码通过系统服务创建的进程、跨账号或真实物理断电。

操作意图和结果独立保存；中间退出后默认阻止再次执行，但还缺对账/放弃的产品入口。没有结果时不自动重新运行，也不能凭未完成日志直接标记成功。失败复核、完整快照、恢复操作 UI、可信模型回调、存储规模及 P3–P6 各阶段继续保留。

