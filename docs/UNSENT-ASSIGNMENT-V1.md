# 未派发分配的显式处置

2026-10-08，P1/P2恢复组件。任务处于 Assigned 并不总表示尚未执行：历史 DispatchIntent 后读回 NotStarted 也会产生 Assigned。入口因此检查完整事件历史，而非只看当前状态。

## 用户行为与保证

需求与计划页的编程执行记录在核心确认条件满足时显示“已分配但尚未派发”。用户填写原因并确认后，保存 User 角色的 AbandonUnsent 事件；任务恢复 Pending，保留 fence，原尝试永久保留且不能复用。原方案和批准、Assign、Attempt、指挥响应及应用回执保持不变。新分配需要新副指挥决定、新 attempt，并递增 fence；项目调度仍需用户手动恢复。

这仅证明本工作台未为该尝试发布派发意图，不证明外部应用或用户没有另行执行。处置不生成远端取消回执、不退回指挥请求预算，也不自动重跑。曾有 DispatchIntent 的尝试，包括之后读回 NotStarted 的尝试，不能使用此入口。

## 核心约束

- Command 为 abandonUnsentAttempt，绑定 project/task/attempt/evidenceDigest/reason；原因非空、无NUL且最多8000 UTF-8字节。
- 当前任务必须 Assigned，原始 Attempt 必须存在，整个历史不得出现该 attempt 的 DispatchIntent。没有 CodexJob 的分配也能审阅，不能以缺少job推断已派发任务未执行。
- 项目不得处于 Active 调度；实际执行、验收或管理线程必须已经交还控制权。断开的 active 线程仍阻止关闭。其他未知编程/验收以及未完成管理应用也阻止操作。
- evidenceDigest 绑定工作流链头、Attempt、job、项目管理证据链头和运行授权。过期摘要拒绝。只读取管理链摘要，不复制全部历史问答。
- 处置命令在宿主入口跳过调度轮询；审核、落盘与状态更新在同一宿主写入锁内完成。关闭操作不会趁检查前推进任务。
- 工作流新增事件先持久化，随后更新可变 CodexJob 为 Abandoned。重启从事件恢复，单独篡改job状态不能生成成功处置；已有关闭事件而job仍Unknown也能修复。
- 旧管理operation仍关联旧job和Assign，不能借放弃重用同一响应再次执行。全部历史attempt ID禁止复用，跨任务、重新规划和重放后同样生效。
- 日交接投影保存原attempt及处置原因，标注等待后续新分配，避免误写远端Cancelled。

## 崩溃边界

Assign已持久化但管理Applied回执未写入：重开核心只依据原Assign补回原管理回执，然后重新审阅关闭。ApplyIntent已写而Assign根本不存在属于下一项专门恢复，不由此入口绕过。若工作流事件已提交而投影中断，核心必须重开恢复；不可在中毒账本上继续写入。

## 验证记录

最终常规Rust105、Node4通过：reports/p2-unsent-complete-checks-20261008.log。fmt及严格全workspace/all-targets clippy通过：reports/p2-unsent-final-clippy-20261008.log。新组件覆盖角色拒绝、过期摘要、活动调度/断线线程持有权、旧管理operation、旧attempt复用、新attempt预检、历史派发/NotStarted拒绝、原证据保留、fence递增、提交后崩溃、缺失管理回执恢复及伪造job状态。

发布/调试构建通过：reports/p2-unsent-build-20261008.log、p2-unsent-debug-build-20261008.log。原生有job场景为reports/unsent-assignment-native-1oCRRc/result.json，无job场景为reports/unsent-no-job-native-hzNaG5/result.json。两者均经过真实Tauri窗口、核心IPC和完整核心重启，验证空白原因/过期摘要拒绝、返回不保存、明确确认后仅新增1条事件、原管理文件及批准保持、重复处置拒绝、重启新增派发0。无job场景没有伪造job；已有job恢复为Abandoned。对话框、关闭结果及无job重开截图已目视检查，两个cleanup均退出0。

reports/p2-unsent-release-20261008/release-launch.json证明本机发布程序启动、正常关窗、关窗后常驻可读，日常新计划0；测试自建核心正常清理，既有用户核心不会被终止。

初次编译中共享测试helper可见性、测试访问私有方法的问题均已修复，原失败日志p2-unsent-tests/p2-unsent-tests-fixed-20261008.log保留。受限环境Vite spawn EPERM改为正常宿主运行通过；全量日志曾出现测试变量多余mut警告，已移除，最终严格clippy通过。源代码与证据摘要见reports/p2-unsent-review-20261008.json。

所有本批管理响应和故障输入为隔离合成夹具，真实模型调用0。该组件通过不等于完整P2或P6通过；真实OpenAI主副指挥、活动取消、纠偏、其他首批接入与安装发布门槛继续按NEXT-STAGE-GATES执行。
