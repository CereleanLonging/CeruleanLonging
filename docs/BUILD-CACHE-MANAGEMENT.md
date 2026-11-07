# 工作台源码缓存管理 / Source build cache management

## 简体中文

本机制管理CeruleanLonging源码工程的编译与构建缓存。项目中的用户任务目录继续由各自的执行边界管理。

### 目录与生命周期

- 常规 `npm run build`、`npm run dev`、`npm run contracts`、桌面构建及 `scripts/check.ps1`、`scripts/lint.ps1` 使用受管作业。
- 每个作业的Rust、Vite及临时文件落在源码工程 `.temp/build-cache/jobs/<作业标识>`。
- 作业结束后清理缓存文件，保留全部空目录。构建失败也进入收尾。强制终止或系统中断后留下的缓存可在维护页重新扫描并清理。
- 构建主任务结束后，超时未退出的所属子进程由本次任务进程组收尾，确认退出后再删除缓存，并保留主任务退出结果。
- 直接运行Cargo时，默认编译目录为 `.temp/build-cache/manual/cargo`，可在维护页清理。需要任务结束自动收尾时，使用上述受管入口。
- 最终程序保存在 `target/debug`、`target/release`，打包流程将安装器和配套载荷复制到这些输出目录后再清理临时编译文件。
- `scripts/check.ps1` 先构建集成测试使用的独立验收程序，再串行执行 Rust 测试。进程类用例包含真实期限和进程树检查，避免并发构建或大量并行文件摘要计算占用这些期限；测试断言与产品期限不因此放宽。

### 构建资源调度（2026-11-05）

Windows 本地受管构建默认采用交互优先调度。Cargo 同时只编译一个任务；构建进程及其子进程最多使用两个逻辑核心、BelowNormal 优先级和 20% CPU 硬上限。Windows Job 对进程组提交内存设置最多 3 GiB 的硬上限，限制在启动子程序之前建立并读回核对。工作区互斥锁拒绝第二个并发构建。

根据用户最新要求，编译前先核对该构建类型的最低已验证额度和当前内存。完整 Rust 构建（desktop、check、lint、contracts 及缓存工具自举）采用 3 GiB 进程组限额，保留 1 GiB 系统余量，启动需至少 4 GiB 可用物理内存和提交空间。此前完整发行构建在约 2.17 GiB 限额下失败、3 GiB 下成功，因此 3 GiB 是目前最低成功额度，不代表所有构建的绝对最低值。前端单独采用 1 GiB 限额，需至少 2 GiB 可用空间；2026-11-05 实际生产构建通过，峰值约 395 MiB。空闲增加不会自动加大额度。

用户最新明确要求“直接进编译，其他的我都有用，下次这种差距在1g以内的不用报了”：以上 4 GiB/2 GiB 是启动参考值；差额不超过 1 GiB 时自动准入，只落本地回执，不再次询问。因此完整 Rust 可在至少 3 GiB、前端可在至少 1 GiB 可用物理及提交空间时启动，保持原构建额度；运行中的 1 GiB 保护线仍保留。超过授权差额才暂缓，不结束用户其他应用。

运行中若物理内存或提交空间余量低于 1 GiB，停止本次构建进程组；不结束用户其他应用，不自动重试或提高上限。每次回执记录 Windows Job 的提交内存峰值（含已退出子进程）和观测到的最低系统余量；成功结果汇总到 `docs/build-cache/build-memory-calibration.json`，供后续变更前校准。单次峰值不直接用作下次硬上限，额度也不会在失败后自动提升。主任务成功且生成有效峰值才更新成功记录。2026-11-05 完整发行构建已在启动容差下成功，整组峰值 2,769,813,504 字节（约 2.58 GiB），运行中最低可用物理内存约 1.76 GiB；继续保留 3 GiB 已验证额度，不将单次峰值当作绝对最低。

入口为 `scripts/build-scheduler.mjs`，Windows 约束由 `scripts/build-resource-guard.ps1` 和 `scripts/build-resource-job.cs` 实施。需要复用已有受管缓存执行 Rust 命令时，使用 `node scripts/build-scheduler.mjs --rust <程序> <参数...>`。只设置 `CARGO_BUILD_JOBS=1` 不能约束单个 rustc 的内部线程和内存；直接 Cargo 命令只有仓库内的单任务默认值，应优先使用受管入口。

