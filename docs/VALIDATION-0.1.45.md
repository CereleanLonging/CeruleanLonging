# 0.1.45 验证范围 / Validation scope

本版属于开发候选。统一独立检查、检查归档、双权重门槛和桌面监督修复使用本地回归验证；真实供应商和桌面会话的完整业务链路单独列为待验证，不由合成结果代替。

## 已完成的针对性验证

| 范围 | 结果 | 边界 |
| --- | --- | --- |
| 生产 React 组件 | 90 个场景通过 | 合成项目与模拟服务；覆盖选模、配置持久化、CLI 会话、桌面 ID、盲评、缓存和按钮布局 |
| Windows 原生选模 | 4 个场景通过 | 真实 Tauri、IPC、DPAPI、设置存储和窗口重启；模型目录与凭证是合成测试数据 |
| 独立验收程序 | 7 个协议场景通过 | 真实独立子进程；合成报告与票据，验证归档摘要、双门槛、缺证据、工具拒票及篡改拒绝 |
| 归档发布 | 针对性回归通过 | 相同待写入内容恢复、不可覆盖、冲突拒绝、受保护读回 |
| 加权账本 | 针对性回归通过 | 收齐前密封、低分锁住功能测试、重开保留结果、拒绝篡改 |
| 核心库与命令参数 | 1,152 项通过，299 项忽略 | 1,086 项核心库测试及 66 项命令参数测试；另行执行最后的 reader21 能力声明回归 |
| 严格工作流集成 | 64 项通过，13 项忽略 | 在发行编译结束后串行运行原套件；包含复验、取消、崩溃恢复、只读查询与历史证据保留 |
| 检查器登记与运行 | 20 项通过，6 项忽略 | 真实进程退出码、期限、输出洪泛、孤立子进程、封存源码及工具替换拒绝 |
| 桌面宿主 | 30 项通过，2 项忽略 | 包含持久设置、窗口隔离、退出与维护窗口约束；忽略项未计为通过 |
| IPC 与常驻服务 | 6 项通过 | 真实核心进程；包括断连恢复、错误协议、重复服务和只读诊断 |
| 缓存生命周期 | 5 个跨进程场景通过 | 活跃任务保护、失败与中断收尾、空目录保留及所属子进程收尾 |
| reader21 兼容性 | 1 项核心回归、3 个实际程序检查通过 | 0.1.44 拒绝新版数据且字节不变，0.1.45 可以重开，能力与备份版本一致 |
| 最终发行构建 | GUI、核心与独立程序构建通过，8 项封包载荷摘要一致 | 本地开发候选；独立验收器的 7 个协议场景也在最终发行程序上通过 |
| 当前桌面入口与图标 | 新快捷方式、管理员启动标记读回一致；6 档图标均在程序中找到 | 保留本机后续修订的盾牌边框；另用实际被替换的图标版 0.1.44 核心重复通过 reader21 兼容检查 |
| 严格 Rust Clippy | 全工作区、全部目标通过 | 使用 `custom-protocol`，警告视为错误 |

按各 Rust 目标最后一次完整结果去重汇总：1,548 项通过、327 项忽略、0 项失败（包含上表桌面宿主）。最后的 reader21 单项复查另列，不重复加总。上述忽略项包含需专用账户、显式真实模型授权或父测试启动的子进程夹具，不计为单独通过。契约导出、TypeScript 类型检查及 Rust 格式检查均通过。

## 本轮发现及修复

1. 检查归档用硬链接发布后，在删除临时别名之前调用严格读回，导致真实检查结果被拒绝。改用不覆盖目标的原子移动，保留待写入恢复和冲突拒绝。
2. 版本改为 0.1.45 后，备份兼容清单遗漏当前版本，导致备份及依赖冷备份的迁移流程失败。补齐当前 schema-1 写入版本，保留未来未知版本拒绝。
3. 阅读器保护已增加到 reader21，但程序能力信息仍为 20。同步能力声明，并增加新旧程序的独立兼容性核对。
4. 原生界面测试试图改写 Tauri 的只读 `invoke`，未能拦截模拟目录。改在自定义协议请求层处理并先进行拦截探针；同时等待重启后的配置恢复，并通过界面按钮关闭选模框后正常退出。首次无效夹具结果不计为通过；它使用合成凭证，不能据此声称该次未尝试外部目录请求。
5. 干净环境的检查脚本原先在集成测试之后才构建独立工具。调整为先准备工具，再运行测试，避免依赖本地遗留程序。
6. 严格工作流首次运行与发行编译并行，出现 9 项失败，包含等待超时、`descendants-left-running` 及后续状态断言。保留该次 55 通过、9 失败、13 忽略的结果。构建结束后按原断言串行运行，64 通过、13 忽略，原失败项全部通过。检查入口改为串行执行 Rust 测试；没有放宽测试断言、产品超时或进程树保护，也不据此作出高负载时延保证。

## 未验证范围

- 真实 Codex／Qoder 评分、完整工具轨迹核验和同一会话设置恢复。
- 主指挥真实整体功能测试及跨 API、CLI、桌面接入的完整业务联调。
- 权重初值的统计校准。50%／50%、80%／90% 是用户批准的证据判据，不能解释为准确率、置信度或测试覆盖率。

工作台、运行核心和独立验收程序必须使用同一套构建。启用 reader21 后，旧程序不能用来继续处理该数据目录。当前源代码更新不改变已经发布安装包的版本或验证结论。

## English

This is a development candidate. Ninety component scenarios, four native model-selection scenarios and seven real independent-verifier protocol scenarios passed using synthetic project/model evidence. Native selection exercises Tauri, IPC, DPAPI, persisted role choices and fresh WebView processes. Thirty desktop-host tests passed; two environment-specific tests were ignored. Strict all-target workspace Clippy passed.

Regression fixes cover atomic archive publication, the current backup-writer allowlist and reader21 capability reporting. Failed native-fixture attempts are excluded from passing results. Real Codex/Qoder scoring, session-setting restoration, full cross-transport workflows and statistical threshold calibration remain unverified.

Six IPC/service tests and five cross-process cache scenarios passed. Reader21 passed a core guard regression and three checks using actual old/new executables: the old core refuses the protected data without changing it and the new core reopens it. The final release-mode GUI, core and verifier built successfully; all eight local package payloads and the pinned verifier identity matched. Seven protocol scenarios also passed against the final release-mode verifier.

The core library and argument parser passed 1,152 tests (299 ignored). Strict workflow integration passed 64 tests (13 ignored), and validator registration/execution passed 20 (6 ignored). The first strict-workflow run, concurrent with release compilation, had nine failures involving deadlines, process teardown and dependent state assertions. All nine passed in the subsequent serial run with unchanged assertions. That initial failure remains recorded; test serialization does not relax product limits or establish latency guarantees under contention.

Across the latest complete Rust target results, including the desktop host, 1,548 tests passed, 327 were ignored and none failed. Filtered follow-up runs are not counted twice. Contract export, TypeScript checking and Rust formatting also passed. The final native model-picker rerun passed all four cases with WebView caches inside the managed temporary job and both GUI processes closed normally.
