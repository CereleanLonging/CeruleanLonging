# 执行、批准与证据契约 v1

2026-10-07 22:19 +08:00。当前为 P0/P1 的可运行契约与文件核验模块，未完成认证服务和产品执行闭环。定义位于 `crates/domain/src/execution.rs`；Rust 自动生成前端类型。

| 契约 | 绑定内容 | 当前核验 |
| --- | --- | --- |
| Project / PlanRevision | 项目、完整 WorkflowPlan、策略及逐任务执行规格 | schema、任务对应、路径、必需验收项；当前仅 Asia/Shanghai |
| ExecutionPolicy | 版本、工具范围、请求数/时长上限、可选美元分预算 | 进入批准摘要；预算整数避免浮点差异，尚无运行时扣账 |
| TaskExecution / AcceptanceCheck | 适配器、产物路径、验收程序/参数/超时 | 不允许缺少产物或独立检查；程序与参数整体参与批准 |
| Approval | planId/revision/policyVersion/revisionDigest、用户、批准时间 | 目录、预算、工具、执行者、检查、产物或版本改变均使旧批准失效 |
| Attempt / SessionBinding | worker、task、attempt、fence、计划摘要、适配器版本/会话/工作区/身份强度 | 回执不可跨尝试、会话或版本使用；UI 观察与供应商 ID 分开 |
| ExecutionReceipt | 上述执行关联、turn、终态、观测时间、证据摘要 | 完成态方可生成待验收产物清单；结构校验不等于证明上游真实性 |
| ArtifactManifest | 计划、task/attempt、receipt 摘要、文件路径/大小/SHA-256 | 核心独立读文件；模型自报哈希不作为实际文件哈希 |
| VerificationReport | 清单摘要、独立验收器、每条批准命令摘要/退出码/超时/输出摘要 | 检查齐全、无重复、退出 0、无超时；收口前重新核对文件 |
| AdapterCapability | 10 种能力，各自 supported/unsupported/unverified、版本及证据摘要 | 产品 10 类接入全部保留，每项当前 unverified；确定性声明需版本和证据 |

## 核心文件观察

`ArtifactObserver` 在批准及完成回执校验后，按批准的产物清单读取实际文件。要求绝对项目根目录，逐层拒绝符号链接/Windows reparse point；拒绝路径穿越、ADS、设备名、重复大小写路径等。单文件上限 64 MiB，整组 256 MiB，超过上限报错，不静默截断。生成时核对文件大小和修改信息；复核重新读取内容，同大小改动也会改变哈希。

`require_verified` 先检查来自可信验收器的报告，再重新观察文件并要求 manifest 完全一致。23:24 已将该门槛接入严格 scheduler 的 verify/accept，并接到常驻宿主；用户 IPC 不暴露自报通过。23:42 已增加独立 runner 的实际进程报告和入账测试；真实模型及不可变快照未接，详见 [严格宿主](STRICT-HOST-V1.md)。

这些 DTO 不是认证令牌。authenticated_user、trusted_verifier、expected_turn 必须由受控通道提供。持有合法格式 JSON 或相同 userId 不构成批准。回执 evidenceSha256 当前只校验格式，供应商回执采集/证据文件核对仍由待实现的适配器与可信宿主完成。路径检查和哈希也不替代 OS 沙箱；核对结束后的并发写入仍需任务租约与不可变集成快照约束。

23:42：验收命令已可由宿主 runner 实际运行，固定 exe/指定可信源、限制进程树与时间、保存实际退出码/有界日志，检查前后产物；详见 VERIFICATION-RUNNER-V1.md。仍须完整不可变快照及可信验证策略，不能让 worker 自己编造 VerificationReport。

## 实证与剩余工作

`crates/core/tests/execution_evidence.rs` 七项：批准变更失效；危险路径与无效格式；回执关联；真实文件 SHA-256/同大小变更/删除；真实 Windows junction 拒绝；验收项缺失/失败/篡改与验后变更；能力声明的证据要求。报告输入属于合成协议夹具，不是一次真实独立编程验收。

本轮同时修复打包碰撞：Tauri 的 externalBin 原与 Cargo 产物均叫 cerulean-core，桌面构建脚本可能覆盖 target/debug 的新核心。打包名改为 **cerulean-runtime.exe**，启动器同步更新。Cargo 原生输出仍叫 cerulean-core.exe。发布目录必须同时携带 CeruleanLonging.exe 与 cerulean-runtime.exe。clippy 前后 Cargo 核心哈希相同，随后 IPC 能读取最新 10×10 能力字段；CI 已加入该检查，云端尚未执行。

更新：本机 IPC、常驻生命周期、契约持久化、UI 完整批准与严格收口已通过本机检查。后续为独立 runner、不可变受测快照和真实受控模型通道，再进入 P2 编程闭环。继续保留原 WorkflowPlan 摘要兼容；新的 PlanRevision 摘要不能复用基础 Plan 或旧工作流批准。