资源不足或已有构建时返回 75，并在 `docs/build-cache/scheduling-logs` 保存回执。调度器不会在后台无限等待或重试；普通界面测试不受此构建策略限速。Windows 以外仍执行 Cargo 单任务，不声称具备 Windows Job 资源限制。

### 缓存维护界面

“维护与升级”底部的“累计缓存监控”显示当前缓存、可清理体积、累计清理量和扫描次数。源码目录可手动指定。面板打开时及每30秒读取当前状态，点击“扫描全项目临时缓存”会写入体积记录。

“缓存区域”只列出实际占用大于0字节的目录。清理后对应条目自动移出列表，目录产生新缓存后在下次扫描时重新显示。全部清空时显示空状态提示，累计统计和操作回执可单独查看。

“清理剩余缓存”处理受管临时目录及已识别的旧Rust缓存子目录。使用中的作业通过跨进程文件锁保护。扫描到链接、占用文件或不完整清理记录时，界面显示对应问题，累计清理量按实际完成回执统计。

### 提炼日志

日志统一写入源码工程 `docs/build-cache/cleanup-logs`。每次记录操作时间、扫描体积、实际清理字节数和文件数、跳过的活跃作业及待处理问题。清理意图在删除之前落盘，完成回执在结束后单独保存，缺失完成回执的操作会在界面提示核对。

## English

This feature manages compiler and build caches belonging to the CeruleanLonging source checkout.

Windows managed builds use one Cargo job, at most two logical processors, BelowNormal priority and a 20% hard CPU limit. A workspace mutex prevents concurrent builds. Full Rust builds use a 3 GiB committed-memory cap and require 4 GiB available physical/commit memory, including a 1 GiB reserve. This is the lowest successful full-release budget observed so far; an earlier 2.17 GiB attempt failed. Frontend builds use a separately validated 1 GiB cap and require 2 GiB available memory. The latest user instruction accepts startup shortfalls of up to 1 GiB without another prompt. Accordingly, full Rust/frontend admission minima are 3/1 GiB available physical and commit memory, respectively; the 4/2 GiB values are references. The runtime 1 GiB reserve guard remains active. Small startup shortfalls are recorded locally. Extra free RAM does not raise these limits. A production frontend build passed with an approximately 395 MiB Job peak on 2026-11-05. Kernel Job peaks and observed reserve minima are saved in receipts; successful measurements are summarized in `docs/build-cache/build-memory-calibration.json`. Neither an individual peak nor a successful cap proves an absolute minimum. Insufficient resources defer or stop only this build, with exit code 75; there is no automatic retry or limit increase. Raw Cargo receives only the single-job default. Non-Windows runs do not claim Windows Job enforcement.

- Standard frontend, desktop, contract-export, check and lint entry points allocate a managed job under `.temp/build-cache/jobs/<job-id>`.
- Rust, Vite and temporary outputs share that job directory. Cleanup runs after success or failure and preserves empty directories. Interrupted jobs can be reviewed and cleaned from Maintenance.
- After the main task exits, its private process group reaps lingering descendants after a grace period. Cache deletion waits for their exit, preserving the main task result.
- Direct Cargo commands use `.temp/build-cache/manual/cargo`. Use the managed entry points for automatic end-of-task cleanup.
- Final program files and installer outputs are staged outside disposable compiler-cache subdirectories before cleanup.
- `scripts/check.ps1` builds the independent verifier before integration tests, then runs Rust tests serially. Native fixtures enforce real deadlines and process-tree checks; serialization avoids competing hashing/launch work without relaxing assertions or product limits.
- The bottom Maintenance panel shows current, reclaimable and cumulatively reclaimed bytes, scan counts, active jobs and recent operations. Explicit scans save measurements.
- Cache-region rows contain only directories with a positive byte count. Cleared entries disappear and return on a later scan when files occupy space again. Empty-state text accompanies the cumulative metrics and operation history.
- Cross-process leases protect active jobs. Links, locked files and incomplete receipts produce visible diagnostics.
- Distilled Markdown logs live under `docs/build-cache/cleanup-logs`, with a persisted intent before deletion and a separate completion receipt afterward.
