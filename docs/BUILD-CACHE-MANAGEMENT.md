# 工作台源码缓存管理 / Source build cache management

## 简体中文

本机制管理CeruleanLonging源码工程的编译与构建缓存。项目中的用户任务目录继续由各自的执行边界管理。

### 目录与生命周期

- 常规 `npm run build`、`npm run dev`、`npm run contracts`、桌面构建及 `scripts/check.ps1`、`scripts/lint.ps1` 使用受管作业。
- 每个作业的Rust、Vite及临时文件落在源码工程 `.temp/build-cache/jobs/<作业标识>`。
- 作业结束后清理缓存文件，保留全部空目录。构建失败也进入收尾。强制终止或系统中断后留下的缓存可在维护页重新扫描并清理。
- 直接运行Cargo时，默认编译目录为 `.temp/build-cache/manual/cargo`，可在维护页清理。需要任务结束自动收尾时，使用上述受管入口。
- 最终程序保存在 `target/debug`、`target/release`，打包流程将安装器和配套载荷复制到这些输出目录后再清理临时编译文件。

### 维护界面

“维护与升级”底部的“累计缓存监控”显示当前缓存、可清理体积、累计清理量和扫描次数。源码目录可手动指定。面板打开时及每30秒读取当前状态，点击“扫描全项目临时缓存”会写入体积记录。

“清理剩余缓存”处理受管临时目录及已识别的旧Rust缓存子目录。使用中的作业通过跨进程文件锁保护。扫描到链接、占用文件或不完整清理记录时，界面显示对应问题，累计清理量按实际完成回执统计。

### 提炼日志

日志统一写入源码工程 `docs/build-cache/cleanup-logs`。每次记录操作时间、扫描体积、实际清理字节数和文件数、跳过的活跃作业及待处理问题。清理意图在删除之前落盘，完成回执在结束后单独保存，缺失完成回执的操作会在界面提示核对。

## English

This feature manages compiler and build caches belonging to the CeruleanLonging source checkout.

- Standard frontend, desktop, contract-export, check and lint entry points allocate a managed job under `.temp/build-cache/jobs/<job-id>`.
- Rust, Vite and temporary outputs share that job directory. Cleanup runs after success or failure and preserves empty directories. Interrupted jobs can be reviewed and cleaned from Maintenance.
- Direct Cargo commands use `.temp/build-cache/manual/cargo`. Use the managed entry points for automatic end-of-task cleanup.
- Final program files and installer outputs are staged outside disposable compiler-cache subdirectories before cleanup.
- The bottom Maintenance panel shows current, reclaimable and cumulatively reclaimed bytes, scan counts, active jobs and recent operations. Explicit scans save measurements.
- Cross-process leases protect active jobs. Links, locked files and incomplete receipts produce visible diagnostics.
- Distilled Markdown logs live under `docs/build-cache/cleanup-logs`, with a persisted intent before deletion and a separate completion receipt afterward.
